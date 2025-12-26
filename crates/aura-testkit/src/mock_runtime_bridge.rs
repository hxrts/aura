//! Mock RuntimeBridge for testing
//!
//! Provides a test-friendly implementation of the RuntimeBridge trait that
//! uses in-memory state instead of real runtime infrastructure.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_testkit::MockRuntimeBridge;
//! use aura_app::AppCore;
//!
//! let bridge = MockRuntimeBridge::new();
//! let app = AppCore::with_runtime(config, Arc::new(bridge))?;
//! ```

use async_trait::async_trait;
use aura_app::runtime_bridge::{
    BridgeDeviceInfo, CeremonyKind, CeremonyStatus, DeviceEnrollmentStart, InvitationBridgeStatus,
    InvitationBridgeType, InvitationInfo, KeyRotationCeremonyStatus, LanPeerInfo,
    RendezvousStatus, RuntimeBridge, SettingsBridgeState, SyncStatus,
};
use aura_app::signal_defs::CONTACTS_SIGNAL;
use aura_app::views::contacts::{Contact, ContactsState};
use aura_app::IntentError;
use aura_core::domain::Hash32;
use aura_core::effects::amp::{
    AmpCiphertext, AmpHeader, ChannelCloseParams, ChannelCreateParams, ChannelJoinParams,
    ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::threshold::ThresholdConfig;
use aura_core::tree::{AttestedOp, TreeOp};
use aura_core::types::FrostThreshold;
use aura_core::SigningContext;
use aura_core::{DeviceId, ThresholdSignature};
use aura_app::ReactiveHandler;
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_relational::ContactFact;
use base64::Engine;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Mock RuntimeBridge for testing
///
/// This provides a functional mock of the RuntimeBridge trait that:
/// - Uses in-memory state for facts, invitations, etc.
/// - Provides deterministic behavior for tests
/// - Allows inspection of committed state
/// - Emits signals when state changes (for proper test integration)
pub struct MockRuntimeBridge {
    /// Authority ID for this mock runtime
    authority_id: AuthorityId,
    /// Device ID for this mock runtime
    device_id: DeviceId,
    /// Reactive handler for signals
    reactive_handler: ReactiveHandler,
    /// Committed relational facts
    facts: Arc<RwLock<Vec<RelationalFact>>>,
    /// Created invitations
    invitations: Arc<RwLock<HashMap<String, InvitationInfo>>>,
    /// Contacts (simulated from accepted invitations)
    contacts: Arc<RwLock<Vec<Contact>>>,
    /// Mock display name
    display_name: Arc<RwLock<String>>,
    /// Mock MFA policy
    mfa_policy: Arc<RwLock<String>>,
    /// Counter for generating unique IDs
    id_counter: AtomicU64,
    /// Simulated current time (ms since epoch)
    current_time_ms: AtomicU64,
    /// Devices registered with this authority
    devices: Arc<RwLock<Vec<BridgeDeviceInfo>>>,
}

// Manual Debug impl since ReactiveHandler doesn't derive Debug
impl std::fmt::Debug for MockRuntimeBridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockRuntimeBridge")
            .field("authority_id", &self.authority_id)
            .field("device_id", &self.device_id)
            .finish_non_exhaustive()
    }
}

impl MockRuntimeBridge {
    /// Create a new mock runtime bridge with a random authority
    pub fn new() -> Self {
        Self::with_authority(AuthorityId::new())
    }

    /// Create a new mock runtime bridge with a specific authority
    pub fn with_authority(authority_id: AuthorityId) -> Self {
        let device_id = DeviceId::new();
        Self {
            authority_id,
            device_id: device_id.clone(),
            reactive_handler: ReactiveHandler::new(),
            facts: Arc::new(RwLock::new(Vec::new())),
            invitations: Arc::new(RwLock::new(HashMap::new())),
            contacts: Arc::new(RwLock::new(Vec::new())),
            display_name: Arc::new(RwLock::new("MockUser".to_string())),
            mfa_policy: Arc::new(RwLock::new("Disabled".to_string())),
            id_counter: AtomicU64::new(1),
            current_time_ms: AtomicU64::new(1700000000000), // Fixed starting time
            devices: Arc::new(RwLock::new(vec![BridgeDeviceInfo {
                id: device_id.to_string(),
                name: "MockDevice".to_string(),
                is_current: true,
                last_seen: Some(1700000000000),
            }])),
        }
    }

    /// Get contacts for test assertions
    pub async fn get_contacts(&self) -> Vec<Contact> {
        self.contacts.read().await.clone()
    }

    /// Helper to emit CONTACTS_SIGNAL with current contacts
    async fn emit_contacts_signal(&self) {
        let contacts = self.contacts.read().await.clone();
        let state = ContactsState {
            contacts,
            selected_contact_id: None,
            search_filter: None,
        };
        // Ignore errors - signal may not be registered yet during initialization
        let _ = self.reactive_handler.emit(&*CONTACTS_SIGNAL, state).await;
    }

    /// Process a ContactFact from binding data and update internal contacts list
    /// Returns true if contacts were changed
    async fn process_contact_fact_data(&self, _binding_type: &str, binding_data: &[u8]) -> bool {
        let Some(fact) = ContactFact::from_bytes(binding_data) else {
            return false;
        };

        match fact {
            ContactFact::Renamed {
                contact_id,
                new_nickname,
                ..
            } => {
                let mut contacts = self.contacts.write().await;
                if let Some(contact) = contacts.iter_mut().find(|c| c.id == contact_id) {
                    contact.nickname = new_nickname;
                } else {
                    contacts.push(Contact {
                        id: contact_id,
                        nickname: new_nickname,
                        suggested_name: None,
                        is_guardian: false,
                        is_resident: false,
                        last_interaction: Some(self.now_ms()),
                        is_online: false,
                    });
                }
                true
            }
            ContactFact::Removed { contact_id, .. } => {
                let mut contacts = self.contacts.write().await;
                let len_before = contacts.len();
                contacts.retain(|c| c.id != contact_id);
                contacts.len() != len_before
            }
            ContactFact::Added {
                contact_id,
                nickname,
                ..
            } => {
                let mut contacts = self.contacts.write().await;
                if contacts.iter().any(|c| c.id == contact_id) {
                    return false;
                }
                contacts.push(Contact {
                    id: contact_id,
                    nickname,
                    suggested_name: None,
                    is_guardian: false,
                    is_resident: false,
                    last_interaction: Some(self.now_ms()),
                    is_online: false,
                });
                true
            }
        }
    }

    /// Get committed facts for test assertions
    pub async fn get_committed_facts(&self) -> Vec<RelationalFact> {
        self.facts.read().await.clone()
    }

    /// Get created invitations for test assertions
    pub async fn get_invitations(&self) -> HashMap<String, InvitationInfo> {
        self.invitations.read().await.clone()
    }

    /// Advance the mock time by the given milliseconds
    pub fn advance_time_ms(&self, ms: u64) {
        self.current_time_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// Set the mock time to a specific value
    pub fn set_time_ms(&self, ms: u64) {
        self.current_time_ms.store(ms, Ordering::SeqCst);
    }

    fn next_id(&self) -> String {
        format!("mock-{}", self.id_counter.fetch_add(1, Ordering::SeqCst))
    }

    fn now_ms(&self) -> u64 {
        self.current_time_ms.load(Ordering::SeqCst)
    }
}

impl Default for MockRuntimeBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RuntimeBridge for MockRuntimeBridge {
    // =========================================================================
    // Identity & Authority (Required)
    // =========================================================================

    fn authority_id(&self) -> AuthorityId {
        self.authority_id.clone()
    }

    fn reactive_handler(&self) -> ReactiveHandler {
        self.reactive_handler.clone()
    }

    // =========================================================================
    // Typed Fact Commit (Override default)
    // =========================================================================

    async fn commit_relational_facts(&self, facts: &[RelationalFact]) -> Result<(), IntentError> {
        // Store all facts
        {
            let mut stored = self.facts.write().await;
            stored.extend(facts.iter().cloned());
        }

        // Process Generic facts to detect ContactFacts and update signals
        let mut contact_changed = false;
        for fact in facts {
            // ContactFacts are stored as RelationalFact::Generic with binding_type containing "contact"
            if let RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            } = fact
            {
                if binding_type.contains("contact") {
                    // Parse the contact fact and update state
                    if self.process_contact_fact_data(binding_type, binding_data).await {
                        contact_changed = true;
                    }
                }
            }
        }

        // Emit signal once if any contacts changed
        if contact_changed {
            self.emit_contacts_signal().await;
        }

        Ok(())
    }

    // =========================================================================
    // AMP Channel Operations (Override defaults to return success)
    // =========================================================================

    async fn amp_create_channel(
        &self,
        _params: ChannelCreateParams,
    ) -> Result<ChannelId, IntentError> {
        Ok(ChannelId::new(Hash32::default()))
    }

    async fn amp_close_channel(&self, _params: ChannelCloseParams) -> Result<(), IntentError> {
        Ok(())
    }

    async fn amp_join_channel(&self, _params: ChannelJoinParams) -> Result<(), IntentError> {
        Ok(())
    }

    async fn amp_leave_channel(&self, _params: ChannelLeaveParams) -> Result<(), IntentError> {
        Ok(())
    }

    async fn bump_channel_epoch(
        &self,
        _context: ContextId,
        _channel: ChannelId,
        _reason: String,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn amp_send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, IntentError> {
        // Return a mock ciphertext
        Ok(AmpCiphertext {
            header: AmpHeader {
                context: params.context,
                channel: params.channel,
                chan_epoch: 0,
                ratchet_gen: 0,
            },
            ciphertext: vec![0u8; 32],
        })
    }

    // =========================================================================
    // Moderation Operations (Override defaults to return success)
    // =========================================================================

    async fn moderation_kick(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_ban(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_unban(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_mute(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
        _duration_secs: Option<u64>,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_unmute(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _target: AuthorityId,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_pin(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _message_id: String,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn moderation_unpin(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _message_id: String,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    async fn channel_set_topic(
        &self,
        _context_id: ContextId,
        _channel_id: ChannelId,
        _topic: String,
        _timestamp_ms: u64,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    // =========================================================================
    // Sync Operations
    // =========================================================================

    async fn get_sync_status(&self) -> SyncStatus {
        SyncStatus {
            is_running: true,
            connected_peers: 0,
            last_sync_ms: Some(self.now_ms()),
            pending_facts: 0,
            active_sessions: 0,
        }
    }

    async fn get_sync_peers(&self) -> Vec<DeviceId> {
        vec![]
    }

    async fn trigger_sync(&self) -> Result<(), IntentError> {
        Ok(())
    }

    async fn sync_with_peer(&self, _peer_id: &str) -> Result<(), IntentError> {
        Ok(())
    }

    // =========================================================================
    // Discovery Operations
    // =========================================================================

    async fn get_discovered_peers(&self) -> Vec<AuthorityId> {
        vec![]
    }

    async fn get_rendezvous_status(&self) -> RendezvousStatus {
        RendezvousStatus {
            is_running: true,
            cached_peers: 0,
        }
    }

    async fn trigger_discovery(&self) -> Result<(), IntentError> {
        Ok(())
    }

    async fn get_lan_peers(&self) -> Vec<LanPeerInfo> {
        vec![]
    }

    async fn send_lan_invitation(
        &self,
        _peer: &LanPeerInfo,
        _invitation_code: &str,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    // =========================================================================
    // Tree Operations
    // =========================================================================

    async fn sign_tree_op(&self, op: &TreeOp) -> Result<AttestedOp, IntentError> {
        // Create a mock attested op without real signature
        Ok(AttestedOp {
            op: op.clone(),
            agg_sig: vec![0u8; 64], // Mock signature
            signer_count: 1,
        })
    }

    async fn bootstrap_signing_keys(&self) -> Result<Vec<u8>, IntentError> {
        // Return mock key material
        Ok(vec![0u8; 32])
    }

    async fn get_threshold_config(&self) -> Option<ThresholdConfig> {
        None // No threshold config in mock
    }

    async fn has_signing_capability(&self) -> bool {
        true
    }

    async fn get_public_key_package(&self) -> Option<Vec<u8>> {
        Some(vec![0u8; 32])
    }

    // =========================================================================
    // Key Rotation (Override defaults to return success)
    // =========================================================================

    async fn commit_guardian_key_rotation(&self, _new_epoch: u64) -> Result<(), IntentError> {
        Ok(())
    }

    async fn rollback_guardian_key_rotation(&self, _failed_epoch: u64) -> Result<(), IntentError> {
        Ok(())
    }

    async fn sign_with_context(
        &self,
        _context: SigningContext,
    ) -> Result<ThresholdSignature, IntentError> {
        // Return a mock threshold signature
        Ok(ThresholdSignature::new(
            vec![0u8; 64],   // signature
            1,              // signer_count
            vec![1],        // signers
            vec![0u8; 32],  // public_key_package
            0,              // epoch
        ))
    }

    async fn rotate_guardian_keys(
        &self,
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _guardian_ids: &[String],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), IntentError> {
        // Return mock key rotation data: (epoch, key_packages, public_key_package)
        let epoch = 1u64;
        let key_packages: Vec<Vec<u8>> = vec![vec![0u8; 32]; 3]; // 3 guardian packages
        let public_key_package = vec![0u8; 32];
        Ok((epoch, key_packages, public_key_package))
    }

    async fn initiate_guardian_ceremony(
        &self,
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _guardian_ids: &[String],
    ) -> Result<String, IntentError> {
        Ok(self.next_id())
    }

    async fn initiate_device_threshold_ceremony(
        &self,
        _threshold_k: FrostThreshold,
        _total_n: u16,
        _device_ids: &[String],
    ) -> Result<String, IntentError> {
        Ok(self.next_id())
    }

    async fn initiate_device_enrollment_ceremony(
        &self,
        device_name: String,
    ) -> Result<DeviceEnrollmentStart, IntentError> {
        Ok(DeviceEnrollmentStart {
            ceremony_id: self.next_id(),
            enrollment_code: format!("aura-enroll:mock:{}", device_name),
            pending_epoch: 1,
            device_id: DeviceId::new(),
        })
    }

    async fn initiate_device_removal_ceremony(
        &self,
        _device_id: String,
    ) -> Result<String, IntentError> {
        Ok(self.next_id())
    }

    async fn get_ceremony_status(&self, ceremony_id: &str) -> Result<CeremonyStatus, IntentError> {
        Ok(CeremonyStatus {
            ceremony_id: ceremony_id.to_string(),
            accepted_count: 0,
            total_count: 3,
            threshold: 2,
            is_complete: false,
            has_failed: false,
            accepted_guardians: Vec::new(),
            error_message: None,
            pending_epoch: Some(1),
        })
    }

    async fn get_key_rotation_ceremony_status(
        &self,
        ceremony_id: &str,
    ) -> Result<KeyRotationCeremonyStatus, IntentError> {
        Ok(KeyRotationCeremonyStatus {
            ceremony_id: ceremony_id.to_string(),
            kind: CeremonyKind::GuardianRotation,
            accepted_count: 0,
            total_count: 3,
            threshold: 2,
            is_complete: false,
            has_failed: false,
            accepted_participants: Vec::new(),
            error_message: None,
            pending_epoch: Some(1),
        })
    }

    async fn cancel_key_rotation_ceremony(&self, _ceremony_id: &str) -> Result<(), IntentError> {
        Ok(())
    }

    async fn get_invited_peer_ids(&self) -> Vec<String> {
        Vec::new()
    }

    async fn respond_to_guardian_ceremony(
        &self,
        _ceremony_id: &str,
        _accept: bool,
        _reason: Option<String>,
    ) -> Result<(), IntentError> {
        Ok(())
    }

    // =========================================================================
    // Invitations (Override defaults with functional mocks)
    // =========================================================================

    async fn export_invitation(&self, invitation_id: &str) -> Result<String, IntentError> {
        let invitations = self.invitations.read().await;

        // Check if we have this invitation already
        if let Some(inv) = invitations.get(invitation_id) {
            // Generate a valid aura:v1:base64 code from the invitation
            let invitation_data = serde_json::json!({
                "version": 1,
                "invitation_id": inv.invitation_id,
                "sender_id": inv.sender_id.uuid().to_string(),
                "invitation_type": match &inv.invitation_type {
                    InvitationBridgeType::Contact { nickname } => {
                        serde_json::json!({
                            "Contact": {
                                "nickname": nickname
                            }
                        })
                    },
                    InvitationBridgeType::Guardian { subject_authority } => {
                        serde_json::json!({
                            "Guardian": {
                                "subject_authority": subject_authority.uuid().to_string()
                            }
                        })
                    },
                    InvitationBridgeType::Channel { block_id } => {
                        serde_json::json!({
                            "Channel": {
                                "block_id": block_id
                            }
                        })
                    },
                    InvitationBridgeType::DeviceEnrollment { device_id, device_name, .. } => {
                        serde_json::json!({
                            "DeviceEnrollment": {
                                "device_id": device_id,
                                "device_name": device_name
                            }
                        })
                    },
                },
                "expires_at": inv.expires_at_ms,
                "message": inv.message
            });

            let json_str = serde_json::to_string(&invitation_data)
                .map_err(|e| IntentError::internal_error(format!("JSON error: {}", e)))?;
            let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
            return Ok(format!("aura:v1:{}", b64));
        }

        // For IDs not in our map, generate a synthetic invitation code
        // This allows tests to export arbitrary IDs without pre-creating invitations
        let now = self.now_ms();
        let invitation_data = serde_json::json!({
            "version": 1,
            "invitation_id": invitation_id,
            "sender_id": self.authority_id.uuid().to_string(),
            "invitation_type": {
                "Contact": {
                    "nickname": null
                }
            },
            "expires_at": now + 3600000,  // 1 hour
            "message": null
        });

        let json_str = serde_json::to_string(&invitation_data)
            .map_err(|e| IntentError::internal_error(format!("JSON error: {}", e)))?;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json_str.as_bytes());
        Ok(format!("aura:v1:{}", b64))
    }

    async fn create_contact_invitation(
        &self,
        receiver: AuthorityId,
        _nickname: Option<String>,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_id = self.next_id();
        let now = self.now_ms();
        let expires_at_ms = ttl_ms.map(|ttl| now + ttl);

        let info = InvitationInfo {
            invitation_id: invitation_id.clone(),
            sender_id: self.authority_id.clone(),
            receiver_id: receiver,
            invitation_type: InvitationBridgeType::Contact { nickname: None },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: now,
            expires_at_ms,
            message,
        };

        let mut invitations = self.invitations.write().await;
        invitations.insert(invitation_id, info.clone());

        Ok(info)
    }

    async fn create_guardian_invitation(
        &self,
        receiver: AuthorityId,
        subject: AuthorityId,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_id = self.next_id();
        let now = self.now_ms();
        let expires_at_ms = ttl_ms.map(|ttl| now + ttl);

        let info = InvitationInfo {
            invitation_id: invitation_id.clone(),
            sender_id: self.authority_id.clone(),
            receiver_id: receiver,
            invitation_type: InvitationBridgeType::Guardian {
                subject_authority: subject,
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: now,
            expires_at_ms,
            message,
        };

        let mut invitations = self.invitations.write().await;
        invitations.insert(invitation_id, info.clone());

        Ok(info)
    }

    async fn create_channel_invitation(
        &self,
        receiver: AuthorityId,
        block_id: String,
        message: Option<String>,
        ttl_ms: Option<u64>,
    ) -> Result<InvitationInfo, IntentError> {
        let invitation_id = self.next_id();
        let now = self.now_ms();
        let expires_at_ms = ttl_ms.map(|ttl| now + ttl);

        let info = InvitationInfo {
            invitation_id: invitation_id.clone(),
            sender_id: self.authority_id.clone(),
            receiver_id: receiver,
            invitation_type: InvitationBridgeType::Channel { block_id },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: now,
            expires_at_ms,
            message,
        };

        let mut invitations = self.invitations.write().await;
        invitations.insert(invitation_id, info.clone());

        Ok(info)
    }

    async fn accept_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        // First, update the invitation status
        let invitation = {
            let mut invitations = self.invitations.write().await;
            if let Some(inv) = invitations.get_mut(invitation_id) {
                inv.status = InvitationBridgeStatus::Accepted;
                Some(inv.clone())
            } else {
                None
            }
        };

        let invitation = invitation.ok_or_else(|| {
            IntentError::internal_error(format!("Invitation not found: {}", invitation_id))
        })?;

        // For contact invitations, add the sender as a contact
        if matches!(
            invitation.invitation_type,
            InvitationBridgeType::Contact { .. } | InvitationBridgeType::Guardian { .. }
        ) {
            let nickname = match &invitation.invitation_type {
                InvitationBridgeType::Contact { nickname } => {
                    nickname.clone().unwrap_or_else(|| {
                        format!("Contact-{}", &invitation.sender_id.to_string()[..8])
                    })
                }
                _ => format!("Contact-{}", &invitation.sender_id.to_string()[..8]),
            };

            let is_guardian = matches!(
                invitation.invitation_type,
                InvitationBridgeType::Guardian { .. }
            );

            let new_contact = Contact {
                id: invitation.sender_id.clone(),
                nickname,
                suggested_name: invitation.message.clone(),
                is_guardian,
                is_resident: false,
                last_interaction: Some(self.now_ms()),
                is_online: false,
            };

            // Add to contacts list, avoiding duplicates
            {
                let mut contacts = self.contacts.write().await;
                if !contacts.iter().any(|c| c.id == new_contact.id) {
                    contacts.push(new_contact);
                }
            }

            // Emit CONTACTS_SIGNAL
            self.emit_contacts_signal().await;
        }

        Ok(())
    }

    async fn decline_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        let mut invitations = self.invitations.write().await;
        if let Some(inv) = invitations.get_mut(invitation_id) {
            inv.status = InvitationBridgeStatus::Declined;
            Ok(())
        } else {
            Err(IntentError::internal_error(format!(
                "Invitation not found: {}",
                invitation_id
            )))
        }
    }

    async fn cancel_invitation(&self, invitation_id: &str) -> Result<(), IntentError> {
        let mut invitations = self.invitations.write().await;
        if let Some(inv) = invitations.get_mut(invitation_id) {
            inv.status = InvitationBridgeStatus::Cancelled;
            Ok(())
        } else {
            Err(IntentError::internal_error(format!(
                "Invitation not found: {}",
                invitation_id
            )))
        }
    }

    async fn list_pending_invitations(&self) -> Vec<InvitationInfo> {
        let invitations = self.invitations.read().await;
        invitations
            .values()
            .filter(|inv| inv.status == InvitationBridgeStatus::Pending)
            .cloned()
            .collect()
    }

    async fn import_invitation(&self, code: &str) -> Result<InvitationInfo, IntentError> {
        // Parse aura:v1:<base64> format
        if !code.starts_with("aura:v1:") {
            return Err(IntentError::internal_error(format!(
                "Invalid invitation code format: must start with aura:v1:"
            )));
        }

        let b64_part = code.strip_prefix("aura:v1:").unwrap();

        // Decode base64
        let json_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(b64_part)
            .map_err(|e| IntentError::internal_error(format!("Invalid base64: {}", e)))?;

        let json_str = String::from_utf8(json_bytes)
            .map_err(|e| IntentError::internal_error(format!("Invalid UTF-8: {}", e)))?;

        // Parse JSON
        let data: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| IntentError::internal_error(format!("Invalid JSON: {}", e)))?;

        // Extract fields
        let invitation_id = data
            .get("invitation_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.next_id());

        let sender_uuid_str = data
            .get("sender_id")
            .and_then(|v| v.as_str())
            .unwrap_or("00000000-0000-0000-0000-000000000000");

        let sender_uuid = Uuid::parse_str(sender_uuid_str)
            .unwrap_or_else(|_| Uuid::new_v4());
        let sender_id = AuthorityId::from_uuid(sender_uuid);

        let expires_at_ms = data.get("expires_at").and_then(|v| v.as_u64());
        let message = data.get("message").and_then(|v| v.as_str()).map(|s| s.to_string());

        // Parse invitation type
        let invitation_type = if let Some(inv_type) = data.get("invitation_type") {
            if inv_type.get("Contact").is_some() {
                let nickname = inv_type
                    .get("Contact")
                    .and_then(|c| c.get("nickname"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());
                InvitationBridgeType::Contact { nickname }
            } else if inv_type.get("Guardian").is_some() {
                let subject_str = inv_type
                    .get("Guardian")
                    .and_then(|g| g.get("subject_authority"))
                    .and_then(|s| s.as_str())
                    .unwrap_or("00000000-0000-0000-0000-000000000000");
                let subject_uuid = Uuid::parse_str(subject_str).unwrap_or_else(|_| Uuid::new_v4());
                InvitationBridgeType::Guardian {
                    subject_authority: AuthorityId::from_uuid(subject_uuid),
                }
            } else if inv_type.get("Channel").is_some() {
                let block_id = inv_type
                    .get("Channel")
                    .and_then(|c| c.get("block_id"))
                    .and_then(|b| b.as_str())
                    .unwrap_or("home")
                    .to_string();
                InvitationBridgeType::Channel { block_id }
            } else {
                InvitationBridgeType::Contact { nickname: None }
            }
        } else {
            InvitationBridgeType::Contact { nickname: None }
        };

        let now = self.now_ms();
        let info = InvitationInfo {
            invitation_id: invitation_id.clone(),
            sender_id,
            receiver_id: self.authority_id.clone(),
            invitation_type,
            status: InvitationBridgeStatus::Pending,
            created_at_ms: now,
            expires_at_ms,
            message,
        };

        // Store the imported invitation
        let mut invitations = self.invitations.write().await;
        invitations.insert(invitation_id, info.clone());

        Ok(info)
    }

    // =========================================================================
    // Settings
    // =========================================================================

    async fn get_settings(&self) -> SettingsBridgeState {
        let display_name = self.display_name.read().await.clone();
        let mfa_policy = self.mfa_policy.read().await.clone();
        let devices = self.devices.read().await;

        SettingsBridgeState {
            display_name,
            mfa_policy,
            threshold_k: 2,
            threshold_n: 3,
            device_count: devices.len(),
            contact_count: 0,
        }
    }

    async fn list_devices(&self) -> Vec<BridgeDeviceInfo> {
        self.devices.read().await.clone()
    }

    async fn set_display_name(&self, name: &str) -> Result<(), IntentError> {
        *self.display_name.write().await = name.to_string();
        Ok(())
    }

    async fn set_mfa_policy(&self, policy: &str) -> Result<(), IntentError> {
        *self.mfa_policy.write().await = policy.to_string();
        Ok(())
    }

    // =========================================================================
    // Misc
    // =========================================================================

    async fn is_authenticated(&self) -> bool {
        true
    }

    async fn current_time_ms(&self) -> Result<u64, IntentError> {
        // Auto-advance time by 1ms on each call to ensure unique timestamps
        // This is important for message deduplication (message IDs include timestamp)
        let time = self.current_time_ms.fetch_add(1, Ordering::SeqCst);
        Ok(time)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_runtime_bridge_basic() {
        let bridge = MockRuntimeBridge::new();
        assert!(bridge.is_authenticated().await);
        assert!(bridge.has_signing_capability().await);
    }

    #[tokio::test]
    async fn test_mock_invitation_flow() {
        let bridge = MockRuntimeBridge::new();
        let receiver = AuthorityId::new();

        // Create invitation
        let invite = bridge
            .create_contact_invitation(receiver, None, Some("Hello!".to_string()), None)
            .await
            .expect("Should create invitation");

        assert_eq!(invite.status, InvitationBridgeStatus::Pending);

        // Export invitation
        let code = bridge
            .export_invitation(&invite.invitation_id)
            .await
            .expect("Should export");
        assert!(code.starts_with("aura:v1:"));

        // Accept invitation
        bridge
            .accept_invitation(&invite.invitation_id)
            .await
            .expect("Should accept");

        let invitations = bridge.get_invitations().await;
        let updated = invitations.get(&invite.invitation_id).unwrap();
        assert_eq!(updated.status, InvitationBridgeStatus::Accepted);
    }

    #[tokio::test]
    async fn test_mock_fact_commit() {
        let bridge = MockRuntimeBridge::new();

        // Initially empty
        let facts = bridge.get_committed_facts().await;
        assert!(facts.is_empty());

        // Commit some facts
        bridge
            .commit_relational_facts(&[])
            .await
            .expect("Should commit");

        // Facts should be stored (empty in this case, but mechanism works)
        let facts = bridge.get_committed_facts().await;
        assert!(facts.is_empty());
    }

    #[tokio::test]
    async fn test_mock_time_control() {
        let bridge = MockRuntimeBridge::new();

        let t1 = bridge.current_time_ms().await.unwrap();
        bridge.advance_time_ms(1000);
        let t2 = bridge.current_time_ms().await.unwrap();

        assert_eq!(t2 - t1, 1001);

        bridge.set_time_ms(2000000000000);
        let t3 = bridge.current_time_ms().await.unwrap();
        assert_eq!(t3, 2000000000000);
    }
}
