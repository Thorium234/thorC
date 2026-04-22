use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, TextureHandle, TextureOptions, Vec2,
};

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
    notice: Option<(String, Instant)>,
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
            notice: None,
        }
    }

    fn snapshot(&self) -> AppSnapshot {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .unwrap_or_else(|_| AppState::new().snapshot())
    }

    fn refresh_texture(&mut self, ctx: &egui::Context, snapshot: &AppSnapshot) {
        if snapshot.current_frame.is_none() || snapshot.frame_version < self.last_frame_version {
            self.texture = None;
            self.last_frame_version = snapshot.frame_version;
            self.last_pointer_position = None;
            return;
        }

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

    fn apply_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(Color32::from_rgb(232, 234, 237));
        visuals.panel_fill = Color32::from_rgb(10, 15, 22);
        visuals.window_fill = Color32::from_rgb(10, 15, 22);
        visuals.faint_bg_color = Color32::from_rgb(18, 24, 34);
        visuals.extreme_bg_color = Color32::from_rgb(6, 10, 16);
        visuals.selection.bg_fill = Color32::from_rgb(58, 110, 165);
        visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(195, 224, 255));
        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(18, 24, 34);
        visuals.widgets.noninteractive.fg_stroke =
            Stroke::new(1.0, Color32::from_rgb(200, 205, 213));
        visuals.widgets.noninteractive.rounding = 12.0.into();
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(27, 35, 48);
        visuals.widgets.inactive.rounding = 10.0.into();
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(35, 47, 65);
        visuals.widgets.hovered.rounding = 10.0.into();
        visuals.widgets.active.bg_fill = Color32::from_rgb(51, 85, 129);
        visuals.widgets.active.rounding = 10.0.into();
        visuals.widgets.open.bg_fill = Color32::from_rgb(35, 47, 65);
        ctx.set_visuals(visuals);
    }

    fn card_frame() -> Frame {
        Frame::none()
            .fill(Color32::from_rgb(18, 24, 34))
            .stroke(Stroke::new(1.0, Color32::from_rgb(38, 50, 67)))
            .inner_margin(Margin::same(16.0))
            .rounding(12.0)
    }

    fn set_notice(&mut self, message: impl Into<String>) {
        self.notice = Some((message.into(), Instant::now()));
    }

    fn current_notice(&self) -> Option<&str> {
        self.notice.as_ref().and_then(|(message, at)| {
            (at.elapsed() < Duration::from_secs(3)).then_some(message.as_str())
        })
    }

    fn is_address_ready(value: &str) -> bool {
        let trimmed = value.trim();
        let Some((host, port)) = trimmed.rsplit_once(':') else {
            return false;
        };

        !host.trim().is_empty() && port.parse::<u16>().is_ok()
    }

    fn persist_addresses(&self) {
        self.manager
            .update_addresses(self.listen_addr.clone(), self.connect_addr.clone());
    }

    fn status_tone(snapshot: &AppSnapshot) -> (Color32, &'static str) {
        if snapshot.connected {
            (Color32::from_rgb(74, 222, 128), "Live session")
        } else if snapshot.connecting {
            (Color32::from_rgb(96, 165, 250), "Connecting")
        } else if snapshot.server_running {
            (Color32::from_rgb(250, 204, 21), "Waiting for peer")
        } else {
            (Color32::from_rgb(248, 113, 113), "Offline")
        }
    }

    fn status_error(snapshot: &AppSnapshot) -> Option<&str> {
        let status = snapshot.status.as_str();
        let lowered = status.to_ascii_lowercase();
        if lowered.contains("failed")
            || lowered.contains("error")
            || lowered.contains("lost")
            || lowered.contains("dropped")
        {
            Some(status)
        } else {
            None
        }
    }

    fn draw_title_bar(&self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let (tone, label) = Self::status_tone(snapshot);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("ThorC")
                        .size(28.0)
                        .color(Color32::from_rgb(242, 245, 247))
                        .strong(),
                );
                ui.label(
                    RichText::new("Minimal remote desktop control")
                        .size(13.0)
                        .color(Color32::from_rgb(150, 162, 178)),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                Frame::none()
                    .fill(tone.linear_multiply(0.14))
                    .stroke(Stroke::new(1.0, tone.linear_multiply(0.6)))
                    .inner_margin(Margin::symmetric(12.0, 8.0))
                    .rounding(999.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(tone, "●");
                            ui.label(
                                RichText::new(label)
                                    .color(Color32::from_rgb(232, 234, 237))
                                    .strong(),
                            );
                        });
                    });
            });
        });
    }

    fn draw_notice_bar(&self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let message = self.current_notice().unwrap_or(snapshot.status.as_str());
        let (accent, _) = Self::status_tone(snapshot);
        Frame::none()
            .fill(accent.linear_multiply(0.12))
            .stroke(Stroke::new(1.0, accent.linear_multiply(0.55)))
            .inner_margin(Margin::symmetric(14.0, 10.0))
            .rounding(12.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(accent, "●");
                    ui.label(
                        RichText::new(message)
                            .size(13.0)
                            .color(Color32::from_rgb(229, 233, 238)),
                    );
                });
            });
    }

    fn draw_error_banner(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let Some(message) = Self::status_error(snapshot) else {
            return;
        };

        Frame::none()
            .fill(Color32::from_rgb(58, 22, 24))
            .stroke(Stroke::new(1.0, Color32::from_rgb(190, 72, 76)))
            .inner_margin(Margin::symmetric(14.0, 12.0))
            .rounding(12.0)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Connection issue")
                            .strong()
                            .color(Color32::from_rgb(255, 214, 214)),
                    );
                    ui.label(
                        RichText::new(message)
                            .size(13.0)
                            .color(Color32::from_rgb(245, 208, 208)),
                    );
                });
            });
    }

    fn draw_control_panel(&mut self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        ui.vertical(|ui| {
            Self::card_frame().show(ui, |ui| {
                ui.label(
                    RichText::new("Session")
                        .size(13.0)
                        .color(Color32::from_rgb(143, 155, 171)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(snapshot.status.as_str())
                        .size(14.0)
                        .color(Color32::from_rgb(157, 168, 182)),
                );
                if let Some(peer_id) = snapshot.peer_id.as_deref() {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("Peer: {peer_id}"))
                            .size(13.0)
                            .color(Color32::from_rgb(210, 215, 222)),
                    );
                }
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Mode: {}", snapshot.connection_label))
                        .size(13.0)
                        .color(Color32::from_rgb(210, 215, 222)),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!(
                        "Traffic: {} sent, {} received",
                        format_bytes(snapshot.bytes_sent),
                        format_bytes(snapshot.bytes_received)
                    ))
                    .size(13.0)
                    .color(Color32::from_rgb(210, 215, 222)),
                );
            });

            ui.add_space(12.0);

            Self::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Host machine").size(16.0).strong());
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Listen address")
                        .size(12.0)
                        .color(Color32::from_rgb(143, 155, 171)),
                );
                ui.add_space(4.0);
                let listen_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.listen_addr)
                        .desired_width(f32::INFINITY)
                        .hint_text("0.0.0.0:9000"),
                );
                if listen_edit.changed() {
                    self.persist_addresses();
                }
                ui.add_space(10.0);
                let button = egui::Button::new(RichText::new("Start Server").strong())
                    .min_size(Vec2::new(ui.available_width(), 34.0));
                if ui
                    .add_enabled(
                        !snapshot.server_running && Self::is_address_ready(&self.listen_addr),
                        button,
                    )
                    .on_disabled_hover_text(
                        "Enter a listen address like 0.0.0.0:9000, or stop using this window for hosting",
                    )
                    .clicked()
                {
                    self.manager.start_server(self.listen_addr.clone());
                    self.set_notice(format!("Starting server on {}", self.listen_addr));
                }
            });

            ui.add_space(12.0);

            Self::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Controller machine").size(16.0).strong());
                ui.add_space(12.0);
                ui.label(
                    RichText::new("Target address")
                        .size(12.0)
                        .color(Color32::from_rgb(143, 155, 171)),
                );
                ui.add_space(4.0);
                let target_edit = ui.add(
                    egui::TextEdit::singleline(&mut self.connect_addr)
                        .desired_width(f32::INFINITY)
                        .hint_text("127.0.0.1:9000"),
                );
                if target_edit.changed() {
                    self.persist_addresses();
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let button_width = ((ui.available_width() - 8.0) / 2.0).max(96.0);
                    let connect = egui::Button::new(RichText::new("Connect").strong())
                        .min_size(Vec2::new(button_width, 34.0));
                    if ui
                        .add_enabled(
                            Self::is_address_ready(&self.connect_addr)
                                && !snapshot.connected
                                && !snapshot.connecting,
                            connect,
                        )
                        .on_disabled_hover_text(
                            "Enter a target like 127.0.0.1:9000, or wait for the current connection attempt to finish",
                        )
                        .clicked()
                    {
                        self.manager.connect(self.connect_addr.clone());
                        self.set_notice(format!("Connecting to {}", self.connect_addr));
                    }

                    let disconnect = egui::Button::new(RichText::new("Disconnect").strong())
                        .min_size(Vec2::new(button_width, 34.0));
                    if ui
                        .add_enabled(snapshot.connected, disconnect)
                        .on_disabled_hover_text("No active session to disconnect")
                        .clicked()
                    {
                        self.manager.disconnect();
                        self.set_notice("Disconnected session");
                    }
                });
            });
        });
    }

    fn draw_viewer_hint(&self, ui: &mut egui::Ui, snapshot: &AppSnapshot) {
        let hint = if snapshot.connected && snapshot.current_frame.is_none() {
            "Connected. Waiting for the first screen frame from the remote machine."
        } else if snapshot.connecting {
            "Connecting to the remote machine."
        } else if snapshot.connected {
            "Connected. Click inside the frame to send input."
        } else if snapshot.server_running {
            "Server is listening. Connect from another ThorC window or machine."
        } else {
            "No session yet. Start a server or connect to a target."
        };

        ui.label(
            RichText::new(hint)
                .size(13.0)
                .color(Color32::from_rgb(157, 168, 182)),
        );
    }

    fn draw_viewer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snapshot: &AppSnapshot) {
        Self::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Remote screen").size(18.0).strong());
                    ui.label(
                        RichText::new("Live viewer and input surface")
                            .size(12.0)
                            .color(Color32::from_rgb(143, 155, 171)),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let dims = snapshot
                        .current_frame_size
                        .map(|(w, h)| format!("{w} x {h}"))
                        .unwrap_or_else(|| "No frame".to_owned());
                    ui.label(
                        RichText::new(dims)
                            .size(12.0)
                            .color(Color32::from_rgb(143, 155, 171)),
                    );
                });
            });
            ui.add_space(6.0);
            self.draw_viewer_hint(ui, snapshot);
            ui.add_space(14.0);

            let available = ui.available_size();
            let desired = Vec2::new(available.x.max(320.0), available.y.max(280.0));

            Frame::none()
                .fill(Color32::from_rgb(5, 8, 14))
                .stroke(Stroke::new(1.0, Color32::from_rgb(35, 47, 65)))
                .rounding(14.0)
                .inner_margin(Margin::same(12.0))
                .show(ui, |ui| {
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
                        self.forward_remote_input(ctx, &response, snapshot);
                    } else {
                        let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
                        let painter = ui.painter_at(rect);
                        painter.rect_filled(rect, 14.0, Color32::from_rgb(8, 12, 18));
                        painter.text(
                            rect.center_top() + Vec2::new(0.0, 92.0),
                            Align2::CENTER_CENTER,
                            "No remote frame yet",
                            FontId::proportional(24.0),
                            Color32::from_rgb(224, 228, 232),
                        );
                        painter.text(
                            rect.center_top() + Vec2::new(0.0, 128.0),
                            Align2::CENTER_CENTER,
                            "Start a server, connect a peer, then click inside this viewer to control it.",
                            FontId::proportional(14.0),
                            Color32::from_rgb(132, 144, 160),
                        );
                    }
                });
        });
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
        let max_x = frame_size.0.saturating_sub(1) as f32;
        let max_y = frame_size.1.saturating_sub(1) as f32;
        let x = (x_ratio * max_x).round() as i32;
        let y = (y_ratio * max_y).round() as i32;
        Some((x, y))
    }

    fn normalize_scroll_delta(delta: f32) -> i32 {
        if delta == 0.0 {
            return 0;
        }

        let steps = (delta / 40.0).round() as i32;
        if steps == 0 {
            delta.signum() as i32
        } else {
            steps
        }
    }

    fn forward_remote_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        snapshot: &AppSnapshot,
    ) {
        let frame_size = match snapshot.current_frame_size {
            Some(size) => size,
            None => {
                self.last_pointer_position = None;
                return;
            }
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

                let scroll_delta = ctx.input(|input| input.raw_scroll_delta);
                let delta_x = Self::normalize_scroll_delta(scroll_delta.x);
                let delta_y = Self::normalize_scroll_delta(scroll_delta.y);
                if response.hovered() && (delta_x != 0 || delta_y != 0) {
                    self.manager.send_message(Message::MouseScroll {
                        x,
                        y,
                        delta_x,
                        delta_y,
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

fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;

    if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}

impl eframe::App for ThorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let snapshot = self.snapshot();
        self.refresh_texture(ctx, &snapshot);
        Self::apply_theme(ctx);

        if self
            .notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(3))
        {
            self.notice = None;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add_space(8.0);
                    self.draw_title_bar(ui, &snapshot);
                    ui.add_space(12.0);
                    self.draw_notice_bar(ui, &snapshot);
                    if Self::status_error(&snapshot).is_some() {
                        ui.add_space(10.0);
                        self.draw_error_banner(ui, &snapshot);
                    }
                    ui.add_space(16.0);

                    self.draw_control_panel(ui, &snapshot);

                    ui.add_space(16.0);
                    self.draw_viewer(ui, ctx, &snapshot);
                });
        });

        ctx.request_repaint_after(Duration::from_millis(16));
    }
}
