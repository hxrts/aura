//! Guardian key recovery data structures supporting recovery choreography and approvals.
//!
//! Uses the authority model - guardians are identified by AuthorityId.

use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};

/// Guardian approval for key recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianKeyApproval {
    /// Guardian's authority identifier
    pub guardian_id: AuthorityId,
    /// Encrypted key share from this guardian
    pub key_share: Vec<u8>,
    /// Guardian's partial signature over the recovery grant
    pub partial_signature: Vec<u8>,
    /// Timestamp when approval was given
    pub timestamp: TimeStamp,
}
