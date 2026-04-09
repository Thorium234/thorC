use std::time::{Duration, Instant};

use uuid::Uuid;

/// Unique identifier for a remote desktop session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_str(&self) -> String {
        self.0.to_string()
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks the lifecycle state of a connection session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Session is being established (handshake in progress).
    Connecting,
    /// Session is active and transmitting data.
    Active,
    /// Session was disconnected gracefully.
    Disconnected,
    /// Session failed due to an error.
    Failed,
}

/// Metadata about an active or recent session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: SessionId,
    pub state: SessionState,
    pub peer_id: Option<String>,
    pub connected_at: Option<Instant>,
    pub last_activity: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub frames_sent: u64,
    pub reconnect_attempts: u32,
}

impl SessionInfo {
    pub fn new(id: SessionId) -> Self {
        Self {
            id,
            state: SessionState::Connecting,
            peer_id: None,
            connected_at: None,
            last_activity: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
            frames_sent: 0,
            reconnect_attempts: 0,
        }
    }

    /// Mark the session as active and record the connection time.
    pub fn activate(&mut self, peer_id: String) {
        self.state = SessionState::Active;
        self.peer_id = Some(peer_id);
        self.connected_at = Some(Instant::now());
        self.last_activity = Instant::now();
    }

    /// Record that data was sent on this session.
    pub fn record_send(&mut self, bytes: usize) {
        self.bytes_sent += bytes as u64;
        self.last_activity = Instant::now();
    }

    /// Record that data was received on this session.
    pub fn record_recv(&mut self, bytes: usize) {
        self.bytes_received += bytes as u64;
        self.last_activity = Instant::now();
    }

    /// Record that a frame was sent.
    pub fn record_frame(&mut self) {
        self.frames_sent += 1;
    }

    /// Mark the session as disconnected.
    pub fn disconnect(&mut self) {
        self.state = SessionState::Disconnected;
    }

    /// Mark the session as failed.
    pub fn fail(&mut self) {
        self.state = SessionState::Failed;
    }

    /// Increment reconnect attempts and reset state to connecting.
    pub fn attempt_reconnect(&mut self) {
        self.reconnect_attempts += 1;
        self.state = SessionState::Connecting;
        self.connected_at = None;
    }

    /// Returns whether the session is currently active.
    pub fn is_active(&self) -> bool {
        self.state == SessionState::Active
    }

    /// Returns how long ago the last activity occurred.
    pub fn time_since_activity(&self) -> Duration {
        self.last_activity.elapsed()
    }

    /// Returns whether the session appears stale (no activity for a while).
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }
}

/// Configuration for session behavior.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Maximum number of reconnect attempts before giving up.
    pub max_reconnect_attempts: u32,
    /// Duration of inactivity before considering the session stale.
    pub stale_timeout: Duration,
    /// Duration to wait between reconnect attempts.
    pub reconnect_backoff: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_reconnect_attempts: 5,
            stale_timeout: Duration::from_secs(30),
            reconnect_backoff: Duration::from_secs(2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_lifecycle() {
        let mut info = SessionInfo::new(SessionId::new());
        assert_eq!(info.state, SessionState::Connecting);
        assert!(!info.is_active());

        info.activate("peer-123".to_owned());
        assert_eq!(info.state, SessionState::Active);
        assert!(info.is_active());

        info.record_send(100);
        assert_eq!(info.bytes_sent, 100);

        info.record_frame();
        assert_eq!(info.frames_sent, 1);

        info.disconnect();
        assert_eq!(info.state, SessionState::Disconnected);
    }

    #[test]
    fn test_reconnect_flow() {
        let mut info = SessionInfo::new(SessionId::new());
        info.activate("peer-123".to_owned());
        info.fail();
        info.attempt_reconnect();
        assert_eq!(info.reconnect_attempts, 1);
        assert_eq!(info.state, SessionState::Connecting);
    }
}
