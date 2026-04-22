use super::*;
use aura_signature::{verify_frost_transcript, SecurityTranscript};

#[derive(Debug, Clone, Serialize)]
struct UpgradeProposalTranscriptPayload {
    package_id: uuid::Uuid,
    version: SemanticVersion,
    artifact_hash: Hash32,
    kind: UpgradeKind,
    activation_fence: Option<IdentityEpochFence>,
}

struct UpgradeProposalTranscript<'a> {
    proposal: &'a UpgradeProposal,
}

impl SecurityTranscript for UpgradeProposalTranscript<'_> {
    type Payload = UpgradeProposalTranscriptPayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.maintenance.upgrade-proposal";

    fn transcript_payload(&self) -> Self::Payload {
        UpgradeProposalTranscriptPayload {
            package_id: self.proposal.package_id,
            version: self.proposal.version,
            artifact_hash: self.proposal.artifact_hash,
            kind: self.proposal.kind,
            activation_fence: self.proposal.activation_fence.clone(),
        }
    }
}

impl MaintenanceService {
    /// Verify threshold signature for a maintenance operation.
    pub(super) async fn verify_threshold_signature<C: aura_core::effects::CryptoEffects>(
        &self,
        authority_id: AuthorityId,
        proposal: &UpgradeProposal,
        crypto_effects: &C,
        key_resolver: &impl aura_core::TrustedKeyResolver,
        threshold_signature: &[u8],
    ) -> SyncResult<()> {
        let transcript = UpgradeProposalTranscript { proposal };
        let trusted_key = key_resolver
            .resolve_release_key(authority_id)
            .map_err(|e| {
                crate::core::errors::sync_validation_error(format!(
                    "release key resolution failed: {e}"
                ))
            })?;

        match verify_frost_transcript(
            crypto_effects,
            &transcript,
            threshold_signature,
            trusted_key.bytes(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_transcript_binds_activation_fence() {
        let base = UpgradeProposal {
            package_id: uuid::Uuid::from_bytes([1; 16]),
            version: SemanticVersion::new(1, 2, 3),
            artifact_hash: Hash32::from([2; 32]),
            artifact_uri: None,
            kind: UpgradeKind::HardFork,
            activation_fence: Some(IdentityEpochFence::new(
                AccountId(AuthorityId::new_from_entropy([3; 32]).0),
                Epoch::new(10),
            )),
        };
        let mut changed = base.clone();
        changed.activation_fence = Some(IdentityEpochFence::new(
            AccountId(AuthorityId::new_from_entropy([3; 32]).0),
            Epoch::new(11),
        ));

        let base_bytes = UpgradeProposalTranscript { proposal: &base }
            .transcript_bytes()
            .unwrap();
        let changed_bytes = UpgradeProposalTranscript { proposal: &changed }
            .transcript_bytes()
            .unwrap();

        assert_ne!(base_bytes, changed_bytes);
    }
}
