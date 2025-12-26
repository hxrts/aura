//! Protocol runtime configuration.

pub const DEFAULT_WIRE_FORMAT_VERSION: u16 = 1;

#[derive(Debug, Clone)]
pub struct ProtocolRuntimeConfig {
    pub wire_format_version: u16,
}

impl Default for ProtocolRuntimeConfig {
    fn default() -> Self {
        Self {
            wire_format_version: DEFAULT_WIRE_FORMAT_VERSION,
        }
    }
}
