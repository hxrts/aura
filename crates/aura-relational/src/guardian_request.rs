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
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Type identifier for guardian request facts.
pub const GUARDIAN_REQUEST_FACT_TYPE_ID: &str = "guardian_request";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianRequestFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "guardian_request", schema_version = 1, context = "context_id")]
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

    pub fn binding_key(&self) -> GuardianRequestFactKey {
        let sub_type = match self {
            GuardianRequestFact::Requested { .. } => "guardian-requested",
            GuardianRequestFact::Cancelled { .. } => "guardian-cancelled",
        };
        GuardianRequestFactKey {
            sub_type,
            data: hash::hash(&self.to_bytes()).to_vec(),
        }
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

        let fact = GuardianRequestFact::from_bytes(binding_data)?;
        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
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
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
    }
}
