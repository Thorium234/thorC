use std::sync::{Arc, Mutex};

use eframe::egui::{self, ColorImage, Sense, TextureHandle, TextureOptions, Vec2};

use crate::core::connection::{AppSnapshot, AppState, ConnectionManager};
use crate::core::protocol::Message;

pub struct ThorApp {
    manager: Arc<ConnectionManager>,
    state: Arc<Mutex<AppState>>,
    connect_addr: String,
    listen_addr: String,
    texture: Option<TextureHandle>,
    last_frame_version: u64,
    last_pointer_position: Option<(i32, i32)>,
}

impl ThorApp {
    pub fn new(manager: Arc<ConnectionManager>, state: Arc<Mutex<AppState>>) -> Self {
        let snapshot = state
            .lock()
            .map(|guard| guard.snapshot())
            .unwrap_or_else(|_| AppState::new().snapshot());

        Self {
            manager,
            state,
            connect_addr: snapshot.target_addr,
            listen_addr: snapshot.listen_addr,
            texture: None,
            last_frame_version: 0,
            last_pointer_position: None,
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .unwrap_or_else(|_| AppState::new().snapshot())
    }

    fn refresh_texture(&mut self, ctx: &egui::Context, snapshot: &AppSnapshot) {
        if snapshot.frame_version == self.last_frame_version {
            return;
        }

        let (frame, (width, height)) =
            match (snapshot.current_frame.as_ref(), snapshot.current_frame_size) {
                (Some(frame), Some(size)) => (frame, size),
                _ => return,
            };

        let image = ColorImage::from_rgba_unmultiplied([width, height], frame);
        if let Some(texture) = &mut self.texture {
            texture.set(image, TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture("remote-screen", image, TextureOptions::LINEAR));
        }

        self.last_frame_version = snapshot.frame_version;
    }

    fn map_pointer_to_remote(
        rect: egui::Rect,
        position: egui::Pos2,
        frame_size: (usize, usize),
    ) -> Option<(i32, i32)> {
        if !rect.contains(position) {
            return None;
        }

        let x_ratio = ((position.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
        let y_ratio = ((position.y - rect.min.y) / rect.height()).clamp(0.0, 1.0);
        let x = (x_ratio * frame_size.0 as f32) as i32;
        let y = (y_ratio * frame_size.1 as f32) as i32;
        Some((x, y))
    }

    fn forward_remote_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        snapshot: &AppSnapshot,
    ) {
        let frame_size = match snapshot.current_frame_size {
            Some(size) => size,
            None => return,
        };

        if response.clicked() {
            response.request_focus();
        }

        if let Some(pointer_pos) = response.interact_pointer_pos() {
            if let Some((x, y)) =
                Self::map_pointer_to_remote(response.rect, pointer_pos, frame_size)
            {
                if self.last_pointer_position != Some((x, y)) {
                    self.manager.send_message(Message::MouseEvent {
                        x,
                        y,
                        button: "move".to_owned(),
                    });
                    self.last_pointer_position = Some((x, y));
                }

                if response.clicked_by(egui::PointerButton::Primary) {
                    self.manager.send_message(Message::MouseEvent {
                        x,
                        y,
                        button: "left".to_owned(),
                    });
                }

                if response.clicked_by(egui::PointerButton::Secondary) {
                    self.manager.send_message(Message::MouseEvent {
                        x,
                        y,
                        button: "right".to_owned(),
                    });
                }
            }
        }

        if response.has_focus() {
            let events = ctx.input(|input| input.events.clone());
            for event in events {
                match event {
                    egui::Event::Text(text) => {
                        for character in text.chars() {
                            self.manager.send_message(Message::KeyboardEvent {
                                key: character.to_string(),
                            });
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers: _,
                        repeat: _,
                        ..
                    } => {
                        if let Some(mapped) = map_key(key) {
                            self.manager.send_message(Message::KeyboardEvent {
                                key: mapped.to_owned(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn map_key(key: egui::Key) -> Option<&'static str> {
    match key {
        egui::Key::Enter => Some("enter"),
        egui::Key::Tab => Some("tab"),
        egui::Key::Backspace => Some("backspace"),
        egui::Key::Escape => Some("escape"),
        egui::Key::Space => Some("space"),
        _ => None,
    }
}

impl eframe::App for ThorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snapshot = self.snapshot();
        self.refresh_texture(ctx, &snapshot);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("ThorC v1");
            ui.label(format!("Your ID: {}", snapshot.local_id));
            ui.label(format!("Status: {}", snapshot.status));

            ui.horizontal(|ui| {
                ui.label("Listen:");
                ui.text_edit_singleline(&mut self.listen_addr);
                if ui.button("Start Server").clicked() {
                    self.manager.start_server(self.listen_addr.clone());
                }
            });

            ui.horizontal(|ui| {
                ui.label("Target:");
                ui.text_edit_singleline(&mut self.connect_addr);
                if ui.button("Connect").clicked() {
                    self.manager.connect(self.connect_addr.clone());
                }
                if ui.button("Disconnect").clicked() {
                    self.manager.disconnect();
                }
            });

            ui.label(format!(
                "Connected: {}",
                if snapshot.connected { "Yes" } else { "No" }
            ));
            ui.label(format!(
                "Peer ID: {}",
                snapshot.peer_id.as_deref().unwrap_or("N/A")
            ));
            ui.label(format!(
                "Server: {}",
                if snapshot.server_running {
                    "Running"
                } else {
                    "Stopped"
                }
            ));

            ui.separator();

            let available = ui.available_size();
            let desired = Vec2::new(available.x.max(320.0), available.y.max(240.0));

            if let Some(texture) = &self.texture {
                let mut display_size = desired;
                if let Some((width, height)) = snapshot.current_frame_size {
                    let image_size = Vec2::new(width as f32, height as f32);
                    let scale = (desired.x / image_size.x)
                        .min(desired.y / image_size.y)
                        .max(0.1);
                    display_size = image_size * scale;
                }

                let image = egui::Image::new(texture)
                    .fit_to_exact_size(display_size)
                    .sense(Sense::click_and_drag());
                let response = ui.add(image);
                self.forward_remote_input(ctx, &response, &snapshot);
            } else {
                ui.allocate_ui(desired, |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label("Remote screen will appear here");
                    });
                });
            }
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
