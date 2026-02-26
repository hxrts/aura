//! # WASM Platform Helpers
//!
//! Helpers for web integration via wasm-bindgen.

use cfg_if::cfg_if;

/// Get the IndexedDB database name
pub fn database_name() -> String {
    "aura".to_string()
}

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use wasm_bindgen::prelude::*;

        /// Initialize the WASM platform
        ///
        /// Call this before creating AppCore.
        pub fn initialize() {
            // Set up panic hook for better error messages.
            console_error_panic_hook::set_once();
        }

        /// Log a message to the browser console.
        #[wasm_bindgen]
        pub fn console_log(message: &str) {
            web_sys::console::log_1(&message.into());
        }

        /// Check if running in a web worker.
        pub fn is_web_worker() -> bool {
            js_sys::global()
                .dyn_into::<web_sys::WorkerGlobalScope>()
                .is_ok()
        }
    } else {
        /// No-op outside wasm targets.
        pub fn initialize() {}

        /// Non-wasm console logging fallback.
        pub fn console_log(message: &str) {
            eprintln!("{message}");
        }

        /// Non-wasm targets are never web workers.
        pub fn is_web_worker() -> bool {
            false
        }
    }
}
