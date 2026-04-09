//! Minimal relay server for ThorC v2.
//!
//! Used as a fallback when direct P2P connection fails.
//! The relay is stateless with respect to session content — it simply
//! forwards encrypted packets between two peers.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Maximum message size the relay will forward.
const MAX_RELAY_MSG: usize = 128 * 1024 * 1024;

/// A single relayed session between a host and a client.
struct RelaySession {
    host: Option<TcpStream>,
    client: Option<TcpStream>,
}

impl RelaySession {
    fn new() -> Self {
        Self {
            host: None,
            client: None,
        }
    }

    fn is_ready(&self) -> bool {
        self.host.is_some() && self.client.is_some()
    }
}

/// Shared state for the relay server.
struct RelayState {
    sessions: HashMap<String, RelaySession>,
    next_session_id: u64,
}

impl RelayState {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            next_session_id: 0,
        }
    }

    fn create_session(&mut self) -> String {
        let id = format!("relay-{}", self.next_session_id);
        self.next_session_id += 1;
        self.sessions.insert(id.clone(), RelaySession::new());
        id
    }

    fn get_session(&self, id: &str) -> Option<&RelaySession> {
        self.sessions.get(id)
    }

    fn get_session_mut(&mut self, id: &str) -> Option<&mut RelaySession> {
        self.sessions.get_mut(id)
    }

    fn remove_session(&mut self, id: &str) {
        self.sessions.remove(id);
    }
}

/// Run the relay server on the given address.
///
/// Protocol:
/// 1. First message from either side is a session join/create command.
/// 2. Once both host and client join a session, the relay forwards all
///    subsequent messages between them.
/// 3. If either side disconnect, the session is torn down.
pub async fn run_relay_server(listen_addr: &str) -> io::Result<()> {
    let listener = TcpListener::bind(listen_addr).await?;
    let state = Arc::new(Mutex::new(RelayState::new()));

    eprintln!("[relay] Listening on {listen_addr}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        eprintln!("[relay] Accepted connection from {peer_addr}");

        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_connection(state, stream).await {
                eprintln!("[relay] Connection error: {err}");
            }
        });
    }
}

async fn handle_connection(state: Arc<Mutex<RelayState>>, mut stream: TcpStream) -> io::Result<()> {
    // Read the role: "host:<session_id>" or "client:<session_id>" or "new"
    let mut role_buf = [0u8; 4];
    stream.read_exact(&mut role_buf).await?;
    let role_len = u32::from_le_bytes(role_buf) as usize;

    let mut role_data = vec![0u8; role_len];
    stream.read_exact(&mut role_data).await?;
    let role_str = String::from_utf8_lossy(&role_data);

    let (session_id, is_host) = parse_role(&role_str, &state)?;

    // Attach this connection to the session
    let session_ready = {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;

        if let Some(session) = state.get_session_mut(&session_id) {
            if is_host {
                session.host = Some(stream);
            } else {
                session.client = Some(stream);
            }

            session.is_ready()
        } else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "session not found",
            ));
        }
    };

    if !session_ready {
        return wait_for_peer(state.clone(), &session_id, is_host).await;
    }

    // Both sides present; start bidirectional forwarding
    forward_between_peers(&state, &session_id, is_host).await
}

fn parse_role(
    role_str: &str,
    state: &Arc<Mutex<RelayState>>,
) -> io::Result<(String, bool)> {
    if role_str == "new-host" {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        let id = state.create_session();
        Ok((id, true))
    } else if role_str == "new-client" {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        let id = state.create_session();
        Ok((id, false))
    } else if let Some(id) = role_str.strip_prefix("host:") {
        Ok((id.to_owned(), true))
    } else if let Some(id) = role_str.strip_prefix("client:") {
        Ok((id.to_owned(), false))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid role specification: {role_str}"),
        ))
    }
}

async fn wait_for_peer(
    state: Arc<Mutex<RelayState>>,
    session_id: &str,
    _is_host: bool,
) -> io::Result<()> {
    // Poll until the other side joins or we timeout
    let session_id = session_id.to_owned();
    for _ in 0..60 {
        // Wait up to 60 seconds
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let ready = state
            .lock()
            .map(|s| s.get_session(&session_id).map(|s| s.is_ready()).unwrap_or(false))
            .unwrap_or(false);
        if ready {
            return Ok(());
        }
    }

    // Timeout: clean up
    state
        .lock()
        .map(|mut s| s.remove_session(&session_id))
        .ok();

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "timed out waiting for peer",
    ))
}

async fn forward_between_peers(
    state: &Arc<Mutex<RelayState>>,
    session_id: &str,
    _is_host: bool,
) -> io::Result<()> {
    let (host, client) = {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;

        let session = state
            .get_session_mut(session_id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "session not found"))?;

        (
            session.host.take().expect("host must exist"),
            session.client.take().expect("client must exist"),
        )
    };

    let (mut host_r, mut host_w) = host.into_split();
    let (mut client_r, mut client_w) = client.into_split();

    // Forward host -> client
    let h2c = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match host_r.read(&mut buf).await {
                Ok(0) => return Ok(()), // EOF
                Ok(n) => {
                    if let Err(e) = client_w.write_all(&buf[..n]).await {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    });

    // Forward client -> host
    let c2h = tokio::spawn(async move {
        let mut buf = [0u8; 65536];
        loop {
            match client_r.read(&mut buf).await {
                Ok(0) => return Ok(()), // EOF
                Ok(n) => {
                    if let Err(e) = host_w.write_all(&buf[..n]).await {
                        return Err(e);
                    }
                }
                Err(e) => return Err(e),
            }
        }
    });

    // Wait for either direction to fail
    let result = tokio::select! {
        r = h2c => r,
        r = c2h => r,
    };

    // Clean up session
    state
        .lock()
        .map(|mut s| s.remove_session(session_id))
        .ok();

    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string())),
    }
}

/// Parse a session ID from a role string (utility for client code).
pub fn parse_session_from_role(role: &str) -> Option<&str> {
    if let Some(id) = role.strip_prefix("host:") {
        return Some(id);
    }
    if let Some(id) = role.strip_prefix("client:") {
        return Some(id);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_role_new_host() {
        let state = Arc::new(Mutex::new(RelayState::new()));
        let (id, is_host) = parse_role("new-host", &state).unwrap();
        assert!(is_host);
        assert_eq!(id, "relay-0");
    }

    #[test]
    fn test_parse_role_existing() {
        let state = Arc::new(Mutex::new(RelayState::new()));
        let (id, is_host) = parse_role("host:my-session", &state).unwrap();
        assert!(is_host);
        assert_eq!(id, "my-session");

        let (id, is_host) = parse_role("client:other-session", &state).unwrap();
        assert!(!is_host);
        assert_eq!(id, "other-session");
    }

    #[test]
    fn test_parse_session_from_role() {
        assert_eq!(parse_session_from_role("host:abc123"), Some("abc123"));
        assert_eq!(parse_session_from_role("client:xyz789"), Some("xyz789"));
        assert_eq!(parse_session_from_role("invalid"), None);
    }
}
