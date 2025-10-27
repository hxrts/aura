//! Unified logging infrastructure for WASM clients

use wasm_bindgen::prelude::*;

// Console bindings
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    pub fn log(s: &str);

    #[wasm_bindgen(js_namespace = console, js_name = log)]
    pub fn log_u32(a: u32);

    #[wasm_bindgen(js_namespace = console, js_name = log)]
    pub fn log_many(a: &str, b: &str);
}

/// Initialize logging system
pub fn init_logging() {
    // Could be extended with log level filtering, timestamps, etc.
}

/// Log an info message to browser console
#[macro_export]
macro_rules! console_log {
    ($($t:tt)*) => {
        $crate::logging::log(&format_args!($($t)*).to_string())
    }
}

/// Log a warning message to browser console
#[macro_export]
macro_rules! console_warn {
    ($($t:tt)*) => {
        $crate::logging::log(&format!("WARN: {}", format_args!($($t)*)))
    }
}

/// Log an error message to browser console
#[macro_export]
macro_rules! console_error {
    ($($t:tt)*) => {
        $crate::logging::log(&format!("ERROR: {}", format_args!($($t)*)))
    }
}
