//! Types for quorum-driven DKG.

use aura_core::{AuthorityId, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// DKG configuration parameters for an epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DkgConfig {
    pub epoch: u64,
    pub threshold: u16,
    pub max_signers: u16,
    pub membership_hash: Hash32,
    pub cutoff: u64,
}

/// Dealer contribution package (opaque until verification).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DealerPackage {
    pub dealer: AuthorityId,
    pub commitment: Vec<u8>,
    pub encrypted_shares: BTreeMap<AuthorityId, Vec<u8>>,
    pub proof: Vec<u8>,
}

/// Finalized DKG transcript (consensus-selected).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DkgTranscript {
    pub epoch: u64,
    pub membership_hash: Hash32,
    pub cutoff: u64,
    pub packages: Vec<DealerPackage>,
    pub transcript_hash: Hash32,
}
