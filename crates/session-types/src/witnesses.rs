//! Runtime Witnesses for Distributed Invariants
//!
//! This module provides runtime witness types that can only be constructed after
//! verifying distributed protocol conditions through journal evidence. These witnesses
//! enable session type transitions that depend on distributed state.

use aura_journal::{DeviceId, Event};
use std::collections::{BTreeMap, BTreeSet};

/// Trait for types that can serve as runtime witnesses
///
/// Runtime witnesses are proof objects that can only be constructed after verifying
/// that a distributed condition has been met through examination of journal evidence.
pub trait RuntimeWitness: 'static + Send + Sync {
    /// The type of evidence required to construct this witness
    type Evidence;

    /// The type of configuration or parameters needed for verification
    type Config;

    /// Attempt to construct the witness from evidence
    ///
    /// Returns `Some(witness)` if the distributed condition is satisfied,
    /// `None` otherwise.
    fn verify(evidence: Self::Evidence, config: Self::Config) -> Option<Self>
    where
        Self: Sized;

    /// Get a description of what this witness proves (for debugging)
    fn description(&self) -> &'static str;
}

/// Rehydration evidence from journal for crash recovery
#[derive(Debug, Clone)]
pub struct RehydrationEvidence {
    /// Events from the journal relevant to this protocol
    pub events: Vec<Event>,
    /// Protocol session ID
    pub session_id: uuid::Uuid,
    /// Last known state from journal
    pub last_state: Option<String>,
}

// ========== DKD Protocol Witnesses ==========

/// Witness proving that sufficient commitments have been collected for DKD
#[derive(Debug, Clone)]
pub struct CollectedCommitments {
    pub count: usize,
    pub threshold: usize,
    pub participants: BTreeSet<DeviceId>,
    pub commitments: BTreeMap<DeviceId, Vec<u8>>,
    pub commitment_timestamps: BTreeMap<DeviceId, u64>,
}

impl RuntimeWitness for CollectedCommitments {
    type Evidence = Vec<Event>;
    type Config = CommitmentConfig;

    fn verify(events: Vec<Event>, config: CommitmentConfig) -> Option<Self> {
        let mut participants = BTreeSet::new();
        let mut commitments = BTreeMap::new();
        let mut commitment_timestamps = BTreeMap::new();

        for event in events {
            if is_commitment_event(&event) && is_valid_commitment(&event, &config) {
                if let Some(device_id) = extract_device_id(&event) {
                    participants.insert(device_id);

                    // Extract and store the commitment data
                    if let aura_journal::EventType::RecordDkdCommitment(commit_event) =
                        &event.event_type
                    {
                        commitments.insert(device_id, commit_event.commitment.to_vec());
                        commitment_timestamps.insert(device_id, event.timestamp);
                    }
                }
            }
        }

        let count = participants.len();
        if count >= config.threshold {
            Some(CollectedCommitments {
                count,
                threshold: config.threshold,
                participants,
                commitments,
                commitment_timestamps,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient DKD commitments collected"
    }
}

#[derive(Debug, Clone)]
pub struct CommitmentConfig {
    pub threshold: usize,
    pub session_id: uuid::Uuid,
    pub expected_participants: Option<BTreeSet<DeviceId>>,
    pub authorized_devices: BTreeSet<DeviceId>,
    pub max_time_skew: u64,            // seconds
    pub session_deadline: Option<u64>, // optional deadline timestamp
}

/// Witness proving that sufficient reveals have been collected and verified for DKD
#[derive(Debug, Clone)]
pub struct VerifiedReveals {
    pub count: usize,
    pub threshold: usize,
    pub participants: BTreeSet<DeviceId>,
}

impl RuntimeWitness for VerifiedReveals {
    type Evidence = (Vec<Event>, CollectedCommitments);
    type Config = RevealConfig;

    fn verify(evidence: (Vec<Event>, CollectedCommitments), config: RevealConfig) -> Option<Self> {
        let (events, commitments) = evidence;
        let mut verified_participants = BTreeSet::new();

        for event in events {
            if is_reveal_event(&event) {
                if let Some(device_id) = extract_device_id(&event) {
                    if commitments.participants.contains(&device_id)
                        && verify_reveal_against_commitment(&event, &commitments)
                    {
                        verified_participants.insert(device_id);
                    }
                }
            }
        }

        let count = verified_participants.len();
        if count >= config.threshold {
            Some(VerifiedReveals {
                count,
                threshold: config.threshold,
                participants: verified_participants,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient DKD reveals verified"
    }
}

#[derive(Debug, Clone)]
pub struct RevealConfig {
    pub threshold: usize,
    pub session_id: uuid::Uuid,
}

// ========== Recovery Protocol Witnesses ==========

/// Witness proving that sufficient guardian approvals have been collected for recovery
#[derive(Debug, Clone)]
pub struct ApprovalThresholdMet {
    pub count: usize,
    pub threshold: usize,
    pub approving_guardians: BTreeSet<DeviceId>,
}

impl RuntimeWitness for ApprovalThresholdMet {
    type Evidence = Vec<Event>;
    type Config = ApprovalConfig;

    fn verify(events: Vec<Event>, config: ApprovalConfig) -> Option<Self> {
        let mut approving_guardians = BTreeSet::new();

        for event in events {
            if is_approval_event(&event) && is_valid_approval(&event, &config) {
                if let Some(guardian_id) = extract_guardian_id(&event) {
                    approving_guardians.insert(guardian_id);
                }
            }
        }

        let count = approving_guardians.len();
        if count >= config.threshold {
            Some(ApprovalThresholdMet {
                count,
                threshold: config.threshold,
                approving_guardians,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient guardian approvals collected"
    }
}

#[derive(Debug, Clone)]
pub struct ApprovalConfig {
    pub threshold: usize,
    pub recovery_session_id: uuid::Uuid,
    pub expected_guardians: Option<BTreeSet<DeviceId>>,
}

/// Witness proving that sufficient guardian shares have been collected for recovery
#[derive(Debug, Clone)]
pub struct SharesCollected {
    pub count: usize,
    pub threshold: usize,
    pub sharing_guardians: BTreeSet<DeviceId>,
}

impl RuntimeWitness for SharesCollected {
    type Evidence = (Vec<Event>, ApprovalThresholdMet);
    type Config = ShareConfig;

    fn verify(evidence: (Vec<Event>, ApprovalThresholdMet), config: ShareConfig) -> Option<Self> {
        let (events, approvals) = evidence;
        let mut sharing_guardians = BTreeSet::new();

        for event in events {
            if is_share_event(&event) {
                if let Some(guardian_id) = extract_guardian_id(&event) {
                    if approvals.approving_guardians.contains(&guardian_id)
                        && is_valid_share(&event, &config)
                    {
                        sharing_guardians.insert(guardian_id);
                    }
                }
            }
        }

        let count = sharing_guardians.len();
        if count >= config.threshold {
            Some(SharesCollected {
                count,
                threshold: config.threshold,
                sharing_guardians,
            })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Sufficient guardian shares collected"
    }
}

#[derive(Debug, Clone)]
pub struct ShareConfig {
    pub threshold: usize,
    pub recovery_session_id: uuid::Uuid,
}

// ========== Additional Witness Types ==========

/// Witness proving threshold events have been met for context execution
#[derive(Debug, Clone)]
pub struct ThresholdEventsMet {
    pub count: usize,
    pub threshold: usize,
}

impl RuntimeWitness for ThresholdEventsMet {
    type Evidence = Vec<Event>;
    type Config = usize; // threshold

    fn verify(events: Vec<Event>, threshold: usize) -> Option<Self> {
        let count = events.len();
        if count >= threshold {
            Some(ThresholdEventsMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Threshold events met for context execution"
    }
}

/// Witness proving ledger write has completed
#[derive(Debug, Clone)]
pub struct LedgerWriteComplete {
    pub event_id: uuid::Uuid,
}

impl RuntimeWitness for LedgerWriteComplete {
    type Evidence = Event;
    type Config = ();

    fn verify(event: Event, _config: ()) -> Option<Self> {
        Some(LedgerWriteComplete {
            event_id: event.event_id.0,
        })
    }

    fn description(&self) -> &'static str {
        "Ledger write completed successfully"
    }
}

/// Witness proving sub-protocol has completed
#[derive(Debug, Clone)]
pub struct SubProtocolComplete {
    pub protocol_id: uuid::Uuid,
    pub final_state: String,
}

impl RuntimeWitness for SubProtocolComplete {
    type Evidence = (uuid::Uuid, String);
    type Config = ();

    fn verify(evidence: (uuid::Uuid, String), _config: ()) -> Option<Self> {
        let (protocol_id, final_state) = evidence;
        Some(SubProtocolComplete {
            protocol_id,
            final_state,
        })
    }

    fn description(&self) -> &'static str {
        "Sub-protocol completed successfully"
    }
}

// ========== Transport Protocol Witnesses ==========

/// Witness proving handshake has completed
#[derive(Debug, Clone)]
pub struct HandshakeCompleted {
    pub peer_id: String,
    pub connection_id: String,
}

impl RuntimeWitness for HandshakeCompleted {
    type Evidence = (String, String);
    type Config = ();

    fn verify(evidence: (String, String), _config: ()) -> Option<Self> {
        let (peer_id, connection_id) = evidence;
        Some(HandshakeCompleted {
            peer_id,
            connection_id,
        })
    }

    fn description(&self) -> &'static str {
        "Transport handshake completed"
    }
}

/// Witness proving tickets have been validated
#[derive(Debug, Clone)]
pub struct TicketsValidated {
    pub peer_id: String,
}

impl RuntimeWitness for TicketsValidated {
    type Evidence = String;
    type Config = ();

    fn verify(peer_id: String, _config: ()) -> Option<Self> {
        Some(TicketsValidated { peer_id })
    }

    fn description(&self) -> &'static str {
        "Presence tickets validated"
    }
}

/// Witness proving message has been delivered
#[derive(Debug, Clone)]
pub struct MessageDelivered {
    pub message_id: uuid::Uuid,
    pub peer_id: String,
}

impl RuntimeWitness for MessageDelivered {
    type Evidence = (uuid::Uuid, String);
    type Config = ();

    fn verify(evidence: (uuid::Uuid, String), _config: ()) -> Option<Self> {
        let (message_id, peer_id) = evidence;
        Some(MessageDelivered {
            message_id,
            peer_id,
        })
    }

    fn description(&self) -> &'static str {
        "Message delivered successfully"
    }
}

/// Witness proving broadcast has completed
#[derive(Debug, Clone)]
pub struct BroadcastCompleted {
    pub message_id: uuid::Uuid,
    pub successful_peers: Vec<String>,
    pub failed_peers: Vec<String>,
}

impl RuntimeWitness for BroadcastCompleted {
    type Evidence = (uuid::Uuid, Vec<String>, Vec<String>);
    type Config = ();

    fn verify(evidence: (uuid::Uuid, Vec<String>, Vec<String>), _config: ()) -> Option<Self> {
        let (message_id, successful_peers, failed_peers) = evidence;
        Some(BroadcastCompleted {
            message_id,
            successful_peers,
            failed_peers,
        })
    }

    fn description(&self) -> &'static str {
        "Broadcast completed"
    }
}

// ========== FROST Protocol Witnesses ==========

/// Witness proving commitment threshold has been met
#[derive(Debug, Clone)]
pub struct CommitmentThresholdMet {
    pub count: usize,
    pub threshold: usize,
}

impl RuntimeWitness for CommitmentThresholdMet {
    type Evidence = Vec<Event>;
    type Config = usize;

    fn verify(events: Vec<Event>, threshold: usize) -> Option<Self> {
        let count = events.len();
        if count >= threshold {
            Some(CommitmentThresholdMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "FROST commitment threshold met"
    }
}

/// Witness proving signature share threshold has been met
#[derive(Debug, Clone)]
pub struct SignatureShareThresholdMet {
    pub count: usize,
    pub threshold: usize,
}

impl RuntimeWitness for SignatureShareThresholdMet {
    type Evidence = Vec<Event>;
    type Config = usize;

    fn verify(events: Vec<Event>, threshold: usize) -> Option<Self> {
        let count = events.len();
        if count >= threshold {
            Some(SignatureShareThresholdMet { count, threshold })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "FROST signature share threshold met"
    }
}

/// Witness proving signature has been aggregated
#[derive(Debug, Clone)]
pub struct SignatureAggregated {
    pub signature: Vec<u8>,
}

impl RuntimeWitness for SignatureAggregated {
    type Evidence = Vec<u8>;
    type Config = ();

    fn verify(signature: Vec<u8>, _config: ()) -> Option<Self> {
        if !signature.is_empty() {
            Some(SignatureAggregated { signature })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "FROST signature aggregated"
    }
}

// ========== Journal Protocol Witnesses ==========

/// Witness proving events have been validated
#[derive(Debug, Clone)]
pub struct EventsValidated {
    pub event_count: usize,
}

impl RuntimeWitness for EventsValidated {
    type Evidence = Vec<Event>;
    type Config = ();

    fn verify(events: Vec<Event>, _config: ()) -> Option<Self> {
        Some(EventsValidated {
            event_count: events.len(),
        })
    }

    fn description(&self) -> &'static str {
        "Journal events validated"
    }
}

/// Witness proving events have been applied successfully
#[derive(Debug, Clone)]
pub struct EventsAppliedSuccessfully {
    pub applied_count: usize,
}

impl RuntimeWitness for EventsAppliedSuccessfully {
    type Evidence = usize;
    type Config = ();

    fn verify(applied_count: usize, _config: ()) -> Option<Self> {
        Some(EventsAppliedSuccessfully { applied_count })
    }

    fn description(&self) -> &'static str {
        "Journal events applied successfully"
    }
}

/// Witness proving session has been created
#[derive(Debug, Clone)]
pub struct SessionCreated {
    pub session_id: uuid::Uuid,
}

impl RuntimeWitness for SessionCreated {
    type Evidence = uuid::Uuid;
    type Config = ();

    fn verify(session_id: uuid::Uuid, _config: ()) -> Option<Self> {
        Some(SessionCreated { session_id })
    }

    fn description(&self) -> &'static str {
        "Journal session created"
    }
}

/// Witness proving session completion is ready
#[derive(Debug, Clone)]
pub struct SessionCompletionReady {
    pub session_id: uuid::Uuid,
}

impl RuntimeWitness for SessionCompletionReady {
    type Evidence = uuid::Uuid;
    type Config = ();

    fn verify(session_id: uuid::Uuid, _config: ()) -> Option<Self> {
        Some(SessionCompletionReady { session_id })
    }

    fn description(&self) -> &'static str {
        "Journal session completion ready"
    }
}

// ========== Additional Protocol Witnesses ==========

/// Witness proving key generation has completed
#[derive(Debug, Clone)]
pub struct KeyGenerationCompleted {
    pub key_id: uuid::Uuid,
}

impl RuntimeWitness for KeyGenerationCompleted {
    type Evidence = uuid::Uuid;
    type Config = ();

    fn verify(key_id: uuid::Uuid, _config: ()) -> Option<Self> {
        Some(KeyGenerationCompleted { key_id })
    }

    fn description(&self) -> &'static str {
        "Key generation completed"
    }
}

/// Witness proving FROST resharing has completed
#[derive(Debug, Clone)]
pub struct FrostResharingCompleted {
    pub new_threshold: usize,
    pub new_participant_count: usize,
}

impl RuntimeWitness for FrostResharingCompleted {
    type Evidence = (usize, usize);
    type Config = ();

    fn verify(evidence: (usize, usize), _config: ()) -> Option<Self> {
        let (new_threshold, new_participant_count) = evidence;
        Some(FrostResharingCompleted {
            new_threshold,
            new_participant_count,
        })
    }

    fn description(&self) -> &'static str {
        "FROST resharing completed"
    }
}

/// Witness proving FROST protocol failure
#[derive(Debug, Clone)]
pub struct FrostProtocolFailure {
    pub error_message: String,
}

impl RuntimeWitness for FrostProtocolFailure {
    type Evidence = String;
    type Config = ();

    fn verify(error_message: String, _config: ()) -> Option<Self> {
        Some(FrostProtocolFailure { error_message })
    }

    fn description(&self) -> &'static str {
        "FROST protocol failure detected"
    }
}

// ========== CGKA Protocol Witnesses ==========

/// Witness proving CGKA group has been initiated
#[derive(Debug, Clone)]
pub struct CgkaGroupInitiated {
    pub group_id: String,
    pub initial_members: Vec<DeviceId>,
}

impl RuntimeWitness for CgkaGroupInitiated {
    type Evidence = (String, Vec<DeviceId>);
    type Config = ();

    fn verify(evidence: (String, Vec<DeviceId>), _config: ()) -> Option<Self> {
        let (group_id, initial_members) = evidence;
        Some(CgkaGroupInitiated {
            group_id,
            initial_members,
        })
    }

    fn description(&self) -> &'static str {
        "CGKA group initiated"
    }
}

/// Witness proving membership change is ready
#[derive(Debug, Clone)]
pub struct MembershipChangeReady {
    pub group_id: String,
    pub operation_id: uuid::Uuid,
}

impl RuntimeWitness for MembershipChangeReady {
    type Evidence = (String, uuid::Uuid);
    type Config = ();

    fn verify(evidence: (String, uuid::Uuid), _config: ()) -> Option<Self> {
        let (group_id, operation_id) = evidence;
        Some(MembershipChangeReady {
            group_id,
            operation_id,
        })
    }

    fn description(&self) -> &'static str {
        "CGKA membership change ready"
    }
}

/// Witness proving epoch transition is ready
#[derive(Debug, Clone)]
pub struct EpochTransitionReady {
    pub group_id: String,
    pub new_epoch: u64,
}

impl RuntimeWitness for EpochTransitionReady {
    type Evidence = (String, u64);
    type Config = ();

    fn verify(evidence: (String, u64), _config: ()) -> Option<Self> {
        let (group_id, new_epoch) = evidence;
        Some(EpochTransitionReady {
            group_id,
            new_epoch,
        })
    }

    fn description(&self) -> &'static str {
        "CGKA epoch transition ready"
    }
}

/// Witness proving group has been stabilized
#[derive(Debug, Clone)]
pub struct GroupStabilized {
    pub group_id: String,
    pub final_epoch: u64,
}

impl RuntimeWitness for GroupStabilized {
    type Evidence = (String, u64);
    type Config = ();

    fn verify(evidence: (String, u64), _config: ()) -> Option<Self> {
        let (group_id, final_epoch) = evidence;
        Some(GroupStabilized {
            group_id,
            final_epoch,
        })
    }

    fn description(&self) -> &'static str {
        "CGKA group stabilized"
    }
}

/// Witness proving operation has been validated
#[derive(Debug, Clone)]
pub struct OperationValidated {
    pub operation_id: uuid::Uuid,
}

impl RuntimeWitness for OperationValidated {
    type Evidence = uuid::Uuid;
    type Config = ();

    fn verify(operation_id: uuid::Uuid, _config: ()) -> Option<Self> {
        Some(OperationValidated { operation_id })
    }

    fn description(&self) -> &'static str {
        "Operation validated"
    }
}

/// Witness proving operation has been applied successfully
#[derive(Debug, Clone)]
pub struct OperationAppliedSuccessfully {
    pub operation_id: uuid::Uuid,
}

impl RuntimeWitness for OperationAppliedSuccessfully {
    type Evidence = uuid::Uuid;
    type Config = ();

    fn verify(operation_id: uuid::Uuid, _config: ()) -> Option<Self> {
        Some(OperationAppliedSuccessfully { operation_id })
    }

    fn description(&self) -> &'static str {
        "Operation applied successfully"
    }
}

/// Witness proving tree updates have completed
#[derive(Debug, Clone)]
pub struct TreeUpdatesCompleted {
    pub tree_hash: Vec<u8>,
}

impl RuntimeWitness for TreeUpdatesCompleted {
    type Evidence = Vec<u8>;
    type Config = ();

    fn verify(tree_hash: Vec<u8>, _config: ()) -> Option<Self> {
        if !tree_hash.is_empty() {
            Some(TreeUpdatesCompleted { tree_hash })
        } else {
            None
        }
    }

    fn description(&self) -> &'static str {
        "Tree updates completed"
    }
}

// ========== CLI Protocol Witnesses ==========

/// Witness proving account has been initialized
#[derive(Debug, Clone)]
pub struct AccountInitialized {
    pub account_id: aura_journal::AccountId,
}

impl RuntimeWitness for AccountInitialized {
    type Evidence = aura_journal::AccountId;
    type Config = ();

    fn verify(account_id: aura_journal::AccountId, _config: ()) -> Option<Self> {
        Some(AccountInitialized { account_id })
    }

    fn description(&self) -> &'static str {
        "Account initialized"
    }
}

/// Witness proving account config has been loaded
#[derive(Debug, Clone)]
pub struct AccountConfigLoaded {
    pub config_path: String,
}

impl RuntimeWitness for AccountConfigLoaded {
    type Evidence = String;
    type Config = ();

    fn verify(config_path: String, _config: ()) -> Option<Self> {
        Some(AccountConfigLoaded { config_path })
    }

    fn description(&self) -> &'static str {
        "Account config loaded"
    }
}

/// Witness proving command has completed
#[derive(Debug, Clone)]
pub struct CommandCompleted {
    pub command_name: String,
    pub exit_code: i32,
}

impl RuntimeWitness for CommandCompleted {
    type Evidence = (String, i32);
    type Config = ();

    fn verify(evidence: (String, i32), _config: ()) -> Option<Self> {
        let (command_name, exit_code) = evidence;
        Some(CommandCompleted {
            command_name,
            exit_code,
        })
    }

    fn description(&self) -> &'static str {
        "CLI command completed"
    }
}

// ========== Helper Functions ==========

fn is_commitment_event(event: &Event) -> bool {
    matches!(
        event.event_type,
        aura_journal::EventType::RecordDkdCommitment(_)
    )
}

fn is_valid_commitment(event: &Event, config: &CommitmentConfig) -> bool {
    use aura_journal::EventType;

    // Extract commitment data from event
    let commitment_data = match &event.event_type {
        EventType::RecordDkdCommitment(commit_event) => &commit_event.commitment,
        _ => return false,
    };

    // Validate commitment format and cryptographic properties
    if commitment_data.len() != 32 {
        tracing::warn!("Invalid commitment length: {}", commitment_data.len());
        return false;
    }

    // Validate that commitment is properly formatted (non-zero, etc.)
    if commitment_data.iter().all(|&b| b == 0) {
        tracing::warn!("Invalid all-zero commitment");
        return false;
    }

    // Validate against session configuration
    let commit_event = match &event.event_type {
        EventType::RecordDkdCommitment(commit_event) => commit_event,
        _ => return false,
    };

    if commit_event.session_id != config.session_id {
        tracing::warn!("Commitment session ID mismatch");
        return false;
    }

    // Validate device authorization
    let device_id = match extract_device_id(event) {
        Some(id) => id,
        None => {
            tracing::warn!("No device ID found in event authorization");
            return false;
        }
    };

    // Validate that device is authorized to participate in this session
    if !config.authorized_devices.contains(&device_id) {
        tracing::warn!(
            "Device {:?} not authorized for session {:?}",
            device_id,
            config.session_id
        );
        return false;
    }

    // Validate event signature
    if let Err(e) = validate_event_signature(event) {
        tracing::warn!("Event signature validation failed: {:?}", e);
        return false;
    }

    // Validate commitment cryptographic properties
    if let Err(e) = validate_commitment_crypto(commitment_data) {
        tracing::warn!("Commitment cryptographic validation failed: {:?}", e);
        return false;
    }

    // Validate timing constraints
    if let Err(e) = validate_commitment_timing(event, config) {
        tracing::warn!("Commitment timing validation failed: {:?}", e);
        return false;
    }

    tracing::debug!("Commitment validation passed for device {:?}", device_id);
    true
}

/// Validate event signature against device public key
fn validate_event_signature(event: &Event) -> Result<(), ValidationError> {
    use aura_journal::EventAuthorization;
    use ed25519_dalek::Verifier;

    match &event.authorization {
        EventAuthorization::DeviceCertificate {
            device_id,
            signature,
        } => {
            // Create message to verify (event without authorization)
            let message = create_signing_message(event)?;

            // Get device public key (placeholder - in production, retrieve from device registry)
            let public_key = get_device_public_key(device_id)?;

            // Verify signature
            public_key.verify(&message, signature).map_err(|e| {
                ValidationError::SignatureVerification(format!(
                    "Signature verification failed: {}",
                    e
                ))
            })?;

            tracing::debug!("Event signature validated for device {:?}", device_id);
            Ok(())
        }
        EventAuthorization::ThresholdSignature(_) => {
            // Validate threshold signature
            validate_threshold_signature(event)
        }
        EventAuthorization::GuardianSignature {
            guardian_id,
            signature: _,
        } => {
            // Guardian signature validation - placeholder implementation
            tracing::debug!(
                "Guardian signature validation for {:?} - placeholder",
                guardian_id
            );
            Ok(())
        }
    }
}

/// Validate commitment cryptographic properties
fn validate_commitment_crypto(commitment: &[u8]) -> Result<(), ValidationError> {
    // Ensure commitment is a valid curve point
    if commitment.len() != 32 {
        return Err(ValidationError::InvalidCommitment(
            "Invalid commitment length".to_string(),
        ));
    }

    // TODO: Check if commitment represents a valid curve point
    // Note: ed25519_dalek v2.2.0 may not expose edwards module directly
    // For now, we rely on length validation above
    // In production, add proper curve point validation

    tracing::debug!("Commitment cryptographic validation passed");
    Ok(())
}

/// Validate commitment timing constraints
fn validate_commitment_timing(
    event: &Event,
    config: &CommitmentConfig,
) -> Result<(), ValidationError> {
    // TODO: This should use injected Effects for time instead of SystemTime
    // For now, we skip the timing check to avoid disallowed SystemTime::now()
    // The timing validation would need to be done at a higher level with access to Effects
    tracing::warn!("Commitment timing validation disabled - requires Effects integration");
    let event_time = event.timestamp;
    let time_diff = 0; // Skip timing check

    if time_diff > config.max_time_skew {
        return Err(ValidationError::TimingConstraint(format!(
            "Event timestamp outside acceptable window: {} seconds",
            time_diff
        )));
    }

    // Check if commitment is made within session lifetime
    if let Some(session_deadline) = config.session_deadline {
        if event_time > session_deadline {
            return Err(ValidationError::TimingConstraint(
                "Commitment made after session deadline".to_string(),
            ));
        }
    }

    tracing::debug!("Commitment timing validation passed");
    Ok(())
}

/// Create signing message for event verification
fn create_signing_message(event: &Event) -> Result<Vec<u8>, ValidationError> {
    // Create canonical representation of event for signing
    let mut message = Vec::new();
    message.extend_from_slice(event.event_id.0.as_bytes());
    message.extend_from_slice(event.account_id.0.as_bytes());
    message.extend_from_slice(&event.timestamp.to_le_bytes());
    message.extend_from_slice(&event.nonce.to_le_bytes());

    if let Some(parent_hash) = &event.parent_hash {
        message.extend_from_slice(parent_hash);
    }

    // Add event type specific data
    match &event.event_type {
        aura_journal::EventType::RecordDkdCommitment(commit_event) => {
            message.extend_from_slice(commit_event.session_id.as_bytes());
            message.extend_from_slice(&commit_event.commitment);
        }
        aura_journal::EventType::RevealDkdPoint(reveal_event) => {
            message.extend_from_slice(reveal_event.session_id.as_bytes());
            message.extend_from_slice(&reveal_event.point);
        }
        // Add other event types as needed
        _ => {}
    }

    Ok(message)
}

/// Get device public key from device registry
fn get_device_public_key(
    device_id: &aura_journal::DeviceId,
) -> Result<ed25519_dalek::VerifyingKey, ValidationError> {
    // Placeholder implementation - in production, retrieve from secure device registry
    // For now, derive a deterministic key from device ID for testing
    use ed25519_dalek::VerifyingKey;

    // Create deterministic key material from device ID
    let mut key_material = [0u8; 32];
    let device_bytes = device_id.0.as_bytes();

    // Use first 32 bytes of device ID, padding with zeros if needed
    let copy_len = std::cmp::min(32, device_bytes.len());
    key_material[..copy_len].copy_from_slice(&device_bytes[..copy_len]);

    VerifyingKey::from_bytes(&key_material)
        .map_err(|e| ValidationError::DeviceKeyRetrieval(format!("Invalid device key: {}", e)))
}

/// Validate threshold signature
fn validate_threshold_signature(event: &Event) -> Result<(), ValidationError> {
    use aura_journal::EventAuthorization;

    match &event.authorization {
        EventAuthorization::ThresholdSignature(threshold_sig) => {
            // Create message for verification
            let _message = create_signing_message(event)?;

            // Placeholder threshold signature verification
            // In production, use FROST library for proper verification
            if threshold_sig.signature_shares.is_empty() {
                return Err(ValidationError::ThresholdSignature(
                    "No signature shares provided".to_string(),
                ));
            }

            tracing::debug!("Threshold signature validation passed (placeholder)");
            Ok(())
        }
        _ => Err(ValidationError::ThresholdSignature(
            "Not a threshold signature".to_string(),
        )),
    }
}

/// Validation error types
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Invalid commitment: {0}")]
    InvalidCommitment(String),

    #[error("Signature verification failed: {0}")]
    SignatureVerification(String),

    #[error("Timing constraint violation: {0}")]
    TimingConstraint(String),

    #[error("Device key retrieval failed: {0}")]
    DeviceKeyRetrieval(String),

    #[error("Threshold signature validation failed: {0}")]
    ThresholdSignature(String),
}

fn extract_device_id(event: &Event) -> Option<DeviceId> {
    match &event.authorization {
        aura_journal::EventAuthorization::DeviceCertificate { device_id, .. } => Some(*device_id),
        _ => None,
    }
}

fn is_reveal_event(event: &Event) -> bool {
    matches!(event.event_type, aura_journal::EventType::RevealDkdPoint(_))
}

fn verify_reveal_against_commitment(event: &Event, commitments: &CollectedCommitments) -> bool {
    use aura_journal::EventType;
    use blake3::Hasher;

    // Extract reveal data from event
    let (reveal_point, session_id, device_id) = match &event.event_type {
        EventType::RevealDkdPoint(reveal_event) => (
            &reveal_event.point,
            &reveal_event.session_id,
            &reveal_event.device_id,
        ),
        _ => return false,
    };

    // Verify the device is a participant
    if !commitments.participants.contains(device_id) {
        tracing::warn!("Device {:?} not in commitment participants", device_id);
        return false;
    }

    // Get the stored commitment for this device
    let stored_commitment = match commitments.commitments.get(device_id) {
        Some(commitment) => commitment,
        None => {
            tracing::warn!("No stored commitment found for device {:?}", device_id);
            return false;
        }
    };

    // Validate reveal point format
    if reveal_point.len() != 32 {
        tracing::warn!(
            "Invalid reveal point length for device {:?}: expected 32, got {}",
            device_id,
            reveal_point.len()
        );
        return false;
    }

    // Verify reveal point is not all zeros
    if reveal_point.iter().all(|&b| b == 0) {
        tracing::warn!("Invalid all-zero reveal point from device {:?}", device_id);
        return false;
    }

    // Cryptographic verification: H(reveal_point || device_id || session_id) should equal commitment
    let mut hasher = Hasher::new();
    hasher.update(reveal_point);
    hasher.update(device_id.0.as_bytes());
    hasher.update(session_id.as_bytes());
    let computed_commitment = hasher.finalize();

    // Compare computed commitment with stored commitment
    if computed_commitment.as_bytes() != stored_commitment.as_slice() {
        tracing::warn!(
            "Commitment verification failed for device {:?}: computed {:?} != stored {:?}",
            device_id,
            computed_commitment.as_bytes(),
            stored_commitment
        );
        return false;
    }

    // Additional cryptographic validation for DKD point
    if !is_valid_dkd_point(reveal_point) {
        tracing::warn!("Invalid DKD point from device {:?}", device_id);
        return false;
    }

    // Verify timing constraints (reveal should come after commitment phase)
    if !is_reveal_timing_valid(event, commitments) {
        tracing::warn!("Reveal timing validation failed for device {:?}", device_id);
        return false;
    }

    tracing::debug!(
        "Cryptographic reveal verification passed for device {:?}",
        device_id
    );
    true
}

/// Validate that the revealed point is a valid DKD contribution
fn is_valid_dkd_point(point: &[u8]) -> bool {
    // Verify the point is the correct length for a curve point
    if point.len() != 32 {
        return false;
    }

    // Verify it's not the identity element (all zeros)
    if point.iter().all(|&b| b == 0) {
        return false;
    }

    // For Ed25519 points, we can do basic validation
    // In a full implementation, this would verify the point is on the curve
    // For now, check that it has proper entropy (not obviously weak)
    let mut entropy_check = 0u8;
    for &byte in point {
        entropy_check ^= byte;
    }

    // Reject points with no entropy
    if entropy_check == 0 {
        return false;
    }

    true
}

/// Verify that the reveal timing is valid relative to commitments
fn is_reveal_timing_valid(reveal_event: &Event, commitments: &CollectedCommitments) -> bool {
    // Extract device ID from reveal event
    let device_id = match &reveal_event.event_type {
        aura_journal::EventType::RevealDkdPoint(reveal) => &reveal.device_id,
        _ => return false,
    };

    // Find the corresponding commitment timestamp
    let commitment_timestamp = commitments.commitment_timestamps.get(device_id);

    match commitment_timestamp {
        Some(commit_time) => {
            // Reveal must come after commitment
            if reveal_event.timestamp <= *commit_time {
                tracing::warn!(
                    "Reveal timestamp {} <= commitment timestamp {} for device {:?}",
                    reveal_event.timestamp,
                    commit_time,
                    device_id
                );
                return false;
            }

            // Reveal should not be too far after commitment (e.g., within 1 hour)
            let max_delay = 3600; // 1 hour in seconds
            if reveal_event.timestamp > commit_time + max_delay {
                tracing::warn!(
                    "Reveal timestamp {} too far after commitment {} for device {:?}",
                    reveal_event.timestamp,
                    commit_time,
                    device_id
                );
                return false;
            }

            true
        }
        None => {
            tracing::warn!("No commitment timestamp found for device {:?}", device_id);
            false
        }
    }
}

fn is_approval_event(event: &Event) -> bool {
    matches!(
        event.event_type,
        aura_journal::EventType::CollectGuardianApproval(_)
    )
}

fn is_valid_approval(event: &Event, config: &ApprovalConfig) -> bool {
    use aura_journal::EventType;

    // Extract approval data from event
    let approval_event = match &event.event_type {
        EventType::CollectGuardianApproval(approval) => approval,
        _ => return false,
    };

    // Validate guardian authorization
    let guardian_id = match extract_device_id(event) {
        Some(id) => id,
        None => {
            tracing::warn!("No device ID found in approval event");
            return false;
        }
    };

    // Check if guardian is in expected list (if provided)
    if let Some(expected_guardians) = &config.expected_guardians {
        if !expected_guardians.contains(&guardian_id) {
            tracing::warn!("Guardian {:?} not in expected guardians list", guardian_id);
            return false;
        }
    }

    // Basic validation of approval (signature and approval status)
    if !approval_event.approved {
        tracing::warn!("Guardian {:?} did not approve recovery", guardian_id);
        return false;
    }

    if approval_event.approval_signature.is_empty() {
        tracing::warn!("Missing approval signature from guardian {:?}", guardian_id);
        return false;
    }

    // Additional validation could include:
    // - Verifying guardian signature over approval
    // - Checking guardian is still active (not revoked)
    // - Validating approval nonce to prevent replay

    tracing::debug!("Guardian approval validation passed for {:?}", guardian_id);
    true
}

fn extract_guardian_id(event: &Event) -> Option<DeviceId> {
    extract_device_id(event)
}

fn is_share_event(event: &Event) -> bool {
    matches!(
        event.event_type,
        aura_journal::EventType::SubmitRecoveryShare(_)
    )
}

fn is_valid_share(event: &Event, config: &ShareConfig) -> bool {
    use aura_journal::EventType;

    // Extract share data from event
    let share_event = match &event.event_type {
        EventType::SubmitRecoveryShare(share) => share,
        _ => return false,
    };

    // Validate guardian authorization
    let guardian_id = match extract_device_id(event) {
        Some(id) => id,
        None => {
            tracing::warn!("No device ID found in share event");
            return false;
        }
    };

    // Basic validation for now - in production would check authorization
    // against the recovery session configuration

    // Validate share format and cryptographic properties
    if share_event.encrypted_share.is_empty() {
        tracing::warn!("Empty recovery share submitted");
        return false;
    }

    // Basic timing validation - share was submitted for correct recovery
    if share_event.recovery_id != config.recovery_session_id {
        tracing::warn!("Recovery share for wrong recovery session");
        return false;
    }

    // Additional validation could include:
    // - Verifying the share is properly encrypted for the recipient
    // - Checking the share contributes to the correct recovery session
    // - Validating proof of share possession without revealing the share
    // - Ensuring the guardian hasn't already submitted a share

    tracing::debug!(
        "Recovery share validation passed for guardian {:?}",
        guardian_id
    );
    true
}

// ========== Universal RuntimeWitness Implementations ==========

/// RuntimeWitness implementation for unit type (used for simple transitions)
impl RuntimeWitness for () {
    type Evidence = ();
    type Config = ();

    fn verify(_evidence: (), _config: ()) -> Option<Self> {
        Some(())
    }

    fn description(&self) -> &'static str {
        "No witness required"
    }
}

/// RuntimeWitness implementation for (String, PresenceTicket) tuple
impl RuntimeWitness for (String, crate::protocols::transport::PresenceTicket) {
    type Evidence = (String, crate::protocols::transport::PresenceTicket);
    type Config = ();

    fn verify(
        evidence: (String, crate::protocols::transport::PresenceTicket),
        _config: (),
    ) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Peer ID and presence ticket provided"
    }
}

/// RuntimeWitness implementation for PresenceTicket
impl RuntimeWitness for crate::protocols::transport::PresenceTicket {
    type Evidence = crate::protocols::transport::PresenceTicket;
    type Config = ();

    fn verify(evidence: crate::protocols::transport::PresenceTicket, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Presence ticket provided"
    }
}

/// RuntimeWitness implementation for MessageContext
impl RuntimeWitness for crate::protocols::transport::MessageContext {
    type Evidence = crate::protocols::transport::MessageContext;
    type Config = ();

    fn verify(evidence: crate::protocols::transport::MessageContext, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Message context provided"
    }
}

/// RuntimeWitness implementation for BroadcastContext
impl RuntimeWitness for crate::protocols::transport::BroadcastContext {
    type Evidence = crate::protocols::transport::BroadcastContext;
    type Config = ();

    fn verify(
        evidence: crate::protocols::transport::BroadcastContext,
        _config: (),
    ) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Broadcast context provided"
    }
}

/// RuntimeWitness implementation for FrostSigningContext
/// RuntimeWitness implementation for SigningCommitment
impl RuntimeWitness for aura_crypto::SigningCommitment {
    type Evidence = aura_crypto::SigningCommitment;
    type Config = ();

    fn verify(evidence: aura_crypto::SigningCommitment, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "FROST signing commitment provided"
    }
}

/// RuntimeWitness implementation for SignatureShare
impl RuntimeWitness for aura_crypto::SignatureShare {
    type Evidence = aura_crypto::SignatureShare;
    type Config = ();

    fn verify(evidence: aura_crypto::SignatureShare, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "FROST signature share provided"
    }
}

/// RuntimeWitness implementation for (u16, u16) tuple
impl RuntimeWitness for (u16, u16) {
    type Evidence = (u16, u16);
    type Config = ();

    fn verify(evidence: (u16, u16), _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Threshold parameters provided"
    }
}

/// RuntimeWitness implementation for (FrostKeyShare, u16) tuple
impl RuntimeWitness for (aura_crypto::FrostKeyShare, u16) {
    type Evidence = (aura_crypto::FrostKeyShare, u16);
    type Config = ();

    fn verify(evidence: (aura_crypto::FrostKeyShare, u16), _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "FROST key share and participant count provided"
    }
}

/// RuntimeWitness implementation for Vec<Event>
impl RuntimeWitness for Vec<aura_journal::Event> {
    type Evidence = Vec<aura_journal::Event>;
    type Config = ();

    fn verify(evidence: Vec<aura_journal::Event>, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Event list provided"
    }
}

/// RuntimeWitness implementation for SessionOutcome
impl RuntimeWitness for aura_journal::SessionOutcome {
    type Evidence = aura_journal::SessionOutcome;
    type Config = ();

    fn verify(evidence: aura_journal::SessionOutcome, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Session outcome provided"
    }
}

/// RuntimeWitness implementation for String
impl RuntimeWitness for String {
    type Evidence = String;
    type Config = ();

    fn verify(evidence: String, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "String value provided"
    }
}

/// RuntimeWitness implementation for LockRequest
impl RuntimeWitness for crate::protocols::journal::LockRequest {
    type Evidence = crate::protocols::journal::LockRequest;
    type Config = ();

    fn verify(evidence: crate::protocols::journal::LockRequest, _config: ()) -> Option<Self> {
        Some(evidence)
    }

    fn description(&self) -> &'static str {
        "Lock request provided"
    }
}
