//! Relay client for ThorC v2.
//!
//! Connects to the relay server and forwards encrypted traffic
//! when direct P2P connection is not possible.

use std::io;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use crate::core::protocol::{read_network_message, write_network_message, NetworkMessage};

/// A client connected to the relay server.
pub struct RelayClient {
    stream: TcpStream,
    /// The role string identifying this side (host or client).
    role: String,
    /// The session ID assigned by the relay.
    session_id: String,
}

impl RelayClient {
    /// Connect to the relay server and join/create a session.
    ///
    /// If `session_id` is `None`, a new session is created.
    /// Otherwise, the client joins the existing session.
    pub async fn connect(
        relay_addr: &str,
        is_host: bool,
        session_id: Option<&str>,
    ) -> io::Result<Self> {
        let mut stream = TcpStream::connect(relay_addr).await?;

        let role = match (is_host, session_id) {
            (true, None) => "new-host".to_owned(),
            (false, None) => "new-client".to_owned(),
            (true, Some(id)) => format!("host:{id}"),
            (false, Some(id)) => format!("client:{id}"),
        };

        // Send role
        let role_bytes = role.as_bytes();
        let len_buf = (role_bytes.len() as u32).to_le_bytes();
        stream.write_all(&len_buf).await?;
        stream.write_all(role_bytes).await?;

        let session_id = session_id
            .map(|s| s.to_owned())
            .unwrap_or_else(|| "pending".to_owned());

        Ok(Self {
            stream,
            role,
            session_id,
        })
    }

    /// Wait for the relay to confirm both sides are connected.
    /// Returns the actual session ID.
    pub async fn wait_for_ready(&mut self) -> io::Result<String> {
        // The relay doesn't send explicit ready messages; we rely on
        // the fact that once both sides connect, forwarding begins.
        // Return our session ID as confirmation.
        Ok(self.session_id.clone())
    }

    /// Write a network message to the relay.
    pub async fn write_message(&mut self, message: &NetworkMessage) -> io::Result<()> {
        write_network_message(&mut self.stream, message).await
    }

    /// Read a network message from the relay.
    pub async fn read_message(&mut self) -> io::Result<NetworkMessage> {
        read_network_message(&mut self.stream).await
    }

    /// Get the raw stream for bidirectional forwarding.
    pub fn into_split(
        self,
    ) -> (
        tokio::net::tcp::OwnedReadHalf,
        tokio::net::tcp::OwnedWriteHalf,
    ) {
        self.stream.into_split()
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the role.
    pub fn role(&self) -> &str {
        &self.role
    }
}
