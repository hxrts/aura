//! Guardian request helpers for relational contexts
//!
//! Encodes guardian request/cancel operations as `RelationalFact::Generic`
//! with a stable binding type. This keeps the core `RelationalFact` enum
//! small while allowing structured guardian flows.

use aura_core::relational::fact::{GenericBinding, RelationalFact};
use aura_core::relational::GuardianParameters;
use aura_core::{AuthorityId, Hash32, TimeStamp};
use serde::{Deserialize, Serialize};

pub const BINDING_TYPE_REQUEST: &str = "guardian_request";
pub const BINDING_TYPE_CANCEL: &str = "guardian_request_cancel";

/// Structured guardian request payload stored inside GenericBinding
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuardianRequestPayload {
    pub account_commitment: Hash32,
    pub guardian_commitment: Hash32,
    pub requester: AuthorityId,
    pub parameters: GuardianParameters,
    pub requested_at: TimeStamp,
    pub expires_at: Option<TimeStamp>,
}

/// Create a Generic relational fact for a guardian request
pub fn make_guardian_request_fact(
    payload: GuardianRequestPayload,
) -> Result<RelationalFact, bincode::Error> {
    let bytes = bincode::serialize(&payload)?;
    Ok(RelationalFact::Generic(GenericBinding::new(
        BINDING_TYPE_REQUEST.to_string(),
        bytes,
    )))
}

/// Create a Generic relational fact for guardian request cancellation
pub fn make_guardian_cancel_fact(
    payload: GuardianRequestPayload,
) -> Result<RelationalFact, bincode::Error> {
    let bytes = bincode::serialize(&payload)?;
    Ok(RelationalFact::Generic(GenericBinding::new(
        BINDING_TYPE_CANCEL.to_string(),
        bytes,
    )))
}

/// Attempt to decode a guardian request payload from a GenericBinding
pub fn parse_guardian_request(binding: &GenericBinding) -> Option<GuardianRequestPayload> {
    if binding.binding_type != BINDING_TYPE_REQUEST && binding.binding_type != BINDING_TYPE_CANCEL {
        return None;
    }
    bincode::deserialize::<GuardianRequestPayload>(&binding.binding_data).ok()
}
