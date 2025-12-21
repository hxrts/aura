//! Guardian request facts for relational contexts
//!
//! Guardian request/cancel operations are stored as domain facts using the
//! journal extensibility mechanism:
//! - encoded as `RelationalFact::Generic { binding_type: "guardian_request", .. }`
//! - reduced via an optional `FactReducer` for query/view indexing

use aura_core::identifiers::ContextId;
use aura_core::relational::GuardianParameters;
use aura_core::{hash, AuthorityId, Hash32, TimeStamp};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use serde::{Deserialize, Serialize};

/// Type identifier for guardian request facts.
pub const GUARDIAN_REQUEST_FACT_TYPE_ID: &str = "guardian_request";

/// Structured guardian request payload stored inside the fact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianRequestPayload {
    pub account_commitment: Hash32,
    pub guardian_commitment: Hash32,
    pub requester: AuthorityId,
    pub parameters: GuardianParameters,
    pub requested_at: TimeStamp,
    pub expires_at: Option<TimeStamp>,
}

/// Guardian request lifecycle fact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GuardianRequestFact {
    Requested {
        context_id: ContextId,
        payload: GuardianRequestPayload,
    },
    Cancelled {
        context_id: ContextId,
        payload: GuardianRequestPayload,
    },
}

impl GuardianRequestFact {
    pub fn requested(context_id: ContextId, payload: GuardianRequestPayload) -> Self {
        Self::Requested { context_id, payload }
    }

    pub fn cancelled(context_id: ContextId, payload: GuardianRequestPayload) -> Self {
        Self::Cancelled { context_id, payload }
    }
}

impl DomainFact for GuardianRequestFact {
    fn type_id(&self) -> &'static str {
        GUARDIAN_REQUEST_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            GuardianRequestFact::Requested { context_id, .. } => *context_id,
            GuardianRequestFact::Cancelled { context_id, .. } => *context_id,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).expect("GuardianRequestFact must serialize")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        bincode::deserialize(bytes).ok()
    }
}

/// Reducer for guardian request facts.
pub struct GuardianRequestFactReducer;

impl FactReducer for GuardianRequestFactReducer {
    fn handles_type(&self) -> &'static str {
        GUARDIAN_REQUEST_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != GUARDIAN_REQUEST_FACT_TYPE_ID {
            return None;
        }

        let fact: GuardianRequestFact = bincode::deserialize(binding_data).ok()?;
        if fact.context_id() != context_id {
            return None;
        }

        let sub = match &fact {
            GuardianRequestFact::Requested { .. } => "guardian-requested",
            GuardianRequestFact::Cancelled { .. } => "guardian-cancelled",
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub.to_string()),
            context_id,
            data: hash::hash(binding_data).to_vec(),
        })
    }
}

/// Best-effort decode helper when iterating Generic facts.
pub fn parse_guardian_request(binding_type: &str, binding_data: &[u8]) -> Option<GuardianRequestFact> {
    if binding_type != GUARDIAN_REQUEST_FACT_TYPE_ID {
        return None;
    }
    GuardianRequestFact::from_bytes(binding_data)
}
