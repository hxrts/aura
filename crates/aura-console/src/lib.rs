use leptos::prelude::*;
use wasm_bindgen::prelude::*;

mod app;

use app::App;

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
