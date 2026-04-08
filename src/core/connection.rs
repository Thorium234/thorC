use std::sync::{Arc, Mutex};

use tokio::runtime::Handle;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::core::protocol::Message;
use crate::core::settings::{load_settings, save_settings, AppSettings};
use crate::network::{client, server};

pub struct AppState {
    pub connected: bool,
    pub peer_id: Option<String>,
    pub current_frame: Option<Vec<u8>>,
    pub current_frame_size: Option<(usize, usize)>,
    pub frame_version: u64,
    pub local_id: String,
    pub listen_addr: String,
    pub target_addr: String,
    pub status: String,
    pub server_running: bool,
    pub outbound: Option<mpsc::UnboundedSender<Message>>,
}

#[derive(Clone)]
pub struct AppSnapshot {
    pub connected: bool,
    pub peer_id: Option<String>,
    pub current_frame: Option<Vec<u8>>,
    pub current_frame_size: Option<(usize, usize)>,
    pub frame_version: u64,
    pub local_id: String,
    pub listen_addr: String,
    pub target_addr: String,
    pub status: String,
    pub server_running: bool,
}

impl AppState {
    pub fn new() -> Self {
        let settings = load_settings();
        Self {
            connected: false,
            peer_id: None,
            current_frame: None,
            current_frame_size: None,
            frame_version: 0,
            local_id: Uuid::new_v4().to_string(),
            listen_addr: settings.listen_addr,
            target_addr: settings.target_addr,
            status: "Idle".to_owned(),
            server_running: false,
            outbound: None,
        }
    }

    pub fn snapshot(&self) -> AppSnapshot {
        AppSnapshot {
            connected: self.connected,
            peer_id: self.peer_id.clone(),
            current_frame: self.current_frame.clone(),
            current_frame_size: self.current_frame_size,
            frame_version: self.frame_version,
            local_id: self.local_id.clone(),
            listen_addr: self.listen_addr.clone(),
            target_addr: self.target_addr.clone(),
            status: self.status.clone(),
            server_running: self.server_running,
        }
    }

    pub fn settings(&self) -> AppSettings {
        AppSettings {
            listen_addr: self.listen_addr.clone(),
            target_addr: self.target_addr.clone(),
        }
    }
}

pub struct ConnectionManager {
    runtime: Handle,
    state: Arc<Mutex<AppState>>,
}

impl ConnectionManager {
    pub fn new(runtime: Handle, state: Arc<Mutex<AppState>>) -> Self {
        Self { runtime, state }
    }

    pub fn start_server(&self, listen_addr: String) {
        {
            if let Ok(mut state) = self.state.lock() {
                state.listen_addr = listen_addr.clone();
                let _ = save_settings(&state.settings());
                if state.server_running {
                    state.status = format!("Server already listening on {}", state.listen_addr);
                    return;
                }
                state.server_running = true;
                state.status = format!("Starting server on {listen_addr}");
            }
        }

        let state = self.state.clone();
        self.runtime.spawn(async move {
            if let Err(err) = server::run_server(state.clone(), listen_addr.clone()).await {
                if let Ok(mut state) = state.lock() {
                    state.server_running = false;
                    state.status = format!("Server error on {listen_addr}: {err}");
                }
            }
        });
    }

    pub fn connect(&self, target_addr: String) {
        {
            if let Ok(mut state) = self.state.lock() {
                state.target_addr = target_addr.clone();
                let _ = save_settings(&state.settings());
                state.status = format!("Connecting to {target_addr}");
            }
        }

        let state = self.state.clone();
        self.runtime.spawn(async move {
            if let Err(err) = client::connect_to_peer(state.clone(), target_addr.clone()).await {
                if let Ok(mut state) = state.lock() {
                    state.connected = false;
                    state.outbound = None;
                    state.peer_id = None;
                    state.status = format!("Connection to {target_addr} failed: {err}");
                }
            }
        });
    }

    pub fn send_message(&self, message: Message) {
        let outbound = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.outbound.clone());

        if let Some(sender) = outbound {
            if sender.send(message).is_err() {
                if let Ok(mut state) = self.state.lock() {
                    state.connected = false;
                    state.outbound = None;
                    state.status = "Connection dropped while sending".to_owned();
                }
            }
        }
    }

    pub fn disconnect(&self) {
        self.send_message(Message::Disconnect);
        if let Ok(mut state) = self.state.lock() {
            state.connected = false;
            state.peer_id = None;
            state.outbound = None;
            state.status = "Disconnected".to_owned();
        }
    }

    pub fn update_addresses(&self, listen_addr: String, target_addr: String) {
        if let Ok(mut state) = self.state.lock() {
            state.listen_addr = listen_addr;
            state.target_addr = target_addr;
            let _ = save_settings(&state.settings());
        }
    }
}
