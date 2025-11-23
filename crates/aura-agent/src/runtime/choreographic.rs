//! Choreographic runtime stubs
//!
//! Placeholder for choreographic runtime integration that will connect
//! with the choreography system from aura-protocol.

/// Stub choreographic runtime
#[derive(Debug)]
pub struct ChoreographicRuntime;

impl ChoreographicRuntime {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ChoreographicRuntime {
    fn default() -> Self {
        Self::new()
    }
}
