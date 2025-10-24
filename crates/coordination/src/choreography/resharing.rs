//! Resharing Protocol Choreography
//!
//! This module implements the P2P key resharing protocol using choreographic
//! programming with the new helper abstractions.

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, ProtocolContext,
    ProtocolContextExt, ProtocolError, ProtocolErrorType, SessionLifecycle,
};
use aura_crypto::{LagrangeInterpolation, ShamirPolynomial, SharePoint};
use aura_journal::{
    AcknowledgeSubShareEvent, DeviceId, DistributeSubShareEvent,
    EventType, FinalizeResharingEvent, InitiateResharingEvent, OperationType,
    ParticipantId as JournalParticipantId, ProtocolType, Session,
};
use std::collections::BTreeMap;

/// Resharing Protocol implementation using SessionLifecycle trait
pub struct ResharingProtocol<'a> {
    ctx: &'a mut ProtocolContext,
    new_threshold: Option<u16>,
    new_participants: Option<Vec<DeviceId>>,
}

impl<'a> ResharingProtocol<'a> {
    pub fn new(
        ctx: &'a mut ProtocolContext,
        new_threshold: Option<u16>,
        new_participants: Option<Vec<DeviceId>>,
    ) -> Self {
        Self {
            ctx,
            new_threshold,
            new_participants,
        }
    }
}

#[async_trait::async_trait]
impl<'a> SessionLifecycle for ResharingProtocol<'a> {
    type Result = Vec<u8>; // Success indicator

    fn operation_type(&self) -> OperationType {
        OperationType::Resharing
    }

    fn generate_context_id(&self) -> Vec<u8> {
        format!(
            "resharing:{}:{:?}",
            self.new_threshold.unwrap_or(self.ctx.threshold.unwrap_or(2) as u16),
            self.new_participants
                .as_ref()
                .unwrap_or(&self.ctx.participants)
        )
        .into_bytes()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Convert participants to session participants
        let session_participants: Vec<JournalParticipantId> = self
            .ctx
            .participants
            .iter()
            .map(|&device_id| JournalParticipantId::Device(device_id))
            .collect();

        // Create Resharing session
        Ok(Session::new(
            aura_journal::SessionId(self.ctx.session_id),
            ProtocolType::Resharing,
            session_participants,
            ledger_context.epoch,
            100, // TTL in epochs
            self.ctx.effects.now().map_err(|e| ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
        ))
    }

    async fn execute_protocol(&mut self, _session: &Session) -> Result<Vec<u8>, ProtocolError> {
        // Get current participants and new configuration
        let participants = self.ctx.participants.clone();
        let new_participants = self
            .new_participants
            .clone()
            .unwrap_or_else(|| participants.clone());
        let new_threshold = self
            .new_threshold
            .unwrap_or(self.ctx.threshold.unwrap_or(2) as u16);

        // Phase 1: Initiate Resharing (only coordinator)
        if participants.first() == Some(&DeviceId(self.ctx.device_id)) {
            let start_epoch = self.ctx.fetch_ledger_context().await?.epoch;
            let session_id = self.ctx.session_id;
            let old_threshold = self.ctx.threshold.unwrap_or(2) as u16;

            EventBuilder::new(self.ctx)
                .with_type(EventType::InitiateResharing(InitiateResharingEvent {
                    session_id,
                    old_threshold,
                    new_threshold,
                    old_participants: participants.clone(),
                    new_participants: new_participants.clone(),
                    start_epoch,
                    ttl_in_epochs: 100,
                }))
                .with_device_auth()
                .build_sign_and_emit()
                .await?;
        }

        // Wait for initiation event
        let session_id = self.ctx.session_id;
        let initiation_event = EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::InitiateResharing])
            .from_authors(participants.first().cloned().into_iter())
            .await_single(100)
            .await?;

        let (final_new_participants, final_new_threshold) = match &initiation_event.event_type {
            EventType::InitiateResharing(ref initiate) => {
                (initiate.new_participants.clone(), initiate.new_threshold)
            }
            _ => {
                return Err(ProtocolError {
                    session_id: self.ctx.session_id,
                    error_type: ProtocolErrorType::UnexpectedEvent,
                    message: "Expected InitiateResharing event".to_string(),
                });
            }
        };

        // Phase 2: Distribute Sub-shares
        if participants.contains(&DeviceId(self.ctx.device_id)) {
            self.distribute_sub_shares(&final_new_participants, final_new_threshold)
                .await?;
        }

        // Phase 3: Collect Sub-shares (for new participants)
        let mut collected_sub_shares = BTreeMap::new();
        if final_new_participants.contains(&DeviceId(self.ctx.device_id)) {
            collected_sub_shares = self
                .collect_sub_shares(&final_new_participants, final_new_threshold)
                .await?;
        }

        // Phase 4: Reconstruct New Share
        if final_new_participants.contains(&DeviceId(self.ctx.device_id)) {
            self.reconstruct_share(&collected_sub_shares).await?;
        }

        // Phase 5: Verify via Test Signature (placeholder)
        if final_new_participants.contains(&DeviceId(self.ctx.device_id)) {
            self.verify_new_shares().await?;
        }

        // Phase 6: Finalize Resharing (only coordinator)
        if participants.first() == Some(&DeviceId(self.ctx.device_id)) {
            let session_id = self.ctx.session_id;
            EventBuilder::new(self.ctx)
                .with_type(EventType::FinalizeResharing(FinalizeResharingEvent {
                    session_id,
                    new_group_public_key: vec![0u8; 32], // Placeholder
                    new_threshold: final_new_threshold,
                    test_signature: vec![0u8; 64], // Placeholder
                }))
                .with_threshold_auth()
                .build_sign_and_emit()
                .await?;
        }

        // Wait for finalization
        let session_id = self.ctx.session_id;
        EventAwaiter::new(self.ctx)
            .for_session(session_id)
            .for_event_types(vec![EventTypePattern::FinalizeResharing])
            .from_authors(participants.first().cloned().into_iter())
            .await_single(100)
            .await?;

        Ok(b"resharing_complete".to_vec())
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<Vec<u8>, ProtocolError> {
        EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::FinalizeResharing])
            .await_single(100) // Default TTL epochs
            .await?;

        Ok(b"resharing_complete".to_vec())
    }
}

/// Resharing Protocol Choreography - Main entry point
pub async fn resharing_choreography(
    ctx: &mut ProtocolContext,
    new_threshold: Option<u16>,
    new_participants: Option<Vec<DeviceId>>,
) -> Result<Vec<u8>, ProtocolError> {
    let mut protocol = ResharingProtocol::new(ctx, new_threshold, new_participants);
    protocol.execute().await
}

impl<'a> ResharingProtocol<'a> {
    /// Distribute sub-shares to new participants
    async fn distribute_sub_shares(
        &mut self,
        new_participants: &[DeviceId],
        new_threshold: u16,
    ) -> Result<(), ProtocolError> {
        // Get current key share and generate polynomial
        let key_share_bytes = self.ctx.get_key_share().await?;
        let key_share_scalar = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(
            key_share_bytes.try_into().unwrap_or([0u8; 32]),
        );
        let polynomial = ShamirPolynomial::from_secret(
            key_share_scalar,
            new_threshold.into(),
            &mut self.ctx.effects.rng(),
        );

        // Distribute sub-shares to each new participant
        for (i, new_participant) in new_participants.iter().enumerate() {
            let x = curve25519_dalek::scalar::Scalar::from((i + 1) as u64);
            let sub_share_scalar = polynomial.evaluate(x);
            let sub_share = sub_share_scalar.to_bytes().to_vec();

            // Encrypt the sub-share using HPKE
            let recipient_public_key = self.ctx.get_device_public_key(new_participant).await?;
            let hpke_public_key = aura_crypto::HpkePublicKey::from_bytes(&recipient_public_key)?;

            let ciphertext = aura_crypto::encrypt_base(
                &sub_share,
                &hpke_public_key,
                &mut self.ctx.effects.rng(),
            )?;
            let encrypted_sub_share = ciphertext.to_bytes();

            let session_id = self.ctx.session_id;
            let device_id = self.ctx.device_id;
            EventBuilder::new(self.ctx)
                .with_type(EventType::DistributeSubShare(DistributeSubShareEvent {
                    session_id,
                    from_device_id: DeviceId(device_id),
                    to_device_id: *new_participant,
                    encrypted_sub_share,
                }))
                .with_device_auth()
                .build_sign_and_emit()
                .await?;
        }

        Ok(())
    }

    /// Collect sub-shares from old participants
    async fn collect_sub_shares(
        &mut self,
        _new_participants: &[DeviceId],
        new_threshold: u16,
    ) -> Result<BTreeMap<DeviceId, Vec<u8>>, ProtocolError> {
        let mut collected_sub_shares = BTreeMap::new();

        // Collect sub-shares from threshold old participants
        for _ in 0..new_threshold {
            let session_id = self.ctx.session_id;
            let event = EventAwaiter::new(self.ctx)
                .for_session(session_id)
                .for_event_types(vec![EventTypePattern::DistributeSubShare])
                .await_single(200)
                .await?;

            if let EventType::DistributeSubShare(ref distribute) = event.event_type {
                if distribute.to_device_id == DeviceId(self.ctx.device_id) {
                    // Decrypt the sub-share
                    let hpke_ciphertext =
                        aura_crypto::HpkeCiphertext::from_bytes(&distribute.encrypted_sub_share)?;
                    let device_private_key = self.ctx.get_device_hpke_private_key().await?;

                    let decrypted =
                        aura_crypto::decrypt_base(&hpke_ciphertext, &device_private_key)?;
                    collected_sub_shares.insert(distribute.from_device_id, decrypted);

                    // Send acknowledgment
                    let session_id = self.ctx.session_id;
                    let device_id = self.ctx.device_id;
                    EventBuilder::new(self.ctx)
                        .with_type(EventType::AcknowledgeSubShare(AcknowledgeSubShareEvent {
                            session_id,
                            from_device_id: distribute.from_device_id,
                            to_device_id: DeviceId(device_id),
                            ack_signature: vec![0u8; 64], // Placeholder
                        }))
                        .with_device_auth()
                        .build_sign_and_emit()
                        .await?;
                }
            }
        }

        Ok(collected_sub_shares)
    }

    /// Reconstruct share from collected sub-shares
    async fn reconstruct_share(
        &mut self,
        collected_sub_shares: &BTreeMap<DeviceId, Vec<u8>>,
    ) -> Result<(), ProtocolError> {
        let share_points: Vec<SharePoint> = collected_sub_shares
            .iter()
            .enumerate()
            .map(|(i, (_device_id, share_bytes))| {
                let scalar = if share_bytes.len() >= 32 {
                    let mut bytes = [0u8; 32];
                    bytes.copy_from_slice(&share_bytes[..32]);
                    curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes)
                } else {
                    curve25519_dalek::scalar::Scalar::from_bytes_mod_order([0u8; 32])
                };
                SharePoint {
                    x: curve25519_dalek::scalar::Scalar::from((i + 1) as u64),
                    y: scalar,
                }
            })
            .collect();

        let reconstructed_scalar = LagrangeInterpolation::interpolate_at_zero(&share_points)?;
        let reconstructed_share = reconstructed_scalar.to_bytes().to_vec();

        // Store new share
        self.ctx.set_key_share(reconstructed_share).await?;
        Ok(())
    }

    /// Verify new shares work correctly
    async fn verify_new_shares(&mut self) -> Result<(), ProtocolError> {
        // TODO: Implement full FROST signing protocol
        // For now, we verify that we have a valid key share
        let key_share = self.ctx.get_key_share().await?;
        if key_share.len() != 32 {
            return Err(ProtocolError {
                session_id: self.ctx.session_id,
                error_type: ProtocolErrorType::InvalidState,
                message: "Invalid key share length after resharing".to_string(),
            });
        }
        Ok(())
    }
}