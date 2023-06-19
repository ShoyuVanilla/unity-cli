use std::{
    ffi::{CStr, CString},
    net::SocketAddr,
    os::raw::c_char,
    sync::{Arc, OnceLock},
};

use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use gethostname::gethostname;
use mdns_sd::{IPMulticastTTLOption, ServiceDaemon, ServiceInfo};
use socket2::{Domain, Socket, Type};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpListener,
    },
    runtime::Builder,
    sync::RwLock,
};
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing::{error, info, info_span, trace, Instrument};
use uuid::Uuid;

use common::{
    ClientMessage, ServerCodec, ServerMessage, PROJECT_NAME_PROP_KEY, PROJECT_PATH_PROP_KEY,
    UNITY_VERSION_PROP_KEY,
};

struct Instance {
    stop_tx: tokio::sync::mpsc::Sender<()>,
    unity_msg_send: tokio::sync::mpsc::UnboundedSender<(Uuid, ServerMessage)>,
}

static INSTANCE: OnceLock<RwLock<Option<Instance>>> = OnceLock::new();

fn instance() -> &'static RwLock<Option<Instance>> {
    INSTANCE.get_or_init(|| RwLock::new(None))
}

type UnityCommandCallback = extern "C" fn(u64, u64, *const c_char, *const *const c_char, i32);

struct UnityState {
    cmd_cb: UnityCommandCallback,
}

static UNITY_STATE: OnceLock<RwLock<Option<UnityState>>> = OnceLock::new();

fn unity_state() -> &'static RwLock<Option<UnityState>> {
    UNITY_STATE.get_or_init(|| RwLock::new(None))
}

#[inline(always)]
fn c_char_to_str(ptr: *const c_char) -> String {
    unsafe {
        let s = CStr::from_ptr(ptr);
        s.to_string_lossy().into_owned()
    }
}

#[no_mangle]
pub extern "C" fn run(
    project_path: *const c_char,
    project_name: *const c_char,
    unity_version: *const c_char,
    command_callback: UnityCommandCallback,
) {
    *unity_state().blocking_write() = Some(UnityState {
        cmd_cb: command_callback,
    });

    let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel(1);
    let (unity_msg_tx, mut unity_msg_rx) = tokio::sync::mpsc::unbounded_channel();

    {
        let mut instance = instance().blocking_write();
        if instance.is_some() {
            return;
        } else {
            *instance = Some(Instance {
                stop_tx,
                unity_msg_send: unity_msg_tx,
            });
        }
    }

    // TODO: tracing_appender support, configurability
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::DEBUG)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .init();

    let project_path = c_char_to_str(project_path);
    let project_name = c_char_to_str(project_name);
    let unity_version = c_char_to_str(unity_version);

    std::thread::spawn(move || {
        struct GlobalStatesGuard;

        impl Drop for GlobalStatesGuard {
            fn drop(&mut self) {
                *instance().blocking_write() = None;
                *unity_state().blocking_write() = None;
            }
        }

        let _guard = GlobalStatesGuard;

        let socket = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let addr = addr.into();
        socket.bind(&addr).unwrap();
        socket.listen(128).unwrap();
        socket.set_keepalive(true).unwrap();

        let listener: std::net::TcpListener = socket.into();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();

        let mdns_daemon = ServiceDaemon::new(IPMulticastTTLOption::NodeLocal).unwrap();
        let service_type = common::MDNS_SERVICE_NAME;
        let instance_name = names::Generator::default().next().unwrap();
        let host_ipv4 = "";
        let host_name = gethostname();
        let properties = [
            (PROJECT_PATH_PROP_KEY, &project_path),
            (PROJECT_NAME_PROP_KEY, &project_name),
            (UNITY_VERSION_PROP_KEY, &unity_version),
        ];
        let service_info = ServiceInfo::new(
            service_type,
            &instance_name,
            host_name.to_string_lossy().as_ref(),
            host_ipv4,
            port,
            &properties[..],
        )
        .unwrap()
        .enable_addr_auto();
        mdns_daemon
            .register(service_info)
            .expect("Failed to register our service");

        let rt = Builder::new_multi_thread().enable_io().build().unwrap();
        rt.block_on(async move {
            let listener = TcpListener::from_std(listener).unwrap();
            let conns: Arc<DashMap<Uuid, tokio::sync::mpsc::Sender<ServerMessage>>> =
                Arc::new(DashMap::new());
            let conns2 = conns.clone();
            let conns3 = conns.clone();
            let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(10);

            let accept_conn_loop = async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, _)) => {
                            let (read, write) = stream.into_split();
                            let read = FramedRead::new(read, ServerCodec::default());
                            let write = FramedWrite::new(write, ServerCodec::default());
                            let (msg_tx, msg_rx) = tokio::sync::mpsc::channel(8);
                            let uuid = Uuid::new_v4();
                            conns2.insert(uuid, msg_tx);
                            let cmd_tx = cmd_tx.clone();
                            let conns = conns3.clone();
                            let on_finish = move || {
                                conns.remove(&uuid);
                            };

                            tokio::spawn(async move {
                                handle_read(read, uuid, cmd_tx)
                                    .instrument(info_span!("handle_read", %uuid))
                                    .await;
                            });
                            tokio::spawn(async move {
                                handle_write(write, msg_rx, on_finish)
                                    .instrument(info_span!("handle_write", %uuid))
                                    .await;
                            });
                        }
                        Err(_e) => {}
                    }
                }
            }
            .instrument(info_span!("accept_conn_loop"));

            let route_msg_from_unity_loop = async move {
                loop {
                    match unity_msg_rx.recv().await {
                        Some((uuid, msg)) => {
                            if let Some(msg_tx) = conns.get(&uuid) {
                                if msg_tx.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
            .instrument(info_span!("route_msg_from_unity_loop"));

            let send_cmd_to_unity_loop = async move {
                loop {
                    match cmd_rx.recv().await {
                        Some((uuid, cmd, args)) => {
                            if let Some(unity_state) = unity_state().read().await.as_ref() {
                                let (uuid_hi, uuid_lo) = uuid.as_u64_pair();
                                let cmd = CString::new(cmd).unwrap().into_raw();
                                let args: Vec<_> = args
                                    .iter()
                                    .map(|s| {
                                        CString::new(s.as_str()).unwrap().into_raw()
                                            as *const c_char
                                    })
                                    .collect();

                                // Send the command to Unity C# script
                                (unity_state.cmd_cb)(
                                    uuid_hi,
                                    uuid_lo,
                                    cmd,
                                    args.as_ptr(),
                                    args.len() as i32,
                                );

                                // Free allocated strings
                                unsafe {
                                    drop(CString::from_raw(cmd));
                                    for ptr in args {
                                        drop(CString::from_raw(ptr as *mut c_char));
                                    }
                                }
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
            .instrument(info_span!("send_cmd_to_unity_loop"));

            tokio::select! {
                _ = accept_conn_loop => {}
                _ = route_msg_from_unity_loop => {}
                _ = send_cmd_to_unity_loop => {}
                _ = stop_rx.recv() => {
                    info!("stopped from unity.");
                }
            }
        });
    });
}

async fn handle_read(
    mut read: FramedRead<OwnedReadHalf, ServerCodec>,
    uuid: Uuid,
    cmd_tx: tokio::sync::mpsc::Sender<(Uuid, String, Vec<String>)>,
) {
    loop {
        match read.next().await {
            Some(Ok(ClientMessage::CommandRequest { cmd, args })) => {
                if let Err(e) = cmd_tx.send((uuid, cmd, args)).await {
                    error!(error = %e, "failed to send client command request through channel!");
                    break;
                }
            }
            Some(Err(e)) => {
                error!(error = %e, "failed to deserialize client message!");
                break;
            }
            None => {
                trace!("stream closed.");
                break;
            }
        }
    }
}

async fn handle_write<F>(
    mut write: FramedWrite<OwnedWriteHalf, ServerCodec>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<ServerMessage>,
    on_finish: F,
) where
    F: FnMut(),
{
    struct ReleaseGuard<G>
    where
        G: FnMut(),
    {
        on_finish: G,
    }

    impl<G> Drop for ReleaseGuard<G>
    where
        G: FnMut(),
    {
        fn drop(&mut self) {
            (self.on_finish)();
        }
    }

    let _guard = ReleaseGuard { on_finish };

    loop {
        match cmd_rx.recv().await {
            Some(msg) => {
                if let Err(e) = write.send(msg).await {
                    error!(error = %e, "failed to send server message!");
                    break;
                }
            }
            None => {
                trace!("channel closed.");
                break;
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn stop() {
    if let Some(ref mut instance) = instance().blocking_write().as_mut() {
        let _ = instance.stop_tx.blocking_send(());
    }
}

#[no_mangle]
pub extern "C" fn is_running() -> bool {
    instance().blocking_read().is_some()
}

#[no_mangle]
pub unsafe extern "C" fn on_unity_console_log(
    uuid_hi: u64,
    uuid_lo: u64,
    log_type: i32,
    log: *const c_char,
    stack_trace: *const c_char,
) -> bool {
    if let Some(instance) = instance().blocking_read().as_ref() {
        let log = c_char_to_str(log);
        let stack_trace = c_char_to_str(stack_trace);
        let msg = ServerMessage::UnityConsoleOutput {
            log_type: log_type.into(),
            log,
            stack_trace,
        };
        instance
            .unity_msg_send
            .send((Uuid::from_u64_pair(uuid_hi, uuid_lo), msg))
            .is_ok()
    } else {
        false
    }
}

#[no_mangle]
pub extern "C" fn on_command_finish(
    uuid_hi: u64,
    uuid_lo: u64,
    is_success: bool,
    result: *const c_char,
) {
    if let Some(instance) = instance().blocking_read().as_ref() {
        let result = if result.is_null() {
            None
        } else {
            Some(c_char_to_str(result))
        };
        let _ = instance.unity_msg_send.send((
            Uuid::from_u64_pair(uuid_hi, uuid_lo),
            ServerMessage::CommandFinished {
                is_success,
                msg: result,
            },
        ));
    }
}

#[no_mangle]
pub extern "C" fn on_csharp_assembly_unload() {
    *unity_state().blocking_write() = None;
}
