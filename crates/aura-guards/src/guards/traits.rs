Aura Development Environment
============================

Rust version: rustc 1.90.0 (1159e78c4 2025-09-14)
Cargo version: cargo 1.90.0 (840b83a10 2025-07-30)
Quint version: 0.25.1
Apalache version: 0.45.4
TLA+ tools: available
Node.js version: v20.19.5
Lean version: Lean (version 4.23.0, arm64-apple-darwin, commit v4.23.0, Release)
Aeneas version: available

Available commands:
  just --list          Show all available tasks
  just build           Build all crates
  just test            Run all tests
  just check           Run clippy and format check
  just quint-parse     Parse Quint files to JSON
  trunk serve          Serve console with hot reload (in console/)
  quint --help         Formal verification with Quint
  apalache-mc --help   Model checking with Apalache
  lean --help          Kernel verification with Lean 4
  aeneas --help        Rust-to-Lean translation
  crate2nix --help     Generate hermetic Nix builds

Hermetic builds:
  nix build            Build with crate2nix (hermetic)
  nix build .#aura-terminal Build specific package
  nix run              Run aura CLI hermetically
  nix flake check      Run hermetic tests

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
