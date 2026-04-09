use std::io;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::core::encryption::EncryptedMessage;

pub const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
pub const MAX_FRAME_SIZE: usize = 128 * 1024 * 1024;
pub const FRAME_CHUNK_SIZE: usize = 1024 * 1024;

/// Handshake messages used to establish encrypted connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeMessage {
    /// Initiator sends its public key for Noise handshake.
    HandshakeInit {
        public_key: [u8; 32],
    },
    /// Responder replies with its public key and handshake payload.
    HandshakeResp {
        public_key: [u8; 32],
        handshake_data: Vec<u8>,
    },
    /// Initiator sends final handshake payload.
    HandshakeFinish {
        handshake_data: Vec<u8>,
    },
    /// Signal that encryption is active; switch to encrypted messages.
    HandshakeComplete,
}

/// Application-level messages (same as v1, now wrapped in encryption).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    ConnectRequest {
        id: String,
        /// Session ID for tracking and reconnect support.
        session_id: Option<String>,
    },
    ConnectAccept {
        /// Session ID assigned by the server.
        session_id: String,
    },
    ConnectReject {
        reason: String,
    },
    /// Full frame (for first frame or when delta threshold exceeded).
    Frame {
        data: Vec<u8>,
    },
    /// Delta frame: list of changed regions.
    DeltaFrame {
        width: u32,
        height: u32,
        regions: Vec<DeltaRegion>,
    },
    /// Start of a streamed large frame.
    FrameStart {
        total_len: u32,
    },
    /// Chunk of a streamed large frame.
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
    /// Request to reconnect (server will send a fresh frame).
    ReconnectRequest,
    /// Heartbeat to keep connection alive and detect stale links.
    Heartbeat,
    Disconnect,
}

/// A changed region within a delta frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// A message envelope that carries either a handshake message or
/// an encrypted application message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    /// Unencrypted handshake message.
    Handshake(HandshakeMessage),
    /// Encrypted application message.
    Encrypted(EncryptedMessage),
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

/// Write a network message (handshake or encrypted) to the wire.
pub async fn write_network_message<W>(writer: &mut W, message: &NetworkMessage) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = bincode::serialize(message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

    if payload.len() > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "network message exceeds maximum allowed size",
        ));
    }

    writer.write_u32_le(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await
}

/// Read a network message from the wire.
pub async fn read_network_message<R>(reader: &mut R) -> io::Result<NetworkMessage>
where
    R: AsyncRead + Unpin,
{
    let size = reader.read_u32_le().await? as usize;
    if size > MAX_MESSAGE_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "received oversized network message",
        ));
    }

    let mut buffer = vec![0_u8; size];
    reader.read_exact(&mut buffer).await?;

    bincode::deserialize(&buffer)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}

/// Write an encrypted application message to the wire.
///
/// This serializes the message, encrypts it, wraps it in
/// `EncryptedMessage`, and sends it over the wire.
pub async fn write_encrypted_message<W, C>(
    writer: &mut W,
    message: &Message,
    cipher: &mut C,
) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    C: crate::core::encryption::Cipher,
{
    let plaintext = bincode::serialize(message)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;

    let ciphertext = cipher.encrypt(&plaintext)?;
    let encrypted = EncryptedMessage {
        nonce: vec![], // Nonce is implicit in the Noise cipher state.
        ciphertext,
    };

    write_network_message(writer, &NetworkMessage::Encrypted(encrypted)).await
}

/// Read and decrypt an application message from the wire.
pub async fn read_encrypted_message<R, C>(
    reader: &mut R,
    cipher: &mut C,
) -> io::Result<Message>
where
    R: AsyncRead + Unpin,
    C: crate::core::encryption::Cipher,
{
    let net_msg = read_network_message(reader).await?;
    let NetworkMessage::Encrypted(encrypted) = net_msg else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "expected encrypted message but got handshake",
        ));
    };

    let plaintext = cipher.decrypt(&encrypted.ciphertext)?;
    bincode::deserialize(&plaintext)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))
}
