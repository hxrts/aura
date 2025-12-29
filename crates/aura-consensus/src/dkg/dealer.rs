//! Dealer contribution generation (BFT-DKG).

use super::types::{DealerPackage, DkgConfig};
use aura_core::{AuraError, Result};

pub fn build_dealer_package(_config: &DkgConfig) -> Result<DealerPackage> {
    Err(AuraError::invalid(
        "DKG dealer package generation is not implemented yet",
    ))
}
