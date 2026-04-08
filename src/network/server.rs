use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio::time::sleep;

use crate::core::connection::AppState;
use crate::core::protocol::{read_message, write_message, Message};
use crate::input::keyboard::execute_keyboard_event;
use crate::input::mouse::execute_mouse_event;
use crate::screen::capture::capture_primary_frame;
use crate::screen::encode::encode_frame;

pub async fn run_server(state: Arc<Mutex<AppState>>, listen_addr: String) -> io::Result<()> {
    let listener = TcpListener::bind(&listen_addr).await?;
    if let Ok(mut state) = state.lock() {
        state.status = format!("Listening on {listen_addr}");
    }

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        if let Ok(mut state) = state.lock() {
            state.status = format!("Accepted connection from {peer_addr}");
        }

        let state_for_conn = state.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(state_for_conn.clone(), stream).await {
                if let Ok(mut state) = state_for_conn.lock() {
                    state.connected = false;
                    state.peer_id = None;
                    state.outbound = None;
                    state.status = format!("Client session ended: {err}");
                }
            }
        });
    }
}

async fn handle_client(
    state: Arc<Mutex<AppState>>,
    stream: tokio::net::TcpStream,
) -> io::Result<()> {
    let (mut reader, mut writer) = stream.into_split();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Message>();

    {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        state.outbound = Some(outbound_tx.clone());
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

    let mut frame_task_started = false;

    loop {
        match read_message(&mut reader).await {
            Ok(Message::ConnectRequest { id }) => {
                if let Ok(mut state) = state.lock() {
                    state.connected = true;
                    state.peer_id = Some(id.clone());
                    state.status = format!("Connected to controller {id}");
                }

                outbound_tx.send(Message::ConnectAccept).map_err(|_| {
                    io::Error::new(io::ErrorKind::BrokenPipe, "failed to send accept")
                })?;

                if !frame_task_started {
                    frame_task_started = true;
                    spawn_frame_sender(outbound_tx.clone());
                }
            }
            Ok(Message::MouseEvent { x, y, button }) => {
                if let Err(err) = execute_mouse_event(x, y, &button) {
                    if let Ok(mut state) = state.lock() {
                        state.status = format!("Mouse input failed: {err}");
                    }
                }
            }
            Ok(Message::KeyboardEvent { key }) => {
                if let Err(err) = execute_keyboard_event(&key) {
                    if let Ok(mut state) = state.lock() {
                        state.status = format!("Keyboard input failed: {err}");
                    }
                }
            }
            Ok(Message::Disconnect) => {
                if let Ok(mut state) = state.lock() {
                    state.connected = false;
                    state.peer_id = None;
                    state.outbound = None;
                    state.status = "Controller disconnected".to_owned();
                }
                return Ok(());
            }
            Ok(Message::ConnectAccept) | Ok(Message::Frame { .. }) => {}
            Err(err) => return Err(err),
        }
    }
}

fn spawn_frame_sender(outbound_tx: mpsc::UnboundedSender<Message>) {
    tokio::spawn(async move {
        loop {
            let capture_result = tokio::task::spawn_blocking(capture_primary_frame).await;
            let frame = match capture_result {
                Ok(Ok(frame)) => frame,
                Ok(Err(_)) | Err(_) => {
                    sleep(Duration::from_millis(250)).await;
                    continue;
                }
            };

            let encoded = encode_frame(frame.width, frame.height, &frame.data);
            if outbound_tx.send(Message::Frame { data: encoded }).is_err() {
                break;
            }

            sleep(Duration::from_millis(100)).await;
        }
    });
}
