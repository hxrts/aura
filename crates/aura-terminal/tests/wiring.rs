//! Callback wiring and signal dispatch tests.
//!
//! Verify that TUI callbacks correctly dispatch to app core operations
//! and that reactive signals propagate without drops or glitches.

mod wiring {
    mod integration_callback_wiring;
    mod integration_callback_wiring_batch2;
    mod integration_callback_wiring_batch3;
    mod integration_reactive_dispatch;
    mod integration_signals;
}
