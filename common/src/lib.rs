use std::{
    io::{Read, Write},
    marker::PhantomData,
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[cfg(feature = "async")]
use bytes::BytesMut;

#[cfg(feature = "async")]
use tokio_util::codec::{Decoder, Encoder, LengthDelimitedCodec};

pub const MDNS_SERVICE_NAME: &str = "_unity-cli._tcp.local.";
pub const PROJECT_PATH_PROP_KEY: &str = "project-path";
pub const PROJECT_NAME_PROP_KEY: &str = "project-name";
pub const UNITY_VERSION_PROP_KEY: &str = "unity-version";

#[derive(Debug, Deserialize, Serialize)]
pub enum ClientMessage {
    CommandRequest { cmd: String, args: Vec<String> },
}

#[derive(Debug, Deserialize, Serialize)]
pub enum UnityLogType {
    Error = 0,
    Assert = 1,
    Warning = 2,
    Log = 3,
    Exception = 4,
    Unknown = 5,
}

impl From<i32> for UnityLogType {
    fn from(value: i32) -> Self {
        match value {
            0 => Self::Error,
            1 => Self::Assert,
            2 => Self::Warning,
            3 => Self::Log,
            4 => Self::Exception,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub enum ServerMessage {
    UnityConsoleOutput {
        log_type: UnityLogType,
        log: String,
        stack_trace: String,
    },
    CompilationStarted,
    Compiling,
    CompilationFinished {},
    AssemblyUnloaded,
    AssemblyReloading,
    AssemblyReloaded,
    IsBusy,
    CommandFinished {
        is_success: bool,
        msg: Option<String>,
    },
}

#[cfg(feature = "sync")]
pub type ClientCodec = SyncHeteroCodec<ClientMessage, ServerMessage>;

#[cfg(feature = "async")]
pub type ServerCodec = AsyncHeteroCodec<ServerMessage, ClientMessage>;

#[cfg(feature = "sync")]
pub struct SyncHeteroCodec<T, U> {
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

#[cfg(feature = "sync")]
impl<T, U> SyncHeteroCodec<T, U> {
    pub fn new() -> Self {
        Self {
            _t: PhantomData::<_>,
            _u: PhantomData::<_>,
        }
    }
}

#[cfg(feature = "sync")]
impl<T, U> Default for SyncHeteroCodec<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "sync")]
impl<T, U> SyncHeteroCodec<T, U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    pub fn write<W: Write>(&self, item: &T, dst: &mut W) -> anyhow::Result<()> {
        let bytes = bincode::serialize(item)?;
        dst.write_all(&(bytes.len() as u32).to_be_bytes())?;
        dst.write_all(&bytes).map_err(anyhow::Error::new)
    }

    pub fn read<R: Read>(&self, src: &mut R) -> anyhow::Result<U> {
        let mut len_buf = [0_u8; 4];
        src.read_exact(&mut len_buf)?;
        let len = u32::from_be_bytes(len_buf) as usize;
        let mut buf = vec![0_u8; len];
        src.read_exact(&mut buf)?;
        bincode::deserialize(&buf).map_err(anyhow::Error::new)
    }
}

#[cfg(feature = "async")]
pub struct AsyncHeteroCodec<T, U> {
    inner: LengthDelimitedCodec,
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

#[cfg(feature = "async")]
impl<T, U> AsyncHeteroCodec<T, U> {
    pub fn new() -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .length_field_type::<u32>()
                .big_endian()
                .new_codec(),
            _t: PhantomData::<_>,
            _u: PhantomData::<_>,
        }
    }
}

#[cfg(feature = "async")]
impl<T, U> Default for AsyncHeteroCodec<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "async")]
impl<T, U> Encoder<T> for AsyncHeteroCodec<T, U>
where
    T: Serialize,
{
    type Error = anyhow::Error;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = bincode::serialize(&item)?;
        self.inner
            .encode(bytes.into(), dst)
            .map_err(anyhow::Error::new)
    }
}

#[cfg(feature = "async")]
impl<T, U> Decoder for AsyncHeteroCodec<T, U>
where
    U: DeserializeOwned,
{
    type Item = U;
    type Error = anyhow::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        self.inner
            .decode(src)?
            .map(|bytes| bincode::deserialize(&bytes).map_err(anyhow::Error::new))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures::{SinkExt, StreamExt};
    use tokio_util::codec::{FramedRead, FramedWrite};

    use super::*;

    #[tokio::test]
    async fn async_write_sync_read() -> anyhow::Result<()> {
        let test_impl = async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port = listener.local_addr()?.port();

            let handle = tokio::task::spawn_blocking(move || {
                let mut read = std::net::TcpStream::connect(format!("127.0.0.1:{}", port))?;
                let codec = ClientCodec::new();
                let msg = codec.read(&mut read)?;
                anyhow::Result::<ServerMessage>::Ok(msg)
            });

            let finish_msg = Some("Test message. 🤓\n".repeat(100));

            let (stream, _) = listener.accept().await?;
            let mut write = FramedWrite::new(stream, ServerCodec::new());
            write
                .send(ServerMessage::CommandFinished {
                    is_success: true,
                    msg: finish_msg.clone(),
                })
                .await?;

            let msg = handle.await??;

            assert!(
                matches!(msg, ServerMessage::CommandFinished { is_success, msg } if is_success && msg == finish_msg)
            );

            anyhow::Result::<()>::Ok(())
        };

        tokio::time::timeout(Duration::from_millis(10), test_impl).await??;

        Ok(())
    }

    #[tokio::test]
    async fn sync_write_async_read() -> anyhow::Result<()> {
        let test_impl = async {
            let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
            let port = listener.local_addr()?.port();

            let cmd = "foo".to_string();
            let args = vec!["--bar".to_string(), "42".to_string()];

            let msg = ClientMessage::CommandRequest {
                cmd: cmd.clone(),
                args: args.clone(),
            };

            let handle = tokio::task::spawn_blocking(move || {
                let (mut stream, _) = listener.accept()?;
                let codec = ClientCodec::new();
                codec.write(&msg, &mut stream)?;
                anyhow::Result::<()>::Ok(())
            });

            let stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await?;
            let mut read = FramedRead::new(stream, ServerCodec::new());
            let msg = read.next().await.expect("No msg received!")?;

            assert!(
                matches!(msg, ClientMessage::CommandRequest { cmd: cmd1, args: args1 } if cmd1 == cmd && args1 == args)
            );

            handle.await??;

            anyhow::Result::<()>::Ok(())
        };

        tokio::time::timeout(Duration::from_millis(10), test_impl).await??;

        Ok(())
    }
}
