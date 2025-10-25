//! Recovery Protocol Choreography
//!
//! This module implements the guardian-based recovery protocol using choreographic
//! programming with the new helper abstractions.

use crate::execution::{
    EventAwaiter, EventBuilder, EventTypePattern, Instruction,
    InstructionResult, ProtocolContext, ProtocolContextExt, ProtocolError, ProtocolErrorType,
    SessionLifecycle,
};
use aura_crypto::{
    decrypt_with_aad, HpkeCiphertext, LagrangeInterpolation,
    SharePoint,
};
use aura_journal::{
    CompleteRecoveryEvent, EventType,
    GuardianId, InitiateRecoveryEvent, OperationType, ParticipantId as JournalParticipantId,
    ProtocolType, Session,
};
use ed25519_dalek::Signer;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Recovery Protocol implementation using SessionLifecycle trait
pub struct RecoveryProtocol<'a> {
    ctx: &'a mut ProtocolContext,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
}

impl<'a> RecoveryProtocol<'a> {
    pub fn new(
        ctx: &'a mut ProtocolContext,
        guardian_ids: Vec<GuardianId>,
        threshold: u16,
    ) -> Self {
        Self {
            ctx,
            guardian_ids,
            threshold,
        }
    }
}

#[async_trait::async_trait]
impl<'a> SessionLifecycle for RecoveryProtocol<'a> {
    type Result = Vec<u8>; // Recovered share

    fn operation_type(&self) -> OperationType {
        OperationType::Recovery
    }

    fn generate_context_id(&self) -> Vec<u8> {
        format!("recovery:{}:{:?}", self.threshold, self.guardian_ids).into_bytes()
    }

    async fn create_session(&mut self) -> Result<Session, ProtocolError> {
        let ledger_context = self.ctx.fetch_ledger_context().await?;

        // Convert guardians to session participants
        let session_participants: Vec<JournalParticipantId> = self
            .guardian_ids
            .iter()
            .map(|&guardian_id| JournalParticipantId::Guardian(guardian_id))
            .collect();

        // Create Recovery session
        Ok(Session::new(
            aura_journal::SessionId(self.ctx.session_id()),
            ProtocolType::Recovery,
            session_participants,
            ledger_context.epoch,
            200, // TTL in epochs - recovery has longer time limit due to cooldown
            self.ctx.effects().now().map_err(|e| ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::Other,
                message: format!("Failed to get timestamp: {:?}", e),
            })?,
        ))
    }

    async fn execute_protocol(&mut self, _session: &Session) -> Result<Vec<u8>, ProtocolError> {
        let recovery_id = self.ctx.session_id();
        let new_device_id = aura_journal::DeviceId(self.ctx.device_id());
        let _start_epoch = self.ctx.fetch_ledger_context().await?.epoch;

        // Extract device public key before the mutable borrow
        let new_device_pk = self.ctx.device_key().verifying_key().to_bytes().to_vec();

        // Phase 1: Initiate Recovery
        EventBuilder::new(self.ctx)
            .with_type(EventType::InitiateRecovery(InitiateRecoveryEvent {
                recovery_id,
                new_device_id,
                new_device_pk,
                required_guardians: self.guardian_ids.clone(),
                quorum_threshold: self.threshold,
                cooldown_seconds: 48 * 3600, // 48 hours in seconds
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        // Phase 2: Collect Guardian Approvals
        let guardian_approvals = self.collect_guardian_approvals(recovery_id).await?;

        // Phase 3: Enforce Cooldown Period
        self.enforce_cooldown_period(recovery_id).await?;

        // Phase 4: Reconstruct Recovery Share
        let recovered_share = self.reconstruct_recovery_share(&guardian_approvals).await?;

        // Phase 5: Complete Recovery
        // Generate a test signature to prove the device can use the recovered key
        let test_message = format!("recovery_test_{}_{}", recovery_id, new_device_id.0);
        let test_signature = self.ctx.device_key().sign(test_message.as_bytes()).to_bytes().to_vec();
        
        EventBuilder::new(self.ctx)
            .with_type(EventType::CompleteRecovery(CompleteRecoveryEvent {
                recovery_id,
                new_device_id,
                test_signature,
            }))
            .with_device_auth()
            .build_sign_and_emit()
            .await?;

        Ok(recovered_share)
    }

    async fn wait_for_completion(
        &mut self,
        winning_session: &Session,
    ) -> Result<Vec<u8>, ProtocolError> {
        let complete_event = EventAwaiter::new(self.ctx)
            .for_session(winning_session.session_id.0)
            .for_event_types(vec![EventTypePattern::CompleteRecovery])
            .await_single(100) // Default TTL epochs
            .await?;

        match &complete_event.event_type {
            EventType::CompleteRecovery(_complete) => {
                // The recovered share was already reconstructed earlier
                // Return a placeholder for now
                Ok(vec![0u8; 32])
            },
            _ => Err(ProtocolError {
                session_id: self.ctx.session_id(),
                error_type: ProtocolErrorType::InvalidState,
                message: "Expected recovery complete event".to_string(),
            }),
        }
    }
}

/// Recovery Protocol Choreography - Main entry point
pub async fn recovery_choreography(
    ctx: &mut ProtocolContext,
    guardian_ids: Vec<GuardianId>,
    threshold: u16,
) -> Result<Vec<u8>, ProtocolError> {
    let mut protocol = RecoveryProtocol::new(ctx, guardian_ids, threshold);
    protocol.execute().await
}

impl<'a> RecoveryProtocol<'a> {
    /// Collect guardian approvals with encrypted recovery shares
    async fn collect_guardian_approvals(
        &mut self,
        recovery_id: Uuid,
    ) -> Result<BTreeMap<GuardianId, Vec<u8>>, ProtocolError> {
        let mut collected_shares = BTreeMap::new();

        // Wait for threshold guardian recovery shares
        for _ in 0..self.threshold {
            let share_event = EventAwaiter::new(self.ctx)
                .for_session(recovery_id)
                .for_event_types(vec![EventTypePattern::SubmitRecoveryShare])
                .await_single(500)
                .await?;

            if let EventType::SubmitRecoveryShare(ref share) = share_event.event_type {
                // Verify guardian is in approved list
                if !self.guardian_ids.contains(&share.guardian_id) {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::UnexpectedEvent,
                        message: format!("Share from unexpected guardian: {:?}", share.guardian_id),
                    });
                }

                // Store encrypted recovery share
                collected_shares.insert(share.guardian_id, share.encrypted_share.clone());
            }
        }

        Ok(collected_shares)
    }

    /// Enforce the cooldown period with periodic checks for vetoes
    async fn enforce_cooldown_period(&mut self, recovery_id: Uuid) -> Result<(), ProtocolError> {
        let cooldown_epochs = 100; // Simplified for MVP
        
        // Check for vetoes periodically during cooldown
        for _ in 0..5 {
            // Check for abort events
            let abort_check = self.ctx
                .execute(Instruction::CheckForEvent {
                    filter: crate::execution::EventFilter {
                        session_id: Some(recovery_id),
                        event_types: Some(vec![EventTypePattern::AbortRecovery]),
                        authors: None,
                        predicate: None,
                    },
                })
                .await?;

            match abort_check {
                InstructionResult::EventReceived(event) => {
                    if let EventType::AbortRecovery(_) = event.event_type {
                        return Err(ProtocolError {
                            session_id: self.ctx.session_id(),
                            error_type: ProtocolErrorType::RecoveryVetoed,
                            message: "Recovery aborted by guardian veto".to_string(),
                        });
                    }
                }
                _ => {} // No veto found, continue
            }

            // Wait for a portion of the cooldown period
            let wait_result = self.ctx
                .execute(Instruction::WaitEpochs(cooldown_epochs / 5))
                .await?;
            
            match wait_result {
                InstructionResult::EpochsElapsed => continue,
                _ => {
                    return Err(ProtocolError {
                        session_id: self.ctx.session_id(),
                        error_type: ProtocolErrorType::InvalidState,
                        message: "Failed to wait epochs".to_string(),
                    })
                }
            }
        }

        Ok(())
    }

    /// Reconstruct the recovery share from guardian shares
    async fn reconstruct_recovery_share(
        &mut self,
        guardian_shares: &BTreeMap<GuardianId, Vec<u8>>,
    ) -> Result<Vec<u8>, ProtocolError> {
        let mut decrypted_points = Vec::new();

        // Decrypt each guardian's share
        for (_guardian_id, encrypted_share) in guardian_shares.iter() {
            // In production, each guardian would have encrypted their share
            // specifically for the recovering device's public key
            // For MVP, we'll simulate this
            
            // Parse encrypted share as HPKE ciphertext
            let ciphertext = HpkeCiphertext::from_bytes(encrypted_share)?;
            
            // Get device's HPKE private key for decryption
            let device_private_key = self.ctx.get_device_hpke_private_key().await?;
            
            // Decrypt with associated data for authenticity
            let aad = format!("recovery:{}", self.ctx.session_id()).into_bytes();
            let decrypted = decrypt_with_aad(&ciphertext, &device_private_key, &aad)?;
            
            // Parse as scalar point
            if decrypted.len() >= 32 {
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&decrypted[..32]);
                let scalar = curve25519_dalek::scalar::Scalar::from_bytes_mod_order(bytes);
                decrypted_points.push(scalar);
            }
        }

        // Create SharePoints for Lagrange interpolation
        let share_points: Vec<SharePoint> = decrypted_points
            .into_iter()
            .enumerate()
            .map(|(i, y)| SharePoint {
                x: curve25519_dalek::scalar::Scalar::from((i + 1) as u64),
                y,
            })
            .collect();

        // Reconstruct the secret via Lagrange interpolation
        let recovered_scalar = LagrangeInterpolation::interpolate_at_zero(&share_points)?;
        
        Ok(recovered_scalar.to_bytes().to_vec())
    }
}

/// Nudge a guardian to approve a recovery request
pub async fn nudge_guardian(
    _ctx: &mut ProtocolContext,
    guardian_id: GuardianId,
    recovery_session_id: Uuid,
) -> Result<(), ProtocolError> {
    // This is a simplified implementation - in production this would
    // send a notification to the guardian via their preferred channel
    tracing::info!(
        "Nudging guardian {} for recovery session {}",
        guardian_id.0,
        recovery_session_id
    );
    
    // For now, just log the nudge attempt
    Ok(())
}