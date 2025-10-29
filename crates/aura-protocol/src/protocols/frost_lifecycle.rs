//! FROST protocol lifecycle with real distributed threshold signing
//!
//! Implements FROST threshold signatures using a multi-round distributed protocol:
//! 1. Round 1: All participants generate and broadcast commitments
//! 2. Round 2: After collecting threshold commitments, participants create and broadcast signature shares
//! 3. Aggregation: Any participant with threshold shares can aggregate into final signature

use crate::core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolStep},
    metadata::{OperationType, ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use aura_crypto::frost::FrostKeyShare;
use aura_messages::FrostSigningResult;
use aura_types::{AuraError, DeviceId, SessionId};
use frost_ed25519 as frost;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Message types for FROST protocol
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
enum FrostMessageType {
    /// Round 1: Broadcast signing commitment
    Commitment { commitment_bytes: Vec<u8> },
    /// Round 2: Broadcast signature share
    SignatureShare { share_bytes: Vec<u8> },
}

/// FROST signing protocol state machine states
#[derive(Debug, Clone)]
enum FrostSigningState {
    Init,
    /// Waiting for coordinator to start the protocol
    AwaitingStart,
    /// Generating commitment (Round 1)
    GeneratingCommitment {
        message: Vec<u8>,
    },
    /// Collecting commitments from other participants (Round 1)
    AwaitingCommitments {
        message: Vec<u8>,
        own_nonces: frost::round1::SigningNonces,
        own_id: frost::Identifier,
        received_commitments: BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
    },
    /// Creating signature share (Round 2)
    CreatingShare {
        message: Vec<u8>,
        nonces: frost::round1::SigningNonces,
        commitments: BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
    },
    /// Collecting signature shares (Round 2)
    AwaitingShares {
        message: Vec<u8>,
        commitments: BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        received_shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
    },
    /// Aggregating shares into final signature
    Aggregating {
        message: Vec<u8>,
        commitments: BTreeMap<frost::Identifier, frost::round1::SigningCommitments>,
        shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare>,
    },
    Complete(FrostSigningResult),
    Failed(AuraError),
}

/// Typestate marker for the FROST signing lifecycle
#[derive(Debug, Clone)]
pub struct FrostSigningLifecycleState;

impl SessionState for FrostSigningLifecycleState {
    const NAME: &'static str = "FrostSigningLifecycle";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

pub struct FrostSigningLifecycle {
    descriptor: ProtocolDescriptor,
    state: FrostSigningState,
    key_share: FrostKeyShare,
    key_package: frost::keys::KeyPackage,
    pubkey_package: frost::keys::PublicKeyPackage,
    participants: Vec<DeviceId>,
    threshold: u16,
    is_coordinator: bool,
    /// Mapping from DeviceId to frost::Identifier
    device_to_frost_id: BTreeMap<DeviceId, frost::Identifier>,
}

impl FrostSigningLifecycle {
    pub fn new(
        device_id: DeviceId,
        session_id: SessionId,
        participants: Vec<DeviceId>,
        key_share: FrostKeyShare,
        key_package: frost::keys::KeyPackage,
        pubkey_package: frost::keys::PublicKeyPackage,
        threshold: u16,
    ) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::FrostSigning,
        )
        .with_operation_type(OperationType::Signing)
        .with_priority(ProtocolPriority::High)
        .with_mode(ProtocolMode::Interactive);

        // Coordinator is the first participant
        let is_coordinator = participants.first() == Some(&device_id);

        // Create deterministic mapping from DeviceId to frost::Identifier
        // Use participant index + 1 as the identifier value
        let mut device_to_frost_id = BTreeMap::new();
        for (idx, dev_id) in participants.iter().enumerate() {
            // FROST identifiers must be non-zero
            if let Ok(frost_id) = frost::Identifier::try_from((idx + 1) as u16) {
                device_to_frost_id.insert(*dev_id, frost_id);
            }
        }

        Self {
            descriptor,
            state: if is_coordinator {
                FrostSigningState::Init
            } else {
                FrostSigningState::AwaitingStart
            },
            key_share,
            key_package,
            pubkey_package,
            participants,
            threshold,
            is_coordinator,
            device_to_frost_id,
        }
    }

    /// Get frost identifier for a device
    fn frost_id_for_device(&self, device_id: &DeviceId) -> Result<frost::Identifier, AuraError> {
        self.device_to_frost_id
            .get(device_id)
            .copied()
            .ok_or_else(|| AuraError::coordination_failed("Unknown device in participant list"))
    }
}

impl ProtocolLifecycle for FrostSigningLifecycle {
    type State = FrostSigningLifecycleState;
    type Output = FrostSigningResult;
    type Error = AuraError;

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match (&self.state, input) {
            // ========== Initialization ==========

            // Coordinator: Start signal initiates protocol with message
            (FrostSigningState::Init, ProtocolInput::LocalSignal { signal, data })
                if signal == "start" && self.is_coordinator =>
            {
                // Extract message to sign from data
                let message = match data {
                    Some(serde_json::Value::String(s)) => s.as_bytes().to_vec(),
                    Some(serde_json::Value::Array(arr)) => {
                        // Try to deserialize as Vec<u8>
                        arr.iter()
                            .filter_map(|v| v.as_u64().map(|n| n as u8))
                            .collect()
                    }
                    _ => return ProtocolStep::completed(
                        vec![],
                        None,
                        Err(AuraError::coordination_failed("Invalid message format for FROST signing")),
                    ),
                };

                // Coordinator broadcasts start message to all participants
                let start_payload = serde_json::to_vec(&message).unwrap_or_default();
                let mut effects = Vec::new();

                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = crate::core::capabilities::ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: start_payload.clone(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                // Coordinator transitions directly to generating commitment
                self.state = FrostSigningState::GeneratingCommitment { message };

                ProtocolStep::progress(effects, None)
            }

            // Non-coordinators: Wait for start message
            (FrostSigningState::AwaitingStart, ProtocolInput::Message(msg))
                if !self.is_coordinator =>
            {
                // Deserialize message
                let message: Vec<u8> = serde_json::from_slice(&msg.payload)
                    .unwrap_or_else(|_| msg.payload.clone());

                self.state = FrostSigningState::GeneratingCommitment { message };
                ProtocolStep::progress(vec![], None)
            }

            // ========== Round 1: Commitment Generation ==========

            (FrostSigningState::GeneratingCommitment { message }, _) => {
                // Generate nonces and commitments using key package
                let mut rng = rand::rngs::OsRng;
                let (nonces, commitments) =
                    frost::round1::commit(self.key_package.signing_share(), &mut rng);

                // Serialize commitment for transmission
                let commitment_bytes = match commitments.serialize() {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        return ProtocolStep::completed(
                            vec![],
                            None,
                            Err(AuraError::coordination_failed(&format!(
                                "Failed to serialize commitment: {:?}",
                                e
                            ))),
                        )
                    }
                };
                let msg_type = FrostMessageType::Commitment { commitment_bytes };
                let payload = serde_json::to_vec(&msg_type).unwrap_or_default();

                // Broadcast commitments to all participants
                let mut effects = Vec::new();
                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = crate::core::capabilities::ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: payload.clone(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                // Store own commitment
                let own_id = self.key_share.identifier;
                let mut received = BTreeMap::new();
                received.insert(own_id, commitments);

                self.state = FrostSigningState::AwaitingCommitments {
                    message: message.clone(),
                    own_nonces: nonces,
                    own_id,
                    received_commitments: received,
                };

                ProtocolStep::progress(effects, None)
            }

            // ========== Round 1: Collecting Commitments ==========

            (
                FrostSigningState::AwaitingCommitments {
                    message,
                    own_nonces,
                    own_id,
                    received_commitments,
                },
                ProtocolInput::Message(msg),
            ) => {
                // Deserialize message
                let msg_type: FrostMessageType = match serde_json::from_slice(&msg.payload) {
                    Ok(t) => t,
                    Err(_) => return ProtocolStep::progress(vec![], None), // Ignore invalid messages
                };

                if let FrostMessageType::Commitment { commitment_bytes } = msg_type {
                    // Deserialize commitment
                    let commitment_array: [u8; 64] = match commitment_bytes.as_slice().try_into() {
                        Ok(arr) => arr,
                        Err(_) => {
                            return ProtocolStep::completed(
                                vec![],
                                None,
                                Err(AuraError::coordination_failed("Invalid commitment size")),
                            )
                        }
                    };

                    let commitment =
                        match frost::round1::SigningCommitments::deserialize(&commitment_array) {
                            Ok(c) => c,
                            Err(e) => {
                                return ProtocolStep::completed(
                                    vec![],
                                    None,
                                    Err(AuraError::coordination_failed(&format!(
                                        "Failed to deserialize commitment: {:?}",
                                        e
                                    ))),
                                )
                            }
                        };

                    // Get sender's frost identifier
                    let sender_id = match self.frost_id_for_device(&msg.from) {
                        Ok(id) => id,
                        Err(e) => return ProtocolStep::completed(vec![], None, Err(e)),
                    };

                    let mut commitments = received_commitments.clone();
                    commitments.insert(sender_id, commitment);

                    // Check if we have commitments from threshold participants
                    if commitments.len() >= self.threshold as usize {
                        // Move to creating shares (Round 2)
                        self.state = FrostSigningState::CreatingShare {
                            message: message.clone(),
                            nonces: own_nonces.clone(),
                            commitments,
                        };
                        ProtocolStep::progress(vec![], None)
                    } else {
                        // Still waiting for more commitments
                        self.state = FrostSigningState::AwaitingCommitments {
                            message: message.clone(),
                            own_nonces: own_nonces.clone(),
                            own_id: *own_id,
                            received_commitments: commitments,
                        };
                        ProtocolStep::progress(vec![], None)
                    }
                } else {
                    // Wrong message type, ignore
                    ProtocolStep::progress(vec![], None)
                }
            }

            // ========== Round 2: Signature Share Creation ==========

            (
                FrostSigningState::CreatingShare {
                    message,
                    nonces,
                    commitments,
                },
                _,
            ) => {
                // Create signing package
                let signing_package =
                    frost::SigningPackage::new(commitments.clone(), message.as_slice());

                // Generate signature share
                let signature_share =
                    match frost::round2::sign(&signing_package, nonces, &self.key_package) {
                        Ok(share) => share,
                        Err(e) => {
                            return ProtocolStep::completed(
                                vec![],
                                None,
                                Err(AuraError::coordination_failed(&format!(
                                    "Failed to create signature share: {:?}",
                                    e
                                ))),
                            )
                        }
                    };

                // Serialize share for transmission (returns [u8; 32] directly)
                let share_bytes = signature_share.serialize();
                let msg_type = FrostMessageType::SignatureShare {
                    share_bytes: share_bytes.to_vec()
                };
                let payload = serde_json::to_vec(&msg_type).unwrap_or_default();

                // Broadcast shares to all participants
                let mut effects = Vec::new();
                for participant in &self.participants {
                    if *participant != self.descriptor.device_id {
                        let message = crate::core::capabilities::ProtocolMessage {
                            from: self.descriptor.device_id,
                            to: *participant,
                            payload: payload.clone(),
                            session_id: Some(self.descriptor.session_id.uuid()),
                        };
                        effects.push(ProtocolEffects::Send { message });
                    }
                }

                // Store own share
                let own_id = self.key_share.identifier;
                let mut shares = BTreeMap::new();
                shares.insert(own_id, signature_share);

                self.state = FrostSigningState::AwaitingShares {
                    message: message.clone(),
                    commitments: commitments.clone(),
                    received_shares: shares,
                };

                ProtocolStep::progress(effects, None)
            }

            // ========== Round 2: Collecting Signature Shares ==========

            (
                FrostSigningState::AwaitingShares {
                    message,
                    commitments,
                    received_shares,
                },
                ProtocolInput::Message(msg),
            ) => {
                // Deserialize message
                let msg_type: FrostMessageType = match serde_json::from_slice(&msg.payload) {
                    Ok(t) => t,
                    Err(_) => return ProtocolStep::progress(vec![], None), // Ignore invalid messages
                };

                if let FrostMessageType::SignatureShare { share_bytes } = msg_type {
                    // Deserialize signature share
                    let share_array: [u8; 32] = match share_bytes.as_slice().try_into() {
                        Ok(arr) => arr,
                        Err(_) => {
                            return ProtocolStep::completed(
                                vec![],
                                None,
                                Err(AuraError::coordination_failed("Invalid signature share size")),
                            )
                        }
                    };

                    let signature_share =
                        match frost::round2::SignatureShare::deserialize(share_array) {
                            Ok(s) => s,
                            Err(e) => {
                                return ProtocolStep::completed(
                                    vec![],
                                    None,
                                    Err(AuraError::coordination_failed(&format!(
                                        "Failed to deserialize signature share: {:?}",
                                        e
                                    ))),
                                )
                            }
                        };

                    // Get sender's frost identifier
                    let sender_id = match self.frost_id_for_device(&msg.from) {
                        Ok(id) => id,
                        Err(e) => return ProtocolStep::completed(vec![], None, Err(e)),
                    };

                    let mut shares = received_shares.clone();
                    shares.insert(sender_id, signature_share);

                    // Check if we have shares from threshold participants
                    if shares.len() >= self.threshold as usize {
                        // Move to aggregation
                        self.state = FrostSigningState::Aggregating {
                            message: message.clone(),
                            commitments: commitments.clone(),
                            shares,
                        };
                        ProtocolStep::progress(vec![], None)
                    } else {
                        // Still waiting for more shares
                        self.state = FrostSigningState::AwaitingShares {
                            message: message.clone(),
                            commitments: commitments.clone(),
                            received_shares: shares,
                        };
                        ProtocolStep::progress(vec![], None)
                    }
                } else {
                    // Wrong message type, ignore
                    ProtocolStep::progress(vec![], None)
                }
            }

            // ========== Aggregation: Final Signature ==========

            (
                FrostSigningState::Aggregating {
                    message,
                    commitments,
                    shares,
                },
                _,
            ) => {
                // Create signing package
                let signing_package =
                    frost::SigningPackage::new(commitments.clone(), message.as_slice());

                // Aggregate signature shares
                let group_signature = match frost::aggregate(&signing_package, shares, &self.pubkey_package) {
                    Ok(sig) => sig,
                    Err(e) => {
                        return ProtocolStep::completed(
                            vec![],
                            None,
                            Err(AuraError::coordination_failed(&format!(
                                "Failed to aggregate signature: {:?}",
                                e
                            ))),
                        )
                    }
                };

                // Convert to bytes
                let sig_bytes = group_signature.serialize();

                // Create result
                let result = FrostSigningResult {
                    session_id: self.descriptor.session_id,
                    signature: sig_bytes.to_vec(),
                    message: message.clone(),
                    signing_participants: self.participants.clone(),
                    verification: aura_messages::FrostSignatureVerification {
                        is_valid: true,
                        verification_details: vec![],
                        group_verification: true,
                    },
                };

                self.state = FrostSigningState::Complete(result.clone());

                ProtocolStep::completed(vec![], None, Ok(result))
            }

            // ========== Terminal States ==========

            // Already complete
            (FrostSigningState::Complete(result), _) => {
                ProtocolStep::completed(vec![], None, Ok(result.clone()))
            }

            // Failed state
            (FrostSigningState::Failed(err), _) => {
                ProtocolStep::completed(vec![], None, Err(err.clone()))
            }

            // ========== Unexpected Input ==========

            // Ignore unexpected messages/signals
            _ => ProtocolStep::progress(vec![], None),
        }
    }

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn is_final(&self) -> bool {
        matches!(
            self.state,
            FrostSigningState::Complete(_) | FrostSigningState::Failed(_)
        )
    }
}

// ========== Error Type ==========
pub type FrostLifecycleError = AuraError;
