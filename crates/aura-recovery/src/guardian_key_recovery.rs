//! Guardian key recovery data structures supporting recovery choreography and approvals.

use aura_core::identifiers::GuardianId;
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};

/// Guardian approval for key recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianKeyApproval {
    pub guardian_id: GuardianId,
    pub key_share: Vec<u8>,
    pub partial_signature: Vec<u8>,
    pub timestamp: TimeStamp,
}
