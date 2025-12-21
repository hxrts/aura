//! Authority-Centric Context Types
//!
//! Provides context types that expose authority/context identifiers as first-class
//! while keeping device-local details derived internally.

use aura_core::effects::PhysicalTimeEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, SessionId};
use std::collections::HashMap;

/// Authority-first context for agent operations
///
/// All agent operations work with authorities and contexts, never exposing
/// device-local details in the public API.
#[derive(Debug, Clone)]
pub struct AuthorityContext {
    /// The authority this agent represents
    pub authority_id: AuthorityId,

    /// Cached account ID derived from authority (computed once at construction)
    pub account_id: AccountId,

    /// Active relational contexts for this authority
    pub active_contexts: HashMap<ContextId, RelationalContext>,

    /// Current session (if any)
    pub session_id: Option<SessionId>,

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
        // Derive device_id internally from authority (1:1 mapping for single-device authorities)
        let device_id = aura_core::identifiers::DeviceId(authority_id.0);

        // Compute account_id once at construction (cached for all service calls)
        let account_id = AccountId::new_from_entropy(hash(&authority_id.to_bytes()));

        Self {
            authority_id,
            account_id,
            active_contexts: HashMap::new(),
            session_id: None,
            device_id,
        }
    }

    /// Add a relational context
    pub fn add_context(&mut self, context: RelationalContext) {
        self.active_contexts.insert(context.context_id, context);
    }

    /// Get a relational context
    pub fn get_context(&self, context_id: &ContextId) -> Option<&RelationalContext> {
        self.active_contexts.get(context_id)
    }

    /// Start a new session
    pub fn start_session(&mut self) -> SessionId {
        let session_id = SessionId::new();
        self.session_id = Some(session_id);
        session_id
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
