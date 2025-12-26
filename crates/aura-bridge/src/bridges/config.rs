//! Bridge runtime configuration.

#[derive(Debug, Clone)]
pub struct BridgeRuntimeConfig {
    pub panic_on_error: bool,
}

impl Default for BridgeRuntimeConfig {
    fn default() -> Self {
        Self {
            panic_on_error: true,
        }
    }
}
