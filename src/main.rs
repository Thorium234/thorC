mod core;
mod gui;
mod input;
mod network;
mod screen;

use std::sync::{Arc, Mutex};

use core::connection::{AppState, ConnectionManager};
use eframe::NativeOptions;
use gui::app::ThorApp;
use tokio::runtime::Builder;

fn main() -> eframe::Result<()> {
    let runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|err| eframe::Error::AppCreation(Box::new(err)))?,
    );

    let state = Arc::new(Mutex::new(AppState::new()));
    let manager = Arc::new(ConnectionManager::new(
        runtime.handle().clone(),
        state.clone(),
    ));

    let native_options = NativeOptions::default();
    eframe::run_native(
        "ThorC v1",
        native_options,
        Box::new(move |_cc| Box::new(ThorApp::new(manager.clone(), state.clone()))),
    )
}
