use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
pub const MAX_FRAME_SIZE: usize = 128 * 1024 * 1024;
pub const FRAME_CHUNK_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    ConnectRequest {
        id: String,
    },
    ConnectAccept,
    ConnectReject {
        reason: String,
    },
    Frame {
        data: Vec<u8>,
    },
    FrameStart {
        total_len: u32,
    },
    FrameChunk {
        data: Vec<u8>,
    },
    MouseEvent {
        x: i32,
        y: i32,
        button: String,
    },
    MouseScroll {
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
    },
    KeyboardEvent {
        key: String,
    },
    Disconnect,
}

pub async fn write_message<W>(writer: &mut W, message: &Message) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = bincode::serialize(message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message exceeds maximum allowed size",
        ));
    }

    writer.write_u32_le(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

pub async fn read_message<R>(reader: &mut R) -> io::Result<Message>
where
    R: AsyncRead + Unpin,
{
    let size = reader.read_u32_le().await? as usize;
    if size > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "received oversized message",
        ));
    }

    let mut buffer = vec![0_u8; size];
    reader.read_exact(&mut buffer).await?;

    bincode::deserialize(&buffer)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}
