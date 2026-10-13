//! Core-side drain client for the A2 restreamer.
//!
//! Core opens the stream to C2; C2 never dials core.

use hatchery_core::{
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
        Ok(DrainSession { stream, buf: Vec::new() })
    }
}

pub struct DrainSession {
    stream: TcpStream,
    /// Bytes read but not yet decoded (several envelopes may share one read).
    buf: Vec<u8>,
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

    /// Downlink: send opaque bytes to the node selected by `open` (C2 relays as-is).
    pub async fn send_data(&mut self, channel_id: u32, payload: Vec<u8>) -> Result<(), DrainError> {
        let frame = Frame::data(channel_id, 0, payload).map_err(DrainError::Frame)?;
        self.stream
            .write_all(&frame.encode().map_err(DrainError::Frame)?)
            .await?;
        Ok(())
    }

    /// Next drained envelope, or `None` on timeout / close. Non-DATA control frames
    /// from C2 (e.g. PONG) are skipped.
    pub async fn recv_envelope(
        &mut self,
        deadline: Duration,
    ) -> Result<Option<DrainEnvelope>, DrainError> {
        let until = tokio::time::Instant::now() + deadline;
        loop {
            if !self.buf.is_empty() {
                match DrainEnvelope::decode(&self.buf) {
                    Ok((env, used)) => {
                        self.buf.drain(..used);
                        return Ok(Some(env));
                    }
                    Err(FrameError::Truncated) => {}
                    Err(_) => {
                        // A bare frame (GOAWAY on auth failure, PONG): surface auth refusal.
                        if let Ok((frame, used)) = Frame::decode(&self.buf) {
                            self.buf.drain(..used);
                            if frame.frame_type == hatchery_core::FrameType::GoAway {
                                return Err(DrainError::Auth);
                            }
                            continue;
                        }
                        return Err(DrainError::Frame(FrameError::Io("bad drain envelope".into())));
                    }
                }
            }
            let mut chunk = vec![0u8; 64 * 1024];
            let left = until.saturating_duration_since(tokio::time::Instant::now());
            match tokio::time::timeout(left, self.stream.read(&mut chunk)).await {
                Ok(Ok(0)) => return Ok(None),
                Ok(Ok(n)) => self.buf.extend_from_slice(&chunk[..n]),
                Ok(Err(e)) => return Err(e.into()),
                Err(_) => return Ok(None),
            }
        }
    }
}
