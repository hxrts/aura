//! Authority-Centric Context Types
//!
//! Provides context types that expose authority/context identifiers as first-class
//! while keeping device-local details derived internally.

use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};

/// Derive the default context ID for an authority.
pub fn default_context_id_for_authority(authority_id: AuthorityId) -> ContextId {
    ContextId::new_from_entropy(hash(&authority_id.to_bytes()))
}

/// Authority-first context for agent operations
///
/// All agent operations work with authorities and contexts, never exposing
/// device-local details in the public API.
#[derive(Debug, Clone)]
pub struct AuthorityContext {
    /// The authority this agent represents
    authority_id: AuthorityId,

    /// Cached account ID derived from authority (computed once at construction)
    account_id: AccountId,

    /// Internal device identifier (derived from authority, not exposed publicly)
    #[allow(dead_code)]
    pub(crate) device_id: aura_core::identifiers::DeviceId,
}

/// Information about a relational context
#[derive(Debug, Clone)]
pub struct RelationalContext {
    /// Context identifier
    pub context_id: ContextId,

    /// Other authorities participating in this context
    pub participants: Vec<AuthorityId>,

    /// Context-specific metadata
    pub metadata: ContextMetadata,
}

/// Context metadata
#[derive(Debug, Clone, Default)]
pub struct ContextMetadata {
    /// Human-readable name (optional)
    pub name: Option<String>,

    /// Context creation timestamp
    pub created_at: u64,

    /// Last activity timestamp
    pub last_activity: u64,
}

impl AuthorityContext {
    /// Create a new authority context
    pub fn new(authority_id: AuthorityId) -> Self {
        // Legacy default: derive device_id from authority (single-device mapping).
        let device_id = DeviceId(authority_id.0);
        Self::new_with_device(authority_id, device_id)
    }

    /// Create a new authority context with an explicit device id.
    ///
    /// This enables multiple device runtimes to operate on behalf of the same
    /// account authority (demo/simulation and future production multi-device).
    pub fn new_with_device(authority_id: AuthorityId, device_id: DeviceId) -> Self {
        // Compute account_id once at construction (cached for all service calls)
        let account_id = AccountId::new_from_entropy(hash(&authority_id.to_bytes()));

        Self {
            authority_id,
            account_id,
            device_id,
        }
    }

    /// Authority identifier for this context.
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Cached account identifier derived from the authority.
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Default context identifier derived from the authority.
    pub fn default_context_id(&self) -> ContextId {
        default_context_id_for_authority(self.authority_id)
    }

    /// Internal access to device ID (crate-private)
    #[allow(dead_code)]
    pub(crate) fn device_id(&self) -> aura_core::identifiers::DeviceId {
        self.device_id
    }
}

impl RelationalContext {
    /// Create a new relational context
    pub async fn new<T: PhysicalTimeEffects>(
        context_id: ContextId,
        participants: Vec<AuthorityId>,
        time_effects: &T,
    ) -> Self {
        let timestamp = time_effects
            .physical_time()
            .await
            .map(|t| t.ts_ms / 1000)
            .unwrap_or(0);
        Self {
            context_id,
            participants,
            metadata: ContextMetadata {
                created_at: timestamp,
                last_activity: timestamp,
                ..Default::default()
            },
        }
    }

    /// Update last activity timestamp
    pub async fn touch<T: PhysicalTimeEffects>(&mut self, time_effects: &T) {
        self.metadata.last_activity = time_effects
            .physical_time()
            .await
            .map(|t| t.ts_ms / 1000)
            .unwrap_or(0);
    }
}
