//! WASM initialization and setup utilities

// WASM memory allocator
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// Initialize WASM environment with panic hooks and logging
///
/// Note: This is not marked with #[wasm_bindgen(start)] to avoid conflicts
/// when app_wasm is used as a library in applications that have their own
/// start function (like aura-console). Call this function or init_manually()
/// from your application's start function instead.
pub fn initialize_wasm() {
    console_error_panic_hook::set_once();
    crate::logging::init_logging();
}

/// Manual initialization for clients that don't use wasm_bindgen(start)
pub fn init_manually() {
    console_error_panic_hook::set_once();
    crate::logging::init_logging();
}
