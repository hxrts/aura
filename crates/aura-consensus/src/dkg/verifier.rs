//! Dealer package verification (BFT-DKG).

use super::types::DealerPackage;
use aura_core::{AuraError, Result};

pub fn verify_dealer_package(_package: &DealerPackage) -> Result<()> {
    if _package.commitment.is_empty() {
        return Err(AuraError::invalid("DKG package commitment is empty"));
    }
    if _package.encrypted_shares.is_empty() {
        return Err(AuraError::invalid("DKG package has no encrypted shares"));
    }
    if _package.proof.is_empty() {
        return Err(AuraError::invalid("DKG package proof is empty"));
    }
    Ok(())
}
