//! Runtime-owned local signer for explicitly approved enrollment transcripts.
//!
//! Participants consume explicit local approval and publish public outputs only.
//! The coordinator has no API accepting a private share or nonce.
use std::sync::Arc;

use aura_core::effects::crypto::{FrostPublicCommitment, FrostSigningPackage};
use aura_core::effects::secure::{ImmutableSecureStoreOutcome, SecureStorageCapability};
use aura_core::effects::{CryptoExtendedEffects, SecureStorageEffects};
use aura_core::{AuraError, DeviceId};
use tokio::sync::{mpsc, oneshot, watch};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentTranscriptSigningError {
    #[error("original signing participant actor stopped")]
    ActorStopped,
    #[error("signing participant response was cancelled")]
    ResponseCancelled,
    #[error("original transcript approval was already consumed")]
    ApprovalConsumed,
    #[error("participant signing package differs from original local approval")]
    PackageBinding,
    #[error("public coordinator participant inventory is invalid")]
    Inventory,
}

fn rejected(source: EnrollmentTranscriptSigningError) -> AuraError {
    AuraError::Crypto {
        message: "owned enrollment transcript signing rejected".into(),
        source: Some(Arc::new(source)),
    }
}

fn native_commitment_matches(
    package: &FrostSigningPackage,
    index: u16,
    original: Option<&Vec<u8>>,
) -> Result<bool, AuraError> {
    let native =
        frost_ed25519::SigningPackage::deserialize(&package.package).map_err(|source| {
            AuraError::crypto_with_source(
                "decode original participant signing package",
                Arc::new(source),
            )
        })?;
    let identifier = frost_ed25519::Identifier::try_from(index).map_err(|source| {
        AuraError::crypto_with_source(
            "decode original participant signing identifier",
            Arc::new(source),
        )
    })?;
    let commitment = native
        .signing_commitments()
        .get(&identifier)
        .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::PackageBinding))?;
    let original =
        original.ok_or_else(|| rejected(EnrollmentTranscriptSigningError::PackageBinding))?;
    let actual = commitment.serialize().map_err(|source| {
        AuraError::crypto_with_source(
            "encode original participant signing commitment",
            Arc::new(source),
        )
    })?;
    Ok(original == &actual)
}

#[cfg(test)]
mod native_commitment_tests {
    use super::*;

    #[test]
    fn malformed_native_package_retains_original_codec_cause() {
        let package = FrostSigningPackage {
            message: vec![],
            package: vec![0xff],
            participants: vec![1],
            public_key_package: vec![],
        };
        let original = vec![];
        let error = native_commitment_matches(&package, 1, Some(&original))
            .expect_err("malformed native package must fail before signing");
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        let mut native_codec = false;
        loop {
            native_codec |= cause.is::<frost_ed25519::Error>();
            match cause.source() {
                Some(source) => cause = source,
                None => break,
            }
        }
        assert!(
            native_codec,
            "native codec source must reach actor supervision"
        );
    }
}

use super::{ApprovedEnrollmentTranscriptRound, ApprovedLocalEnrollmentTranscript};

pub(super) enum ParticipantRequest {
    Commitment(oneshot::Sender<Result<FrostPublicCommitment, AuraError>>),
    Share(
        FrostSigningPackage,
        oneshot::Sender<Result<Vec<u8>, AuraError>>,
    ),
}

/// Bounded public ingress. It exposes neither local effects nor private keys.
#[derive(Clone)]
pub(crate) struct EnrollmentTranscriptParticipantIngress {
    device: DeviceId,
    index: u16,
    requests: mpsc::Sender<ParticipantRequest>,
    terminal_failure: watch::Receiver<Option<AuraError>>,
}

impl EnrollmentTranscriptParticipantIngress {
    async fn stopped(&self, source: EnrollmentTranscriptSigningError) -> AuraError {
        let mut terminal = self.terminal_failure.clone();
        loop {
            if let Some(original) = terminal.borrow().as_ref() {
                return original.clone();
            }
            if terminal.changed().await.is_err() {
                return rejected(source);
            }
        }
    }
    pub(super) async fn commitment(&self) -> Result<FrostPublicCommitment, AuraError> {
        let (sender, receiver) = oneshot::channel();
        if self
            .requests
            .send(ParticipantRequest::Commitment(sender))
            .await
            .is_err()
        {
            return Err(self
                .stopped(EnrollmentTranscriptSigningError::ActorStopped)
                .await);
        }
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(self
                .stopped(EnrollmentTranscriptSigningError::ResponseCancelled)
                .await),
        }
    }
    pub(super) async fn share(&self, package: FrostSigningPackage) -> Result<Vec<u8>, AuraError> {
        let (sender, receiver) = oneshot::channel();
        if self
            .requests
            .send(ParticipantRequest::Share(package, sender))
            .await
            .is_err()
        {
            return Err(self
                .stopped(EnrollmentTranscriptSigningError::ActorStopped)
                .await);
        }
        match receiver.await {
            Ok(outcome) => outcome,
            Err(_) => Err(self
                .stopped(EnrollmentTranscriptSigningError::ResponseCancelled)
                .await),
        }
    }
}

pub(super) fn remote_public_ingress(
    device: DeviceId,
    index: u16,
) -> (
    EnrollmentTranscriptParticipantIngress,
    mpsc::Receiver<ParticipantRequest>,
    watch::Sender<Option<AuraError>>,
) {
    let (requests, receiver) = mpsc::channel(2);
    let (terminal_sender, terminal_failure) = watch::channel(None);
    (
        EnrollmentTranscriptParticipantIngress {
            device,
            index,
            requests,
            terminal_failure,
        },
        receiver,
        terminal_sender,
    )
}

/// Returns an owned participant future for the existing runtime TaskGroup.
/// The caller must retain the actual returned owned task handle, observe its
/// terminal failure and drain it during service shutdown. No raw spawn occurs.
pub(super) fn admitted_participant<'tree, 'custody: 'tree, 'owner: 'custody, 'runtime: 'owner>(
    approval: ApprovedLocalEnrollmentTranscript<'tree, 'custody, 'owner, 'runtime>,
) -> (
    EnrollmentTranscriptParticipantIngress,
    impl std::future::Future<Output = Result<(), AuraError>>
        + 'tree
        + use<'tree, 'custody, 'owner, 'runtime>,
) {
    let (sender, mut receiver) = mpsc::channel(2);
    let (terminal_sender, terminal_failure) = watch::channel(None);
    let ingress = EnrollmentTranscriptParticipantIngress {
        device: approval.device,
        index: approval.index,
        requests: sender,
        terminal_failure,
    };
    let future = async move {
        let outcome = async {
            let effects = approval.effects.clone();
            approval
                .custody
                .generation()
                .require_effects(effects.as_ref())?;
            approval
                .original_window
                .execute(effects.as_ref(), || async {
                    for domain in &approval.domains {
                        // A consumed approval is durably recorded before nonce creation. A
                        // restart cannot generate another nonce under the same approval.
                        let retired = effects
                            .secure_store_immutable(
                                &domain.retirement_location,
                                &domain.approval_digest,
                                &[
                                    SecureStorageCapability::Read,
                                    SecureStorageCapability::Write,
                                ],
                            )
                            .await?;
                        if retired != ImmutableSecureStoreOutcome::Created {
                            return Err(rejected(
                                EnrollmentTranscriptSigningError::ApprovalConsumed,
                            ));
                        }
                        let mut nonces = Some(Zeroizing::new(
                            effects.frost_generate_nonces(approval.local_share).await?,
                        ));
                        let mut published_commitment = false;
                        let mut original_commitment = None;
                        while let Some(request) = receiver.recv().await {
                            match request {
                                ParticipantRequest::Commitment(response) => {
                                    if published_commitment {
                                        let source = rejected(
                                            EnrollmentTranscriptSigningError::ApprovalConsumed,
                                        );
                                        let _ = response.send(Err(source.clone()));
                                        return Err(source);
                                    }
                                    let Some(local) = nonces.as_ref() else {
                                        let source = rejected(
                                            EnrollmentTranscriptSigningError::ApprovalConsumed,
                                        );
                                        let _ = response.send(Err(source.clone()));
                                        return Err(source);
                                    };
                                    let commitment = match effects
                                        .frost_public_commitment(approval.index, local)
                                        .await
                                    {
                                        Ok(commitment) => commitment,
                                        Err(source) => {
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                    };
                                    original_commitment = Some(commitment.commitment_bytes.clone());
                                    published_commitment = true;
                                    if response.send(Ok(commitment)).is_err() {
                                        return Err(rejected(
                                            EnrollmentTranscriptSigningError::ResponseCancelled,
                                        ));
                                    }
                                }
                                ParticipantRequest::Share(package, response) => {
                                    let native_commitment_matches = match native_commitment_matches(
                                        &package,
                                        approval.index,
                                        original_commitment.as_ref(),
                                    ) {
                                        Ok(matches) => matches,
                                        Err(source) => {
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                    };
                                    if !published_commitment
                                        || package.message != domain.message
                                        || package.public_key_package != approval.public_package
                                        || !package.participants.contains(&approval.index)
                                        || !native_commitment_matches
                                    {
                                        let source = rejected(
                                            EnrollmentTranscriptSigningError::PackageBinding,
                                        );
                                        let _ = response.send(Err(source.clone()));
                                        return Err(source);
                                    }
                                    // Move the nonce out before any awaited primitive. Errors
                                    // and cancellation drop it; no retry can borrow it again.
                                    let Some(local) = nonces.take() else {
                                        let source = rejected(
                                            EnrollmentTranscriptSigningError::ApprovalConsumed,
                                        );
                                        let _ = response.send(Err(source.clone()));
                                        return Err(source);
                                    };
                                    let share = match effects
                                        .frost_sign_share_for_message(
                                            &package,
                                            approval.local_share,
                                            &local,
                                            &domain.message,
                                            &approval.public_package,
                                            approval.threshold,
                                        )
                                        .await
                                    {
                                        Ok(share) => share,
                                        Err(source) => {
                                            let _ = response.send(Err(source.clone()));
                                            return Err(source);
                                        }
                                    };
                                    response.send(Ok(share)).map_err(|_| {
                                        rejected(
                                            EnrollmentTranscriptSigningError::ResponseCancelled,
                                        )
                                    })?;
                                    break;
                                }
                            }
                        }
                        if nonces.is_some() {
                            return Err(rejected(
                                EnrollmentTranscriptSigningError::ResponseCancelled,
                            ));
                        }
                    }
                    Ok(())
                })
                .await
                .map_err(|source| AuraError::Crypto {
                    message: "original participant signing window failed".into(),
                    source: Some(Arc::new(source)),
                })
        }
        .await;
        if let Err(original) = &outcome {
            // Publish before dropping ingress: early protected-store/nonce/time
            // failures remain the concrete request error and owned task cause.
            let _ = terminal_sender.send(Some(original.clone()));
        }
        outcome
    };
    (ingress, future)
}

/// Public-only coordinator inputs. The constructor must be restricted to the
/// original local approved intent owner and retain its actual public policy.
impl ApprovedEnrollmentTranscriptRound {
    pub(super) async fn sign<T: aura_signature::SecurityTranscript>(
        self,
        transcript: &T,
        ingress: &[EnrollmentTranscriptParticipantIngress],
    ) -> Result<Vec<u8>, AuraError> {
        let original_bytes = transcript.required_transcript_bytes().map_err(|source| {
            AuraError::crypto_with_source(
                "encode original approved typed signing domain",
                Arc::new(source),
            )
        })?;
        if original_bytes != self.message {
            return Err(rejected(EnrollmentTranscriptSigningError::PackageBinding));
        }
        let effects = self.effects.clone();
        let admitted = ingress;
        let policy =
            aura_protocol::public_transcript_signing::PublicTranscriptSigningPolicy::checked(
                transcript,
                &self.public_package,
                &self.verifying_key,
                self.threshold,
                &self.participants,
            )?;
        if ingress.len() != self.participants.len()
            || self.participants.iter().any(|(device, index)| {
                ingress
                    .iter()
                    .filter(|entry| entry.device == *device && entry.index == *index)
                    .count()
                    != 1
            })
        {
            return Err(rejected(EnrollmentTranscriptSigningError::Inventory));
        }
        self.original_window
            .execute(effects.as_ref(), || async {
                aura_protocol::public_transcript_signing::sign_public_transcript(
                    effects.as_ref(),
                    policy,
                    |device, index| async move {
                        let participant = admitted
                            .iter()
                            .find(|entry| entry.device == device && entry.index == index)
                            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::Inventory))?;
                        participant.commitment().await
                    },
                    |device, index, package| async move {
                        let participant = admitted
                            .iter()
                            .find(|entry| entry.device == device && entry.index == index)
                            .ok_or_else(|| rejected(EnrollmentTranscriptSigningError::Inventory))?;
                        participant.share(package).await
                    },
                )
                .await
            })
            .await
            .map_err(|source| AuraError::Crypto {
                message: "original coordinator signing window failed".into(),
                source: Some(Arc::new(source)),
            })
    }
}
