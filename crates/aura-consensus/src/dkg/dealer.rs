//! Dealer contribution generation (BFT-DKG).

use super::types::{DealerPackage, DkgConfig};
use aura_core::util::serialization::to_vec;
use aura_core::{hash, AuraError, AuthorityId, Result};
use std::collections::BTreeMap;

#[derive(serde::Serialize)]
struct DealerShareDigest<'a> {
    dealer: AuthorityId,
    participant: AuthorityId,
    config: &'a DkgConfig,
}

#[derive(serde::Serialize)]
struct DealerPackageDigest<'a> {
    dealer: AuthorityId,
    config: &'a DkgConfig,
}

/// Build a deterministic dealer package for the given dealer.
///
/// Note: This is a placeholder construction that produces stable, verifiable
/// payloads for orchestration and testing. Real cryptographic DKG will replace
/// this with verifiable secret sharing and proofs.
pub fn build_dealer_package(config: &DkgConfig, dealer: AuthorityId) -> Result<DealerPackage> {
    if config.participants.is_empty() {
        return Err(AuraError::invalid(
            "DKG config requires explicit participants",
        ));
    }

    let mut encrypted_shares = BTreeMap::new();
    for participant in &config.participants {
        let digest = DealerShareDigest {
            dealer,
            participant: *participant,
            config,
        };
        let encoded = to_vec(&digest).map_err(|e| AuraError::serialization(e.to_string()))?;
        let mut hasher = hash::hasher();
        hasher.update(b"AURA_DKG_SHARE");
        hasher.update(&encoded);
        encrypted_shares.insert(*participant, hasher.finalize().to_vec());
    }

    let package_digest = DealerPackageDigest { dealer, config };
    let package_bytes =
        to_vec(&package_digest).map_err(|e| AuraError::serialization(e.to_string()))?;

    let mut commitment_hasher = hash::hasher();
    commitment_hasher.update(b"AURA_DKG_COMMITMENT");
    commitment_hasher.update(&package_bytes);

    let mut proof_hasher = hash::hasher();
    proof_hasher.update(b"AURA_DKG_PROOF");
    proof_hasher.update(&package_bytes);

    Ok(DealerPackage {
        dealer,
        commitment: commitment_hasher.finalize().to_vec(),
        encrypted_shares,
        proof: proof_hasher.finalize().to_vec(),
    })
}
