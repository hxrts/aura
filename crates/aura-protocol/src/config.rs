//! Protocol runtime configuration.

/// Default wire format version for protocol messages.
pub const DEFAULT_WIRE_FORMAT_VERSION: u16 = 1;

/// Runtime configuration for protocol serialization and compatibility.
#[derive(Debug, Clone)]
pub struct ProtocolRuntimeConfig {
    /// Wire format version used for protocol serialization.
    pub wire_format_version: u16,
    /// Multiplier applied to computed termination budgets.
    pub termination_budget_multiplier: f64,
}

impl Default for ProtocolRuntimeConfig {
    fn default() -> Self {
        Self {
            wire_format_version: DEFAULT_WIRE_FORMAT_VERSION,
            termination_budget_multiplier: 1.0,
        }
    }
}
