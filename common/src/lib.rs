use std::{marker::PhantomData, io::{Read, Write}};

use bytes::BytesMut;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
    IsBusy,
    UnityConsoleOutput {
        log_type: UnityLogType,
        log: String,
        stack_trace: String,
    },
    CommandFinished {
        is_success: bool,
        result: Option<String>,
    },
}

pub struct HeteroCodec<T, U> {
    inner: LengthDelimitedCodec,
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

impl<T, U> Default for HeteroCodec<T, U> {
    fn default() -> Self {
        Self {
            inner: LengthDelimitedCodec::default(),
            _t: PhantomData::<_>,
            _u: PhantomData::<_>,
        }
    }
}

// TODO: u32, Big Endian
impl<T, U> Encoder<T> for HeteroCodec<T, U>
where
    T: Serialize,
{
    type Error = anyhow::Error;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = bincode::serialize(&item)?;
        self.inner.encode(bytes.into(), dst).map_err(anyhow::Error::new)
    }
}

impl<T, U> Decoder for HeteroCodec<T, U>
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

pub type ClientCodec = HeteroCodec<ClientMessage, ServerMessage>;

pub type ServerCodec = HeteroCodec<ServerMessage, ClientMessage>;

pub struct HeteroSyncCodec<T, U> {
    _t: PhantomData<T>,
    _u: PhantomData<U>,
}

impl<T, U> Default for HeteroSyncCodec<T, U> {
    fn default() -> Self {
        Self {
            _t: PhantomData::<_>,
            _u: PhantomData::<_>,
        }
    }
}

impl<T, U> HeteroSyncCodec<T, U>
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

#[cfg(test)]
mod test {
    use futures::SinkExt;
    use tokio_util::codec::FramedWrite;

    use super::*;

    #[tokio::test]
    async fn async_write_sync_read() -> anyhow::Result<()> {
        let sender = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let port = sender.local_addr()?.port();

        let handle = std::thread::spawn(move || {
            let mut reader = std::net::TcpStream::connect(format!("127.0.0.1:{}", port))?;
            let codec = HeteroSyncCodec::<ClientMessage, ServerMessage>::default();
            let msg = codec.read(&mut reader)?;
            anyhow::Result::<ServerMessage>::Ok(msg)
        });

        let (stream, _) = sender.accept().await?;
        let mut write = FramedWrite::new(stream, ServerCodec::default());
        write.send(ServerMessage::CommandFinished { is_success: true, result: Some("Foo".into()) }).await?;

        let msg = handle.join().unwrap()?;
        assert!(matches!(msg, ServerMessage::CommandFinished { is_success, result } if is_success && result == Some("Foo".into())));

        Ok(())
    }
}
