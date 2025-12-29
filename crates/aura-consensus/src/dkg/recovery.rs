//! Share recovery from a finalized transcript (BFT-DKG).

use super::types::DkgTranscript;
use aura_core::{AuraError, Result};

pub fn recover_share_from_transcript(_transcript: &DkgTranscript) -> Result<Vec<u8>> {
    Err(AuraError::invalid(
        "DKG share recovery is not implemented yet",
    ))
}
