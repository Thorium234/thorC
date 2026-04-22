use std::io;
use std::mem;
use std::sync::{Arc, Mutex};

use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::core::connection::AppState;
use crate::core::protocol::{read_message, write_message, Message, MAX_FRAME_SIZE};
use crate::screen::decode::decode_frame;

struct PendingFrame {
    total_len: usize,
    data: Vec<u8>,
}

fn apply_decoded_frame(
    state: &Arc<Mutex<AppState>>,
    session_nonce: u64,
    target_addr: &str,
    data: &[u8],
) {
    match decode_frame(data) {
        Ok(frame) => {
            if let Ok(mut state) = state.lock() {
                if state.is_current_session(session_nonce) {
                    state.current_frame = Some(frame.rgba);
                    state.current_frame_size = Some((frame.width, frame.height));
                    state.frame_version = state.frame_version.wrapping_add(1);
                    state.record_bytes_received(data.len());
                    let peer = state.peer_id.clone().unwrap_or_else(|| target_addr.to_owned());
                    state.activate_session(peer);
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
    }
}

fn apply_delta_frame(
    state: &Arc<Mutex<AppState>>,
    session_nonce: u64,
    target_addr: &str,
    width: u32,
    height: u32,
    regions: &[crate::core::protocol::DeltaRegion],
) {
    if let Ok(mut state) = state.lock() {
        if !state.is_current_session(session_nonce) {
            return;
        }

        // If we don't have a base frame yet, we need a full frame first
        let Some(mut current) = state.current_frame.clone() else {
            state.status = "Waiting for base frame before applying delta".to_owned();
            return;
        };

        let current_width = state.current_frame_size.map(|(w, _)| w).unwrap_or(0);
        let current_height = state.current_frame_size.map(|(_, h)| h).unwrap_or(0);

        // Resize if dimensions changed
        if current_width != width as usize || current_height != height as usize {
            current = vec![0u8; (width * height * 4) as usize];
        }

        // Apply each delta region
        let row_stride = width as usize * 4;
        for region in regions {
            let rx = region.x as usize;
            let ry = region.y as usize;
            let rw = region.width as usize;
            let rh = region.height as usize;

            for row in 0..rh {
                let y = ry + row;
                if y >= height as usize {
                    break;
                }
                let src_start = row * rw * 4;
                let src_end = src_start + rw * 4;
                let dst_start = y * row_stride + rx * 4;
                let dst_end = dst_start + rw * 4;

                if dst_end <= current.len() && src_end <= region.data.len() {
                    current[dst_start..dst_end]
                        .copy_from_slice(&region.data[src_start..src_end]);
                }
            }
        }

        let peer = state.peer_id.clone().unwrap_or_else(|| target_addr.to_owned());
        state.current_frame = Some(current);
        state.current_frame_size = Some((width as usize, height as usize));
        state.frame_version = state.frame_version.wrapping_add(1);
        let delta_bytes = regions.iter().map(|region| region.data.len()).sum::<usize>();
        state.record_bytes_received(delta_bytes);
        state.activate_session(peer);
        state.status = format!("Streaming from {target_addr}");
    }
}

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

    // v2: Send session ID if we have one
    let session_id = {
        let state = state
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "state lock poisoned"))?;
        state.session.as_ref().map(|s| s.id.as_str().to_owned())
    };

    outbound_tx
        .send(Message::ConnectRequest {
            id: local_id,
            session_id,
        })
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "failed to send handshake"))?;
    let mut pending_frame: Option<PendingFrame> = None;

    loop {
        match read_message(&mut reader).await {
            Ok(Message::ConnectAccept { session_id }) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.connected = true;
                        state.status =
                            format!("Connected to {target_addr} (session {session_id})");
                    }
                }
            }
            Ok(Message::ConnectReject { reason }) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.mark_session_failed();
                        state.status = format!("Connection rejected: {reason}");
                    }
                }
                return Err(io::Error::new(io::ErrorKind::ConnectionRefused, reason));
            }
            Ok(Message::Frame { data }) => {
                pending_frame = None;
                apply_decoded_frame(&state, session_nonce, &target_addr, &data);
            }
            Ok(Message::DeltaFrame {
                width,
                height,
                regions,
            }) => {
                apply_delta_frame(&state, session_nonce, &target_addr, width, height, &regions);
            }
            Ok(Message::FrameStart { total_len }) => {
                let total_len = total_len as usize;
                if total_len > MAX_FRAME_SIZE {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.clear_connection_state();
                            state.mark_session_failed();
                        }
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "received oversized frame stream",
                    ));
                }

                pending_frame = Some(PendingFrame {
                    total_len,
                    data: Vec::with_capacity(total_len),
                });
            }
            Ok(Message::FrameChunk { data }) => {
                let Some(frame) = pending_frame.as_mut() else {
                    continue;
                };

                if frame.data.len() + data.len() > frame.total_len {
                    if let Ok(mut state) = state.lock() {
                        if state.is_current_session(session_nonce) {
                            state.clear_connection_state();
                            state.mark_session_failed();
                        }
                    }
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "frame chunk exceeded declared size",
                    ));
                }

                frame.data.extend_from_slice(&data);

                if frame.data.len() == frame.total_len {
                    let complete_frame = mem::take(&mut pending_frame)
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidData, "missing pending frame")
                        })?;
                    apply_decoded_frame(&state, session_nonce, &target_addr, &complete_frame.data);
                }
            }
            Ok(Message::Disconnect) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.status = "Remote peer disconnected".to_owned();
                    }
                }
                return Ok(());
            }
            Ok(Message::Heartbeat) => {
                // v2: Respond to heartbeat to keep connection alive
                if let Ok(state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        if let Some(sender) = state.outbound.clone() {
                            let _ = sender.send(Message::Heartbeat);
                        }
                    }
                }
            }
            Ok(Message::ConnectRequest { .. })
            | Ok(Message::MouseEvent { .. })
            | Ok(Message::MouseScroll { .. })
            | Ok(Message::KeyboardEvent { .. })
            | Ok(Message::ReconnectRequest { .. }) => {}
            Err(err) => {
                if let Ok(mut state) = state.lock() {
                    if state.is_current_session(session_nonce) {
                        state.clear_connection_state();
                        state.mark_session_failed();
                        state.status = format!("Connection lost: {err}");
                    }
                }
                return Err(err);
            }
        }
    }
}
