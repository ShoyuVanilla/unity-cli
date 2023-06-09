use std::marker::PhantomData;

use anyhow::anyhow;
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

impl<T, U> Encoder<T> for HeteroCodec<T, U>
where
    T: Serialize,
{
    type Error = anyhow::Error;

    fn encode(&mut self, item: T, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let bytes = bincode::serialize(&item)?;
        self.inner.encode(bytes.into(), dst).map_err(|e| anyhow!(e))
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
            .map(|bytes| bincode::deserialize(&bytes).map_err(|e| anyhow!(e)))
            .transpose()
    }
}

pub type ClientCodec = HeteroCodec<ClientMessage, ServerMessage>;

pub type ServerCodec = HeteroCodec<ServerMessage, ClientMessage>;
