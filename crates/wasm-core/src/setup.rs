//! WASM initialization and setup utilities

use wasm_bindgen::prelude::*;

// WASM memory allocator
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Initialize WASM environment with panic hooks and logging
#[wasm_bindgen(start)]
pub fn initialize_wasm() {
    console_error_panic_hook::set_once();
    crate::logging::init_logging();
}

/// Manual initialization for clients that don't use wasm_bindgen(start)
pub fn init_manually() {
    console_error_panic_hook::set_once();
    crate::logging::init_logging();
}
