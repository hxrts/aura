//! Effect API compatibility shim bridging the old and new effect system architectures.

/// Compatibility bridge for legacy effect APIs
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
