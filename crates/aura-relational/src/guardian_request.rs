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
        Self::Requested {
            context_id,
            payload,
        }
    }

    pub fn cancelled(context_id: ContextId, payload: GuardianRequestPayload) -> Self {
        Self::Cancelled {
            context_id,
            payload,
        }
    }

    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id() == context_id
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

    #[allow(clippy::expect_used)] // DomainFact::to_bytes is infallible by trait signature.
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
        if !fact.validate_for_reduction(context_id) {
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
pub fn parse_guardian_request(
    binding_type: &str,
    binding_data: &[u8],
) -> Option<GuardianRequestFact> {
    if binding_type != GUARDIAN_REQUEST_FACT_TYPE_ID {
        return None;
    }
    GuardianRequestFact::from_bytes(binding_data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::relational::GuardianParameters;
    use aura_core::time::PhysicalTime;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_payload() -> GuardianRequestPayload {
        GuardianRequestPayload {
            account_commitment: Hash32::from_bytes(&hash::hash(b"account")),
            guardian_commitment: Hash32::from_bytes(&hash::hash(b"guardian")),
            requester: test_authority(1),
            parameters: GuardianParameters::default(),
            requested_at: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
            expires_at: None,
        }
    }

    #[test]
    fn test_guardian_request_reducer_idempotence() {
        let reducer = GuardianRequestFactReducer;
        let context_id = test_context_id();
        let fact = GuardianRequestFact::requested(context_id, test_payload());
        let bytes = fact.to_bytes();

        let binding1 = reducer.reduce(context_id, GUARDIAN_REQUEST_FACT_TYPE_ID, &bytes);
        let binding2 = reducer.reduce(context_id, GUARDIAN_REQUEST_FACT_TYPE_ID, &bytes);
        assert_eq!(binding1, binding2);
    }
}
