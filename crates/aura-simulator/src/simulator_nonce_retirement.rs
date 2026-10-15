//! Simulator-only retirement shared by fresh instances of the same signer.
//! It lives outside protected profile checkpoints and never stores nonce secrets.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use aura_core::{
    crypto::tree_signing::{FrostNonceRetirement, FrostNonceRetirementRecord},
    AuraError,
};

/// The original fixture owns this ledger across every restored branch. It is
/// deliberately not checkpointed or made reachable from deployable facades.
#[derive(Default)]
pub struct SimulatorNonceRetirementLedger {
    retired: Mutex<HashMap<([u8; 32], u64, FrostNonceRetirementRecord), u64>>,
}

impl SimulatorNonceRetirementLedger {
    /// Bind one runtime to its original signer and epoch. Branch ids describe
    /// observations; they never authorize reusing an actual commitment.
    pub fn scoped(
        self: &Arc<Self>,
        signer: [u8; 32],
        epoch: u64,
        branch: u64,
    ) -> Arc<dyn FrostNonceRetirement> {
        Arc::new(ScopedSimulatorNonceRetirement {
            ledger: Arc::clone(self),
            signer,
            epoch,
            branch,
        })
    }

    /// Public commitment digests observed for one signer; no nonce/share bytes.
    pub fn records_for(&self, signer: [u8; 32], epoch: u64) -> Vec<FrostNonceRetirementRecord> {
        self.retired
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .filter(|(owner, original_epoch, _)| *owner == signer && *original_epoch == epoch)
            .map(|(_, _, record)| record.clone())
            .collect()
    }
}

struct ScopedSimulatorNonceRetirement {
    ledger: Arc<SimulatorNonceRetirementLedger>,
    signer: [u8; 32],
    epoch: u64,
    branch: u64,
}

#[async_trait::async_trait]
impl FrostNonceRetirement for ScopedSimulatorNonceRetirement {
    async fn retire_frost_nonce(
        &self,
        record: &FrostNonceRetirementRecord,
    ) -> Result<(), AuraError> {
        let mut retired = self
            .ledger
            .retired
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match retired.entry((self.signer, self.epoch, record.clone())) {
            std::collections::hash_map::Entry::Occupied(_) => Err(AuraError::crypto(
                "simulator FROST commitment already retired",
            )),
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(self.branch);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_signer_epoch_commitment_cannot_retire_in_two_branches() {
        let ledger = Arc::new(SimulatorNonceRetirementLedger::default());
        let record = FrostNonceRetirementRecord {
            participant: 1,
            commitment_digest: aura_core::Hash32::from_bytes(b"public test commitment"),
        };
        ledger
            .scoped([1; 32], 0, 1)
            .retire_frost_nonce(&record)
            .await
            .unwrap();
        assert!(ledger
            .scoped([1; 32], 0, 2)
            .retire_frost_nonce(&record)
            .await
            .is_err());
        assert_eq!(ledger.records_for([1; 32], 0), vec![record.clone()]);
        ledger
            .scoped([2; 32], 0, 2)
            .retire_frost_nonce(&record)
            .await
            .unwrap();
        ledger
            .scoped([1; 32], 1, 2)
            .retire_frost_nonce(&record)
            .await
            .unwrap();
    }
}
