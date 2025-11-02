//! Aura development console - A web-based interface for monitoring and debugging Aura systems.
//!
//! This console provides real-time visibility into protocol execution, state changes,
//! and system metrics for development and testing purposes.

use leptos::prelude::*;
use wasm_bindgen::prelude::*;

mod app;

use app::App;

/// Initializes and mounts the Aura development console to the DOM.
///
/// This function sets up the WASM runtime, initializes logging, and mounts
/// the Leptos application to the page body.
#[wasm_bindgen(start)]
pub fn start() {
    // Initialize wasm_core first
    wasm_core::init_manually();

    // Set up console-specific logging
    wasm_logger::init(wasm_logger::Config::default());

    log::info!("Aura Dev Console initializing...");

    // Mount the Leptos app
    mount_to_body(App);

    log::info!("Aura Dev Console mounted successfully");
}
