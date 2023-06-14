use std::{
    ffi::{c_char, CStr, CString},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use common::{
    ClientCodec, ClientMessage, ServerMessage, PROJECT_NAME_PROP_KEY, PROJECT_PATH_PROP_KEY,
    UNITY_VERSION_PROP_KEY,
};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use parking_lot::{Condvar, Mutex};

type Command = (u64, u64, String, Vec<String>);

fn ptr_to_string(ptr: *const c_char) -> String {
    unsafe { CStr::from_ptr(ptr).to_string_lossy().to_string() }
}

fn str_to_ptr<T: AsRef<str>>(s: &T) -> *const c_char {
    CString::new(s.as_ref()).unwrap().into_raw()
}

#[test]
fn general_use_case() {
    const PROJECT_PATH: &str = "foo/bar/baz";
    let project_path_cstr = CString::new(PROJECT_PATH).unwrap();
    const PROJECT_NAME: &str = "My Unity Project";
    let project_name_cstr = CString::new(PROJECT_NAME).unwrap();
    const UNITY_VERSION: &str = "2023.5.30";
    let unity_version_cstr = CString::new(UNITY_VERSION).unwrap();

    static COMMANDS: Mutex<Vec<Command>> = Mutex::new(Vec::new());

    extern "C" fn cmd_cb(
        u1: u64,
        u2: u64,
        cmd: *const c_char,
        args: *const *const c_char,
        args_len: i32,
    ) {
        let cmd = ptr_to_string(cmd);
        let args_len = args_len as usize;
        let slice = unsafe { std::slice::from_raw_parts(args, args_len) };
        let args = (0..args_len).map(|i| ptr_to_string(slice[i])).collect();
        dbg!((&cmd, &args));
        COMMANDS.lock().push((u1, u2, cmd, args));
    }

    COMMANDS.lock().clear();

    ucli_server::run(
        project_path_cstr.into_raw(),
        project_name_cstr.into_raw(),
        unity_version_cstr.into_raw(),
        cmd_cb,
    );

    assert!(ucli_server::is_running());

    fn discover_and_connect(port_cvar: Arc<(Mutex<Option<u16>>, Condvar)>) {
        let mdns = ServiceDaemon::new(mdns_sd::IPMulticastTTLOption::NodeLocal).unwrap();
        let receiver = mdns.browse(common::MDNS_SERVICE_NAME).unwrap();
        while let Ok(event) = receiver.recv() {
            dbg!(&event);
            if let ServiceEvent::ServiceResolved(info) = event {
                dbg!(&info);
                if (
                    info.get_property_val_str(PROJECT_PATH_PROP_KEY),
                    info.get_property_val_str(PROJECT_NAME_PROP_KEY),
                    info.get_property_val_str(UNITY_VERSION_PROP_KEY),
                ) != (Some(PROJECT_PATH), Some(PROJECT_NAME), Some(UNITY_VERSION))
                {
                    continue;
                }
                dbg!(info.get_fullname());
                let (lock, cvar) = &*port_cvar;
                *lock.lock() = Some(info.get_port());
                cvar.notify_one();
            }
        }
    }

    let pair = Arc::new((Mutex::new(None), Condvar::new()));
    let pair2 = pair.clone();

    std::thread::spawn(move || {
        discover_and_connect(pair2);
    });

    let (port_a, cvar): &(
        parking_lot::lock_api::Mutex<parking_lot::RawMutex, Option<u16>>,
        Condvar,
    ) = &pair;
    let wait_timeout_result = cvar.wait_for(&mut port_a.lock(), Duration::from_millis(5000));

    assert!(!wait_timeout_result.timed_out(), "Cannot find service!");

    let pair = Arc::new((Mutex::new(None), Condvar::new()));
    let pair2 = pair.clone();

    std::thread::spawn(move || {
        discover_and_connect(pair2);
    });

    let (port_b, cvar): &(
        parking_lot::lock_api::Mutex<parking_lot::RawMutex, Option<u16>>,
        Condvar,
    ) = &pair;
    let wait_timeout_result = cvar.wait_for(&mut port_a.lock(), Duration::from_millis(5000));

    assert!(!wait_timeout_result.timed_out(), "Cannot find service!");

    let port_a = port_a.lock().unwrap();
    let port_b = port_b.lock().unwrap();

    assert_eq!(port_a, port_b);

    let mut conn_a = TcpStream::connect(format!("127.0.0.1:{}", port_a)).unwrap();
    let mut conn_b = TcpStream::connect(format!("127.0.0.1:{}", port_b)).unwrap();

    let cmd = "foo".to_string();
    let args = vec!["bar".to_string(), "baz".to_string()];
    let msg = ClientMessage::CommandRequest {
        cmd: "foo".to_string(),
        args: vec!["bar".to_string(), "baz".to_string()],
    };
    ClientCodec::default().write(&msg, &mut conn_a).unwrap();

    std::thread::sleep(Duration::from_millis(100));

    assert!(ucli_server::is_running());
    assert_eq!(1, COMMANDS.lock().len());
    let (id_hi_a, id_lo_a, cmd_recvd, args_recvd) = COMMANDS.lock().remove(0);
    assert_eq!((cmd_recvd, args_recvd), (cmd, args));

    let log_a = "log to connection A";
    let st_a = "some stack trace A".repeat(100);
    let log_a_ptr = str_to_ptr(&log_a);
    let st_a_ptr = str_to_ptr(&st_a);
    unsafe {
        ucli_server::on_unity_console_log(id_hi_a, id_lo_a, 0, log_a_ptr, st_a_ptr);
    }

    std::thread::sleep(Duration::from_millis(100));

    let msg = ClientCodec::default().read(&mut conn_a);
    match msg {
        Ok(ServerMessage::UnityConsoleOutput {
            log_type: _,
            log,
            stack_trace,
        }) => {
            assert_eq!((log.as_str(), stack_trace.as_str()), (log_a, st_a.as_ref()));
        }
        _ => {
            panic!();
        }
    }

    unsafe {
        drop(CString::from_raw(log_a_ptr as *mut c_char));
        drop(CString::from_raw(st_a_ptr as *mut c_char));
    }

    ucli_server::stop();
    std::thread::sleep(Duration::from_millis(50));

    assert!(!ucli_server::is_running());
}
