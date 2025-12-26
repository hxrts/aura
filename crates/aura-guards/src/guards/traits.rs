//! Effect System Trait for Guards
//!
//! Compatibility shims for guard callers while migrating fully to the pure
//! guard interpreter path (ADR-014). This module intentionally limits the
//! surface area to authority/metadata access.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::AuthorityId;

/// Minimal context provider for guards (authority + metadata).
pub trait GuardContextProvider {
    fn authority_id(&self) -> AuthorityId;
    fn get_metadata(&self, key: &str) -> Option<String>;
    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Production
    }
    fn can_perform_operation(&self, _operation: &str) -> bool {
        true
    }
}

pub const META_BISCUIT_TOKEN: &str = "biscuit_token";
pub const META_BISCUIT_ROOT_PK: &str = "biscuit_root_pk";

pub fn require_biscuit_metadata(
    provider: &impl GuardContextProvider,
) -> aura_core::AuraResult<(String, String)> {
    let token = provider.get_metadata(META_BISCUIT_TOKEN).ok_or_else(|| {
        aura_core::AuraError::invalid("missing biscuit_token metadata".to_string())
    })?;
    let root_pk = provider.get_metadata(META_BISCUIT_ROOT_PK).ok_or_else(|| {
        aura_core::AuraError::invalid("missing biscuit_root_pk metadata".to_string())
    })?;
    Ok((token, root_pk))
}

/// Security context for guard operations
#[derive(Debug, Clone)]
pub struct SecurityContext {
    /// Authority performing the operation
    pub authority_id: AuthorityId,
    /// Current security level
    pub security_level: SecurityLevel,
    /// Whether hardware security is available
    pub hardware_secure: bool,
}

/// Security level enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Low security - testing/development
    Low,
    /// Normal security - production
    Normal,
    /// High security - sensitive operations
    High,
}

impl Default for SecurityContext {
    fn default() -> Self {
        Self {
            authority_id: AuthorityId::default(),
            security_level: SecurityLevel::Normal,
            hardware_secure: false,
        }
    }
}
