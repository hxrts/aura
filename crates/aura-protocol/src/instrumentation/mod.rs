//! Dev Console Instrumentation
//!
//! This module provides instrumentation hooks for the Aura Dev Console,
//! enabling real-time observation and debugging of protocol execution.
//!
//! The instrumentation is designed to be:
//! - **Non-intrusive**: Zero performance overhead when disabled
//! - **Optional**: Controlled by compile-time features
//! - **Compatible**: Works with console-types for WebSocket protocol

#[cfg(feature = "dev-console")]
pub mod context_hooks;
#[cfg(feature = "dev-console")]
pub mod events;
#[cfg(feature = "dev-console")]
pub mod recorder;

// Re-export instrumentation types when feature is enabled
#[cfg(feature = "dev-console")]
pub use context_hooks::InstrumentationHooks;
#[cfg(feature = "dev-console")]
pub use events::{ConsoleEvent, ProtocolEvent};
#[cfg(feature = "dev-console")]
pub use recorder::TraceRecorder;

// No-op stubs when feature is disabled
#[cfg(not(feature = "dev-console"))]
pub struct TraceRecorder;

#[cfg(not(feature = "dev-console"))]
impl TraceRecorder {
    pub fn new() -> Self {
        Self
    }
    pub fn record_event(&mut self, _event: &str) {}
    pub fn export_trace(&self) -> Vec<u8> {
        Vec::new()
    }
}

#[cfg(not(feature = "dev-console"))]
impl Default for TraceRecorder {
    fn default() -> Self {
        Self::new()
    }
}
