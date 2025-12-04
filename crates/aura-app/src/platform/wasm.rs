//! # WASM Platform Helpers
//!
//! Helpers for web integration via wasm-bindgen.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Initialize the WASM platform
///
/// Call this before creating AppCore
#[cfg(target_arch = "wasm32")]
pub fn initialize() {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();

    // Initialize logging to browser console
    // console_log::init_with_level(log::Level::Debug).ok();
}

/// Log a message to the browser console
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn console_log(message: &str) {
    web_sys::console::log_1(&message.into());
}

/// Get the IndexedDB database name
pub fn database_name() -> String {
    "aura".to_string()
}

/// Check if running in a web worker
#[cfg(target_arch = "wasm32")]
pub fn is_web_worker() -> bool {
    js_sys::global()
        .dyn_into::<web_sys::WorkerGlobalScope>()
        .is_ok()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn is_web_worker() -> bool {
    false
}

/// Non-WASM fallback implementations
#[cfg(not(target_arch = "wasm32"))]
pub fn initialize() {
    // No-op on non-WASM
}
