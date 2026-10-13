//! Persistent uplink journal and JournalReq responder for the A2 restreamer.
//!
//! The node appends opaque payloads locally. When C2 asks for a gap range over
//! the multiplex uplink, this module serves `JournalReply` from disk.

use hatchery_core::FileJournal;
use hatchery_core::{
    ChannelKind, Frame, FrameError, FrameType, OpenPayload,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Node-side journal bound to a restreamer uplink.
pub struct UplinkJournal {
    node_id: String,
    journal: Arc<FileJournal>,
}

impl UplinkJournal {
    pub fn open(node_id: impl Into<String>, path: impl Into<PathBuf>) -> Result<Self, String> {
        let journal = FileJournal::open(path.into()).map_err(|e| e.to_string())?;
        Ok(Self {
            node_id: node_id.into(),
            journal: Arc::new(journal),
        })
    }

    pub fn append(&self, channel_id: u32, payload: &[u8]) -> Result<u64, String> {
        self.journal
            .append(channel_id, payload)
            .map_err(|e| e.to_string())
    }

    pub fn journal(&self) -> &FileJournal {
        &self.journal
    }

    /// Dial the restreamer node listen address, OPEN a channel, and answer JournalReq.
    pub async fn serve_uplink(
        self: Arc<Self>,
        restreamer_addr: std::net::SocketAddr,
        channel_id: u32,
        kind: ChannelKind,
    ) -> Result<(), String> {
        let mut stream = TcpStream::connect(restreamer_addr)
            .await
            .map_err(|e| e.to_string())?;
        let open = Frame::open(
            channel_id,
            &OpenPayload {
                channel_kind: kind,
                node_id: self.node_id.clone(),
            },
        )
        .map_err(|e| e.to_string())?;
        stream
            .write_all(&open.encode().map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut filled = 0usize;
        loop {
            let n = stream
                .read(&mut buf[filled..])
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            filled += n;
            while filled > 0 {
                match Frame::decode(&buf[..filled]) {
                    Ok((frame, consumed)) => {
                        buf.copy_within(consumed..filled, 0);
                        filled -= consumed;
                        match frame.frame_type {
                            FrameType::JournalReq => {
                                let limit = frame.journal_req_limit().unwrap_or(64) as usize;
                                let body = self
                                    .journal
                                    .reply_body(frame.channel_id, frame.seq, limit)
                                    .map_err(|e| e.to_string())?;
                                let reply = Frame::journal_reply(frame.channel_id, &body)
                                    .map_err(|e| e.to_string())?;
                                stream
                                    .write_all(&reply.encode().map_err(|e| e.to_string())?)
                                    .await
                                    .map_err(|e| e.to_string())?;
                            }
                            FrameType::Window | FrameType::Ping | FrameType::Pong => {}
                            FrameType::GoAway | FrameType::Close => return Ok(()),
                            _ => {}
                        }
                    }
                    Err(FrameError::Truncated) => break,
                    Err(e) => return Err(e.to_string()),
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_persist() {
        let dir = std::env::temp_dir().join(format!("node-uj-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let j = UplinkJournal::open("node-a", dir.join("j")).unwrap();
        assert_eq!(j.append(1, b"x").unwrap(), 1);
        assert_eq!(j.append(1, b"y").unwrap(), 2);
        let body = j.journal().reply_body(1, 0, 10).unwrap();
        assert_eq!(body.entries.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
