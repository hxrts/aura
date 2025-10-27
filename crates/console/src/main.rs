use leptos::prelude::*;

mod app;

use app::App;

fn main() {
    // Set panic hook for better error messages in development
    #[cfg(debug_assertions)]
    console_error_panic_hook::set_once();

    // Initialize logging
    wasm_logger::init(wasm_logger::Config::default());

    log::info!("Starting Aura Dev Console");

    // Mount the Leptos app to the root element
    mount_to_body(App);
}
