//! Effect API stubs
//!
//! Placeholder for effect API compatibility layer that bridges
//! the old and new effect system architectures.

/// Stub effect API bridge
#[derive(Debug)]
pub struct EffectApiBridge;

impl EffectApiBridge {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self
    }
}

impl Default for EffectApiBridge {
    fn default() -> Self {
        Self::new()
    }
}
