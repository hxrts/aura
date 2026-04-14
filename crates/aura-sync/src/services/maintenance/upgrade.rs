use super::*;

impl MaintenanceService {
    /// Verify threshold signature for a maintenance operation.
    pub(super) async fn verify_threshold_signature<C: aura_core::effects::CryptoEffects>(
        &self,
        proposal: &UpgradeProposal,
        crypto_effects: &C,
        threshold_signature: &[u8],
        group_public_key: &[u8],
    ) -> SyncResult<()> {
        let message = self.construct_upgrade_message(proposal);

        match crypto_effects
            .frost_verify(&message, threshold_signature, group_public_key)
            .await
        {
            Ok(true) => {
                tracing::info!(
                    "Threshold signature verification successful for upgrade proposal {}",
                    proposal.package_id
                );
                Ok(())
            }
            Ok(false) => {
                let error_msg = format!(
                    "Threshold signature verification failed for upgrade proposal {}",
                    proposal.package_id
                );
                tracing::error!("{}", error_msg);
                Err(crate::core::errors::sync_validation_error(error_msg))
            }
            Err(e) => {
                let error_msg = format!(
                    "Threshold signature verification error for upgrade proposal {}: {}",
                    proposal.package_id, e
                );
                tracing::error!("{}", error_msg);
                Err(crate::core::errors::sync_validation_error(error_msg))
            }
        }
    }

    /// Construct message for upgrade proposal signature verification.
    #[allow(clippy::unwrap_used)] // Vec::write_all is infallible
    fn construct_upgrade_message(&self, proposal: &UpgradeProposal) -> Vec<u8> {
        use std::io::Write;

        let mut message = Vec::new();
        message.write_all(b"AURA_UPGRADE_PROPOSAL").unwrap();
        message.write_all(proposal.package_id.as_bytes()).unwrap();
        message
            .write_all(proposal.version.to_string().as_bytes())
            .unwrap();
        message.write_all(&proposal.artifact_hash.0).unwrap();

        match proposal.kind {
            UpgradeKind::SoftFork => message.write_all(b"SOFT_FORK").unwrap(),
            UpgradeKind::HardFork => message.write_all(b"HARD_FORK").unwrap(),
        }

        if let Some(ref fence) = proposal.activation_fence {
            message.write_all(fence.account_id.0.as_bytes()).unwrap();
            message
                .write_all(&fence.epoch.value().to_le_bytes())
                .unwrap();
        }

        message
    }

    /// Map activation_epoch to IdentityEpochFence.
    pub(super) fn map_activation_epoch(
        proposal: &crate::protocols::ota::UpgradeProposal,
        proposer: AuthorityId,
    ) -> Option<IdentityEpochFence> {
        proposal.activation_epoch.map(|activation_epoch| {
            IdentityEpochFence::new(AccountId(proposer.0), activation_epoch)
        })
    }

    /// Generate artifact URI for package downloads.
    pub(super) fn generate_artifact_uri(
        proposal: &crate::protocols::ota::UpgradeProposal,
        version: &SemanticVersion,
    ) -> Option<String> {
        Some(format!(
            "aura://{}/{}/{:02x}{:02x}{:02x}{:02x}",
            proposal.package_id.hyphenated(),
            version,
            proposal.package_hash.0[0],
            proposal.package_hash.0[1],
            proposal.package_hash.0[2],
            proposal.package_hash.0[3]
        ))
    }
}
