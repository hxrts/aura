//! Share recovery from a finalized transcript (BFT-DKG).

use super::types::DkgTranscript;
use aura_core::{AuraError, AuthorityId, Result};

/// Recover the encrypted share payload for a specific authority.
///
/// Decryption of the returned payload is handled by higher-level ceremony logic.
pub fn recover_share_from_transcript(
    transcript: &DkgTranscript,
    authority_id: AuthorityId,
) -> Result<Vec<u8>> {
    for package in &transcript.packages {
        if let Some(payload) = package.encrypted_shares.get(&authority_id) {
            return Ok(payload.clone());
        }
    }

    Err(AuraError::not_found(
        "DKG transcript does not contain share for authority",
    ))
}
