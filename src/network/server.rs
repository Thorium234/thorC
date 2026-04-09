use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::sync::mpsc;

use crate::core::connection::AppState;
use crate::core::protocol::{
    read_message, write_message, Message, FRAME_CHUNK_SIZE, MAX_FRAME_SIZE,
};
use crate::input::keyboard::execute_keyboard_event;
use crate::input::mouse::{execute_mouse_event, execute_mouse_scroll};
use crate::screen::capture::PrimaryCapturer;
use crate::screen::delta::DeltaEncoder;
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
                    state.clear_connection_state();
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
    let reject_busy = {
        let state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        state.outbound.is_some() || state.connected
    };

    if reject_busy {
        write_message(
            &mut writer,
            &Message::ConnectReject {
                reason: "another remote-control session is already active".to_owned(),
            },
        )
        .await?;
        return Ok(());
    }

    let session_nonce = {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        let session_nonce = state.begin_session();
        state.clear_connection_state();
        state.outbound = Some(outbound_tx.clone());
        session_nonce
    };

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
            Ok(Message::ConnectRequest { id, session_id }) => {
                // v2: Generate a session ID if the client didn't provide one
                let assigned_session_id = session_id.unwrap_or_else(|| {
                    let state = state.lock().ok();
                    state
                        .as_ref()
                        .and_then(|s| s.session.as_ref())
                        .map(|s| s.id.as_str().to_owned())
                        .unwrap_or_else(|| "unknown".to_owned())
                });

                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.activate_session(id.clone());
                        state.status = format!("Connected to controller {id}");
                    }
                }

                outbound_tx
                    .send(Message::ConnectAccept {
                        session_id: assigned_session_id,
                    })
                    .map_err(|_| {
                        io::Error::new(io::ErrorKind::BrokenPipe, "failed to send accept")
                    })?;

                if !frame_task_started {
                    frame_task_started = true;
                    spawn_frame_sender(state.clone(), outbound_tx.clone(), session_nonce);
                }
            }
            Ok(Message::MouseEvent { x, y, button }) => {
                if let Err(err) = execute_mouse_event(x, y, &button) {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.status = format!("Mouse input failed: {err}");
                        }
                    }
                }
            }
            Ok(Message::MouseScroll {
                x,
                y,
                delta_x,
                delta_y,
            }) => {
                if let Err(err) = execute_mouse_scroll(x, y, delta_x, delta_y) {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.status = format!("Mouse scroll failed: {err}");
                        }
                    }
                }
            }
            Ok(Message::KeyboardEvent { key }) => {
                if let Err(err) = execute_keyboard_event(&key) {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.status = format!("Keyboard input failed: {err}");
                        }
                    }
                }
            }
            Ok(Message::Heartbeat) => {
                // v2: Respond to heartbeat
                if let Ok(state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        let _ = outbound_tx.send(Message::Heartbeat);
                    }
                }
            }
            Ok(Message::ReconnectRequest) => {
                // v2: Client is asking for a fresh frame
                if !frame_task_started {
                    frame_task_started = true;
                    spawn_frame_sender(state.clone(), outbound_tx.clone(), session_nonce);
                }
            }
            Ok(Message::Disconnect) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.begin_session();
                        state.clear_connection_state();
                        state.status = "Controller disconnected".to_owned();
                    }
                }
                return Ok(());
            }
            Ok(Message::ConnectAccept { .. })
            | Ok(Message::ConnectReject { .. })
            | Ok(Message::FrameStart { .. })
            | Ok(Message::FrameChunk { .. })
            | Ok(Message::Frame { .. })
            | Ok(Message::DeltaFrame { .. }) => {}
            Err(err) => return Err(err),
        }
    }
}

fn spawn_frame_sender(
    state: Arc<Mutex<AppState>>,
    outbound_tx: mpsc::UnboundedSender<Message>,
    session_nonce: u64,
) {
    tokio::task::spawn_blocking(move || {
        let mut capturer = match PrimaryCapturer::new() {
            Ok(capturer) => capturer,
            Err(err) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.status = format!("Screen capture init failed: {err}");
                    }
                }
                return;
            }
        };

        // v2: Initialize delta encoder
        let mut delta_encoder = DeltaEncoder::new(Default::default());
        let use_delta = state
            .lock()
            .map(|s| s.delta_encoding)
            .unwrap_or(true);

        // v2: Calculate frame interval from target FPS
        let target_fps = state
            .lock()
            .map(|s| s.target_fps.max(1).min(60))
            .unwrap_or(15);
        let frame_interval = Duration::from_millis(1000 / target_fps as u64);

        loop {
            let is_current = state
                .lock()
                .map(|state| state.is_current_session(session_nonce))
                .unwrap_or(false);
            if !is_current {
                break;
            }

            let frame = match capturer.capture_frame() {
                Ok(frame) => frame,
                Err(err) => {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.status = format!("Screen capture failed: {err}");
                        }
                    }
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }
            };

            // v2: Try delta encoding if enabled
            if use_delta {
                if let Some(regions) =
                    delta_encoder.compute_delta(&frame.data, frame.width, frame.height)
                {
                    // Delta detected: send regions
                    if regions.is_empty() {
                        // No changes, skip this frame to save bandwidth
                        std::thread::sleep(frame_interval);
                        continue;
                    }

                    let delta_msg = Message::DeltaFrame {
                        width: frame.width as u32,
                        height: frame.height as u32,
                        regions: regions
                            .into_iter()
                            .map(|r| crate::core::protocol::DeltaRegion {
                                x: r.x,
                                y: r.y,
                                width: r.width,
                                height: r.height,
                                data: r.data,
                            })
                            .collect(),
                    };

                    if outbound_tx.send(delta_msg).is_err() {
                        if let Ok(mut state) = state.lock() {
                            if state.is_current_session(session_nonce) {
                                state.status =
                                    "Screen stream stopped: client channel closed".to_owned();
                                state.clear_connection_state();
                            }
                        }
                        break;
                    }

                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.record_bytes_sent(
                                frame.width * frame.height * 4, // approximate
                            );
                        }
                    }

                    std::thread::sleep(frame_interval);
                    continue;
                }
            }

            // Fallback: full frame
            let encoded = encode_frame(frame.width, frame.height, &frame.data);
            if encoded.len() > MAX_FRAME_SIZE {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.status =
                            format!("Screen frame too large to stream: {} bytes", encoded.len());
                    }
                }
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }

            let send_result = if encoded.len() <= FRAME_CHUNK_SIZE {
                let data_len = encoded.len();
                if outbound_tx.send(Message::Frame { data: encoded }).is_err() {
                    break;
                }
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.record_bytes_sent(data_len);
                        state.record_frame();
                    }
                }
                true
            } else {
                let data_len = encoded.len();
                let start_result = outbound_tx.send(Message::FrameStart {
                    total_len: encoded.len() as u32,
                });

                if start_result.is_err() {
                    break;
                }

                let mut failed = false;
                for chunk in encoded.chunks(FRAME_CHUNK_SIZE) {
                    if outbound_tx
                        .send(Message::FrameChunk {
                            data: chunk.to_vec(),
                        })
                        .is_err()
                    {
                        failed = true;
                        break;
                    }
                }

                if failed {
                    false
                } else {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.record_bytes_sent(data_len);
                            state.record_frame();
                        }
                    }
                    true
                }
            };

            if !send_result {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.status = "Screen stream stopped: client channel closed".to_owned();
                        state.clear_connection_state();
                    }
                }
                break;
            }

            std::thread::sleep(frame_interval);
        }
    });
}
