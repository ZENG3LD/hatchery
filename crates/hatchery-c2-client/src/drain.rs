//! Core-side drain client for the A2 restreamer.
//!
//! Core opens the stream to C2; C2 never dials core.

use hatchery_c2_protocol::{
    ChannelKind, DrainEnvelope, Frame, FrameError, OpenPayload,
};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Debug)]
pub enum DrainError {
    Io(std::io::Error),
    Frame(FrameError),
    Auth,
}

impl std::fmt::Display for DrainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Frame(e) => write!(f, "{e}"),
            Self::Auth => write!(f, "drain auth refused"),
        }
    }
}

impl std::error::Error for DrainError {}

impl From<std::io::Error> for DrainError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct DrainClient {
    addr: SocketAddr,
    token: String,
}

impl DrainClient {
    pub fn new(addr: SocketAddr, token: impl Into<String>) -> Self {
        Self {
            addr,
            token: token.into(),
        }
    }

    pub async fn connect(&self) -> Result<DrainSession, DrainError> {
        let mut stream = TcpStream::connect(self.addr).await?;
        stream.write_all(self.token.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        Ok(DrainSession { stream })
    }
}

pub struct DrainSession {
    stream: TcpStream,
}

impl DrainSession {
    pub async fn open(
        &mut self,
        node_id: &str,
        channel_id: u32,
        kind: ChannelKind,
    ) -> Result<(), DrainError> {
        let frame = Frame::open(
            channel_id,
            &OpenPayload {
                channel_kind: kind,
                node_id: node_id.to_owned(),
            },
        )
        .map_err(DrainError::Frame)?;
        self.stream
            .write_all(&frame.encode().map_err(DrainError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn resume(&mut self, after: u64) -> Result<(), DrainError> {
        self.stream
            .write_all(&Frame::resume(after).encode().map_err(DrainError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn ack(&mut self, seq: u64) -> Result<(), DrainError> {
        self.stream
            .write_all(&Frame::ack(seq).encode().map_err(DrainError::Frame)?)
            .await?;
        Ok(())
    }

    pub async fn recv_envelope(
        &mut self,
        deadline: Duration,
    ) -> Result<Option<DrainEnvelope>, DrainError> {
        let mut buf = vec![0u8; 64 * 1024];
        let n = match tokio::time::timeout(deadline, self.stream.read(&mut buf)).await {
            Ok(Ok(0)) => return Ok(None),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => return Ok(None),
        };
        let (env, _) = DrainEnvelope::decode(&buf[..n]).map_err(DrainError::Frame)?;
        Ok(Some(env))
    }
}
