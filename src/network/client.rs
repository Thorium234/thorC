use std::io;
use std::sync::{Arc, Mutex};

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::core::connection::AppState;
use crate::core::protocol::{read_message, write_message, Message};
use crate::screen::decode::decode_frame;

pub async fn connect_to_peer(
    state: Arc<Mutex<AppState>>,
    target_addr: String,
    session_nonce: u64,
) -> io::Result<()> {
    let stream = TcpStream::connect(&target_addr).await?;
    let (mut reader, mut writer) = stream.into_split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();

    {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        if !state.is_current_session(session_nonce) {
            return Ok(());
        }
        state.outbound = Some(outbound_tx.clone());
        state.status = format!("Connected transport to {target_addr}, waiting for accept");
    }

    tokio::spawn(async move {
        while let Some(message) = outbound_rx.recv().await {
            if write_message(&mut writer, &message).await.is_err() {
                break;
            }
            if matches!(message, Message::Disconnect) {
                break;
            }
        }
    });

    let local_id = {
        let state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        state.local_id.clone()
    };
    outbound_tx
        .send(Message::ConnectRequest { id: local_id })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "failed to send handshake"))?;

    loop {
        match read_message(&mut reader).await {
            Ok(Message::ConnectAccept) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.connected = true;
                        state.status = format!("Connected to {target_addr}");
                    }
                }
            }
            Ok(Message::ConnectReject { reason }) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.status = format!("Connection rejected: {reason}");
                    }
                }
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, reason));
            }
            Ok(Message::Frame { data }) => match decode_frame(&data) {
                Ok(frame) => {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.current_frame = Some(frame.rgba);
                            state.current_frame_size = Some((frame.width, frame.height));
                            state.frame_version = state.frame_version.wrapping_add(1);
                            state.connected = true;
                            state.status = format!("Streaming from {target_addr}");
                        }
                    }
                }
                Err(err) => {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.status = format!("Failed to decode frame: {err}");
                        }
                    }
                }
            },
            Ok(Message::Disconnect) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.status = "Remote peer disconnected".to_owned();
                    }
                }
                return Ok(());
            }
            Ok(Message::ConnectRequest { .. })
            | Ok(Message::MouseEvent { .. })
            | Ok(Message::KeyboardEvent { .. }) => {}
            Err(err) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.status = format!("Connection lost: {err}");
                    }
                }
                return Err(err);
            }
        }
    }
}
