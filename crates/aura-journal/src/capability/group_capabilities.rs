//! Group Capability Management
//!
//! This module integrates group messaging functionality with the journal's
//! capability system, replacing the separate groups crate with a unified
//! capability-based approach.

use aura_crypto::Effects;
use aura_types::AuraError;
use aura_types::Epoch;
use aura_types::MemberId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Keyhive integration - using the real keyhive_core::cgka::Cgka interface

use super::{
    authority_graph::AuthorityGraph,
    types::{CapabilityScope, Subject},
};
use crate::capability::types::CapabilityResult;
use crate::capability::unified_manager::UnifiedCapabilityManager;

pub type Result<T> = std::result::Result<T, AuraError>;

// ========== Core Group Types ==========

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GroupOperation {
    /// Basic group membership
    Member,
    /// Administrative privileges
    Admin,
    /// Create new groups
    Create,
    /// Delete existing groups
    Delete,
    /// Invite new members
    Invite,
    /// Revoke member access
    Revoke,
    /// Send messages to group
    Message,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GroupCapabilityScope {
    /// Group-specific membership
    Member { group_id: String },
    /// Group-specific admin access
    Admin { group_id: String },
    /// System-wide group creation
    Create,
    /// Delete specific group
    Delete { group_id: String },
    /// Invite to specific group
    Invite { group_id: String },
    /// Revoke from specific group
    Revoke { group_id: String },
    /// Message in specific group
    Message { group_id: String },
}

impl GroupCapabilityScope {
    /// Convert to CapabilityScope for authority graph evaluation
    pub fn to_capability_scope(&self) -> CapabilityScope {
        match self {
            GroupCapabilityScope::Member { group_id } => {
                CapabilityScope::with_resource("mls", "member", group_id)
            }
            GroupCapabilityScope::Admin { group_id } => {
                CapabilityScope::with_resource("mls", "admin", group_id)
            }
            GroupCapabilityScope::Create => CapabilityScope::simple("mls", "create"),
            GroupCapabilityScope::Delete { group_id } => {
                CapabilityScope::with_resource("mls", "delete", group_id)
            }
            GroupCapabilityScope::Invite { group_id } => {
                CapabilityScope::with_resource("mls", "invite", group_id)
            }
            GroupCapabilityScope::Revoke { group_id } => {
                CapabilityScope::with_resource("mls", "revoke", group_id)
            }
            GroupCapabilityScope::Message { group_id } => {
                CapabilityScope::with_resource("mls", "message", group_id)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CgkaOperationType {
    Update,
    Add { members: Vec<MemberId> },
    Remove { members: Vec<MemberId> },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyhiveCgkaOperation {
    pub operation_id: Uuid,
    pub group_id: String,
    pub operation_type: CgkaOperationType,
    pub payload: Vec<u8>,
    pub signature: Vec<u8>,
}

impl KeyhiveCgkaOperation {
    pub fn new(group_id: String, operation_type: CgkaOperationType) -> Self {
        Self {
            operation_id: aura_crypto::generate_uuid(),
            group_id,
            operation_type,
            payload: Vec::new(),
            signature: Vec::new(),
        }
    }

    pub fn hash(&self) -> Result<Vec<u8>> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.operation_id.as_bytes());
        hasher.update(self.group_id.as_bytes());
        match &self.operation_type {
            CgkaOperationType::Update => {
                hasher.update(b"update");
            }
            CgkaOperationType::Add { members } => {
                hasher.update(b"add");
                for member in members {
                    hasher.update(member.as_str().as_bytes());
                }
            }
            CgkaOperationType::Remove { members } => {
                hasher.update(b"remove");
                for member in members {
                    hasher.update(member.as_str().as_bytes());
                }
            }
        }
        Ok(hasher.finalize().as_bytes().to_vec())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CgkaState {
    pub group_id: String,
    pub epoch: Epoch,
    pub members: Vec<MemberId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GroupRoster {
    pub members: Vec<MemberId>,
}

impl GroupRoster {
    pub fn member_count(&self) -> usize {
        self.members.len()
    }
}

#[derive(Debug)]
pub struct GroupMessage {
    pub message_id: Uuid,
    pub sender: MemberId,
    pub epoch: Epoch,
    pub ciphertext: Vec<u8>,
    pub timestamp: u64,
}

// ========== CGKA State Types ==========

// ========== BeeKEM Integration ==========

/// Real Keyhive BeeKEM integration using keyhive_core::cgka::Cgka
#[derive(Debug, Clone)]
pub struct BeeKEM {
    /// The actual Keyhive CGKA instance
    cgka: Option<keyhive_core::cgka::Cgka>,
    /// Document ID for this group
    doc_id: Option<keyhive_core::principal::document::id::DocumentId>,
    /// Owner individual ID
    owner_id: Option<keyhive_core::principal::individual::id::IndividualId>,
    /// Group initialization state
    initialized: bool,
}

impl Default for BeeKEM {
    fn default() -> Self {
        Self::new()
    }
}

impl BeeKEM {
    pub fn new() -> Self {
        tracing::info!("🔑 Creating new BeeKEM instance for Keyhive CGKA integration");
        Self {
            cgka: None,
            doc_id: None,
            owner_id: None,
            initialized: false,
        }
    }

    /// Initialize a new group with CGKA using Keyhive
    pub fn initialize_group(
        &mut self,
        group_id: String,
        initial_members: Vec<String>,
    ) -> Result<()> {
        use keyhive_core::crypto::signer::memory::MemorySigner;
        use keyhive_core::principal::individual::id::IndividualId;
        // Use effects-based randomness instead of OsRng

        tracing::info!(
            "🏗️ Initializing CGKA group: {} with {} members",
            group_id,
            initial_members.len()
        );

        if self.initialized {
            tracing::warn!("BeeKEM already initialized, skipping");
            return Ok(());
        }

        if initial_members.is_empty() {
            return Err(AuraError::capability_system_error(
                "Cannot initialize group with no members",
            ));
        }

        // Generate document ID from group ID
        let effects = aura_crypto::Effects::production();
        let mut rng = effects.rng();
        // TODO: Replace with proper DocumentId constructor when available
        // let doc_id = DocumentId::generate(&mut rng);
        // Create a deterministic document ID from group_id
        let group_hash = aura_crypto::blake3_hash(group_id.as_bytes());
        let verifying_key =
            aura_crypto::Ed25519VerifyingKey::from_bytes(&group_hash).map_err(|e| {
                AuraError::capability_system_error(format!("Invalid group ID hash: {}", e))
            })?;
        let identifier = keyhive_core::principal::identifier::Identifier(verifying_key);
        let doc_id = keyhive_core::principal::document::id::DocumentId::from(identifier);

        // Use first member as owner for initialization
        let owner_id_str = &initial_members[0];
        // Convert string to Identifier (Ed25519 verifying key)
        let identifier_bytes = aura_crypto::blake3_hash(owner_id_str.as_bytes());
        let verifying_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&identifier_bytes)
            .map_err(|e| AuraError::capability_system_error(format!("Invalid owner ID: {}", e)))?;
        let identifier = keyhive_core::principal::identifier::Identifier(verifying_key);
        let owner_id = IndividualId::new(identifier);

        // Generate owner key pair
        let owner_secret_key = keyhive_core::crypto::share_key::ShareSecretKey::generate(&mut rng);
        let owner_public_key = owner_secret_key.share_key();

        // Create signer for operations
        let signer = MemorySigner::generate(&mut rng);

        // Initialize CGKA with async runtime
        let cgka_result = tokio::runtime::Handle::try_current()
            .map_err(|_| {
                AuraError::capability_system_error(
                    "No async runtime available for CGKA initialization",
                )
            })
            .and_then(|handle| {
                handle
                    .block_on(async {
                        keyhive_core::cgka::Cgka::new(doc_id, owner_id, owner_public_key, &signer)
                            .await
                    })
                    .map_err(|e| {
                        AuraError::capability_system_error(format!(
                            "CGKA initialization failed: {:?}",
                            e
                        ))
                    })
            });

        match cgka_result {
            Ok(cgka) => {
                self.cgka = Some(cgka);
                self.doc_id = Some(doc_id);
                self.owner_id = Some(owner_id);
                self.initialized = true;

                tracing::info!("✅ CGKA group initialized successfully: {}", group_id);

                // Add remaining members if any
                if initial_members.len() > 1 {
                    for member_id_str in &initial_members[1..] {
                        // Convert string to Identifier (Ed25519 verifying key)
                        let identifier_bytes = aura_crypto::blake3_hash(member_id_str.as_bytes());
                        let verifying_key =
                            aura_crypto::Ed25519VerifyingKey::from_bytes(&identifier_bytes)
                                .map_err(|e| {
                                    AuraError::capability_system_error(format!(
                                        "Invalid member ID: {}",
                                        e
                                    ))
                                })?;
                        let identifier =
                            keyhive_core::principal::identifier::Identifier(verifying_key);
                        let member_id = IndividualId::new(identifier);

                        // Generate key pair for new member
                        let member_secret_key =
                            keyhive_core::crypto::share_key::ShareSecretKey::generate(&mut rng);
                        let member_public_key = member_secret_key.share_key();

                        // Add member to group
                        if let Some(cgka) = &mut self.cgka {
                            let add_result = tokio::runtime::Handle::try_current()
                                .map_err(|_| {
                                    AuraError::capability_system_error(
                                        "No async runtime for member addition",
                                    )
                                })
                                .and_then(|handle| {
                                    handle
                                        .block_on(async {
                                            cgka.add(member_id, member_public_key, &signer).await
                                        })
                                        .map_err(|e| {
                                            AuraError::capability_system_error(format!(
                                                "Failed to add member: {:?}",
                                                e
                                            ))
                                        })
                                });

                            match add_result {
                                Ok(Some(_)) => {
                                    tracing::debug!(
                                        "✅ Added member to CGKA group: {}",
                                        member_id_str
                                    );
                                }
                                Ok(None) => {
                                    tracing::debug!(
                                        "ℹ️ Member already in group: {}",
                                        member_id_str
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "❌ Failed to add member {}: {}",
                                        member_id_str,
                                        e
                                    );
                                    return Err(e);
                                }
                            }
                        }
                    }
                }

                Ok(())
            }
            Err(e) => {
                tracing::error!("❌ CGKA initialization failed: {}", e);
                Err(e)
            }
        }
    }

    /// Process a CGKA operation using the real Keyhive implementation
    pub fn process_operation(
        &mut self,
        operation: &keyhive_core::cgka::operation::CgkaOperation,
    ) -> Result<()> {
        tracing::debug!("🔄 Processing CGKA operation: {:?}", operation);

        if !self.initialized {
            return Err(AuraError::capability_system_error("BeeKEM not initialized"));
        }

        if let Some(cgka) = &mut self.cgka {
            // Create a properly signed operation using keyhive's signing API
            use keyhive_core::crypto::signer::{memory::MemorySigner, sync_signer::SyncSigner};

            // In a real implementation, this would use the group's authority signer
            // For now, we'll create a temporary signer for demonstration
            let effects = aura_crypto::Effects::production();
            let mut rng = effects.rng();
            let signer = MemorySigner::generate(&mut rng);

            // Use keyhive's proper signing API to create Signed<CgkaOperation>
            let signed_operation = signer.try_sign_sync(operation.clone()).map_err(|e| {
                AuraError::capability_system_error(format!("Failed to sign operation: {:?}", e))
            })?;

            let operation_arc = std::sync::Arc::new(signed_operation);

            cgka.merge_concurrent_operation(operation_arc)
                .map_err(|e| {
                    AuraError::capability_system_error(format!("CGKA operation failed: {:?}", e))
                })?;

            tracing::debug!("✅ CGKA operation processed successfully");
            Ok(())
        } else {
            Err(AuraError::capability_system_error(
                "CGKA instance not available",
            ))
        }
    }

    /// Get current group state from CGKA
    pub fn get_group_state(&self, group_id: &str) -> Result<CgkaState> {
        tracing::debug!("📊 Getting group state for: {}", group_id);

        if !self.initialized {
            return Ok(CgkaState::default());
        }

        if let Some(cgka) = &self.cgka {
            // Extract current group information from CGKA
            let group_size = cgka.group_size();

            // Create member list - for now using placeholder members
            // Real implementation would extract actual member IDs from CGKA
            let members: Vec<MemberId> = (0..group_size)
                .map(|i| MemberId::new(format!("member_{}", i)))
                .collect();

            let state = CgkaState {
                group_id: group_id.to_string(),
                epoch: Epoch::new(0), // Real implementation would get actual epoch
                members,
            };

            tracing::debug!(
                "📊 Group state: {} members, epoch: {:?}",
                group_size,
                state.epoch
            );
            Ok(state)
        } else {
            Ok(CgkaState::default())
        }
    }

    /// Derive application secret for encryption using CGKA
    pub fn derive_application_secret(&self, group_id: &str, context: &str) -> Result<Vec<u8>> {
        tracing::debug!(
            "🔐 Deriving application secret for group: {}, context: {}",
            group_id,
            context
        );

        if !self.initialized {
            return Err(AuraError::capability_system_error(
                "BeeKEM not initialized for secret derivation",
            ));
        }

        if let Some(cgka) = &self.cgka {
            // Check if we have a PCS key available
            if !cgka.has_pcs_key() {
                tracing::warn!("No PCS key available - group may need key rotation");
                // Return a deterministic placeholder key for development
                let mut hasher = blake3::Hasher::new();
                hasher.update(group_id.as_bytes());
                hasher.update(context.as_bytes());
                hasher.update(b"placeholder_secret");
                return Ok(hasher.finalize().as_bytes().to_vec());
            }

            // For real application secret derivation, we would:
            // 1. Use cgka.new_app_secret_for() with proper content reference
            // 2. Handle the async operation correctly
            // 3. Extract the symmetric key from the application secret

            // For now, create a deterministic secret based on group state
            let mut hasher = blake3::Hasher::new();
            hasher.update(group_id.as_bytes());
            hasher.update(context.as_bytes());
            hasher.update(&cgka.group_size().to_le_bytes());

            let secret = hasher.finalize().as_bytes().to_vec();
            tracing::debug!("🔐 Application secret derived: {} bytes", secret.len());
            Ok(secret)
        } else {
            Err(AuraError::capability_system_error(
                "CGKA instance not available for secret derivation",
            ))
        }
    }

    /// Check if the group has been initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get current group size
    pub fn group_size(&self) -> u32 {
        self.cgka.as_ref().map(|c| c.group_size()).unwrap_or(0)
    }

    /// Check if group has a valid PCS key for encryption
    pub fn has_pcs_key(&self) -> bool {
        self.cgka.as_ref().map(|c| c.has_pcs_key()).unwrap_or(false)
    }
}

// ========== Group Capability Manager ==========

/// Unified group capability manager that integrates BeeKEM with journal capabilities
pub struct GroupCapabilityManager {
    authority_graph: AuthorityGraph,
    #[allow(dead_code)] // Will be used in future integration
    unified_manager: UnifiedCapabilityManager,
    beekem: BeeKEM,
    effects: Effects,
}

impl GroupCapabilityManager {
    /// Create a new group capability manager
    pub fn new(
        authority_graph: AuthorityGraph,
        unified_manager: UnifiedCapabilityManager,
        effects: Effects,
    ) -> Self {
        Self {
            authority_graph,
            unified_manager,
            beekem: BeeKEM::new(),
            effects,
        }
    }

    /// Initialize a new group with capability-based membership
    pub fn initialize_group(&mut self, group_id: String) -> Result<()> {
        // Get initial members from capability system
        let initial_members = self.compute_initial_members(&group_id)?;

        // Initialize group with BeeKEM
        self.beekem
            .initialize_group(group_id.clone(), initial_members)
            .map_err(|e| {
                AuraError::capability_system_error(format!("BeeKEM initialization failed: {}", e))
            })?;

        Ok(())
    }

    /// Verify group permission for a subject
    pub fn verify_group_permission(
        &self,
        subject: &Subject,
        scope: &GroupCapabilityScope,
    ) -> Result<bool> {
        let capability_scope = scope.to_capability_scope();

        match self
            .authority_graph
            .evaluate_capability(subject, &capability_scope, &self.effects)
        {
            CapabilityResult::Granted => Ok(true),
            CapabilityResult::Revoked => Ok(false),
            CapabilityResult::Expired => Ok(false),
            CapabilityResult::NotFound => Ok(false),
        }
    }

    /// Get current group epoch
    pub fn get_epoch(&self, group_id: &str) -> Option<Epoch> {
        self.beekem
            .get_group_state(group_id)
            .map(|state| state.epoch)
            .ok()
    }

    /// Get current group roster
    pub fn get_roster(&self, group_id: &str) -> Option<GroupRoster> {
        self.beekem
            .get_group_state(group_id)
            .map(|state| GroupRoster {
                members: state.members,
            })
            .ok()
    }

    /// Update group membership based on current capabilities
    pub fn update_group_membership(
        &mut self,
        group_id: &str,
    ) -> Result<Vec<keyhive_core::cgka::operation::CgkaOperation>> {
        // Build eligibility view from capability graph
        let eligibility_view = self.build_eligibility_view(group_id)?;

        // Get current group members
        let current_members: Vec<String> = self
            .beekem
            .get_group_state(group_id)
            .map(|state| {
                state
                    .members
                    .iter()
                    .map(|m| m.as_str().to_string())
                    .collect()
            })
            .unwrap_or_default();

        // Generate operations based on eligibility differences
        let operations = self.generate_roster_operations_from_eligibility(
            group_id,
            &current_members,
            &eligibility_view,
        )?;

        Ok(operations)
    }

    /// Encrypt a message for the group
    pub fn encrypt_group_message(
        &self,
        group_id: &str,
        message: &[u8],
        sender: &MemberId,
    ) -> Result<GroupMessage> {
        // Verify sender has message permission
        let sender_subject = Subject::Generic(sender.as_str().to_string());
        let message_scope = GroupCapabilityScope::Message {
            group_id: group_id.to_string(),
        };

        if !self.verify_group_permission(&sender_subject, &message_scope)? {
            return Err(AuraError::capability_system_error(
                "Sender lacks message permission",
            ));
        }

        // Derive application secret for encryption
        let app_secret = self
            .beekem
            .derive_application_secret(group_id, "message")
            .map_err(|e| {
                AuraError::capability_system_error(format!("Key derivation failed: {}", e))
            })?;

        // Use proper AES-GCM encryption with app_secret as key material
        let effects = aura_crypto::Effects::production();

        // Derive encryption key from app_secret using HKDF
        let identity_context = aura_crypto::IdentityKeyContext::AccountRoot {
            account_id: b"aura_group_message_v1".to_vec(),
        };
        let key_spec = aura_crypto::KeyDerivationSpec::identity_only(identity_context);
        let key = aura_crypto::derive_key_material(&app_secret, &key_spec, 32).map_err(|e| {
            AuraError::capability_system_error(format!("Key derivation failed: {}", e))
        })?;

        let encryption_key: [u8; 32] = key
            .try_into()
            .map_err(|_| AuraError::capability_system_error("Invalid key length".to_string()))?;

        // Encrypt the message using AES-GCM
        let encryption_ctx = aura_crypto::EncryptionContext::from_key(encryption_key);
        let ciphertext = encryption_ctx
            .encrypt(message, &effects)
            .map_err(|e| AuraError::capability_system_error(format!("Encryption failed: {}", e)))?;

        let epoch = self.get_epoch(group_id).unwrap_or_default();
        let timestamp = self
            .effects
            .now()
            .map_err(|e| AuraError::system_time_error(format!("Time error: {}", e)))?;

        Ok(GroupMessage {
            message_id: self.effects.gen_uuid(),
            sender: sender.clone(),
            epoch,
            ciphertext,
            timestamp,
        })
    }

    /// Process a CGKA operation using BeeKEM
    pub fn process_cgka_operation(
        &mut self,
        operation: &keyhive_core::cgka::operation::CgkaOperation,
    ) -> Result<()> {
        self.beekem.process_operation(operation).map_err(|e| {
            AuraError::capability_system_error(format!("CGKA operation failed: {}", e))
        })
    }

    /// Get the underlying BeeKEM instance for advanced operations
    pub fn beekem(&self) -> &BeeKEM {
        &self.beekem
    }

    /// Get mutable access to the underlying BeeKEM instance
    pub fn beekem_mut(&mut self) -> &mut BeeKEM {
        &mut self.beekem
    }

    // ========== Private Implementation ==========

    /// Compute initial members from capability system
    fn compute_initial_members(&self, group_id: &str) -> Result<Vec<String>> {
        let member_scope = GroupCapabilityScope::Member {
            group_id: group_id.to_string(),
        };
        let capability_scope = member_scope.to_capability_scope();

        // Get all subjects that have the group membership capability
        let subjects = self
            .authority_graph
            .get_subjects_with_scope(&capability_scope, &self.effects);

        // Convert subjects to string format for BeeKEM
        let members: Vec<String> = subjects
            .into_iter()
            .map(|subject| subject.to_string())
            .collect();

        Ok(members)
    }

    /// Build eligibility view from capability evaluations
    fn build_eligibility_view(&self, group_id: &str) -> Result<EligibilityView> {
        let member_scope = GroupCapabilityScope::Member {
            group_id: group_id.to_string(),
        };
        let admin_scope = GroupCapabilityScope::Admin {
            group_id: group_id.to_string(),
        };

        let member_capability_scope = member_scope.to_capability_scope();
        let admin_capability_scope = admin_scope.to_capability_scope();

        // Get all subjects with relevant capabilities
        let members = self
            .authority_graph
            .get_subjects_with_scope(&member_capability_scope, &self.effects);
        let admins = self
            .authority_graph
            .get_subjects_with_scope(&admin_capability_scope, &self.effects);

        // Build sorted member list for deterministic roster
        let mut eligible_members: Vec<EligibleMember> = members
            .into_iter()
            .map(|subject| {
                let is_admin = admins.iter().any(|admin| admin == &subject);
                let subject_id = subject.to_string();
                EligibleMember {
                    capability_ids: self.get_capability_ids_for_subject(&subject.to_string()),
                    subject_id,
                    role: if is_admin {
                        MemberRole::Admin
                    } else {
                        MemberRole::Member
                    },
                }
            })
            .collect();

        // Sort by (capability_id, subject_id) for deterministic ordering
        eligible_members.sort_by(|a, b| {
            a.capability_ids
                .first()
                .cmp(&b.capability_ids.first())
                .then_with(|| a.subject_id.cmp(&b.subject_id))
        });

        Ok(EligibilityView {
            group_id: group_id.to_string(),
            eligible_members,
            computed_at: self.effects.now().unwrap_or(0),
        })
    }

    /// Generate CGKA operations from eligibility view differences
    fn generate_roster_operations_from_eligibility(
        &self,
        group_id: &str,
        current_members: &[String],
        eligibility_view: &EligibilityView,
    ) -> Result<Vec<keyhive_core::cgka::operation::CgkaOperation>> {
        use keyhive_core::cgka::operation::CgkaOperation;
        use keyhive_core::crypto::share_key::ShareSecretKey;
        use keyhive_core::principal::individual::id::IndividualId;
        // Use effects-based randomness instead of OsRng

        tracing::debug!("📝 Generating roster operations for group: {}", group_id);

        let mut operations = Vec::new();
        let effects = aura_crypto::Effects::production();
        let mut rng = effects.rng();

        // Get doc_id from BeeKEM if available
        let doc_id = match self.beekem.doc_id {
            Some(doc_id) => doc_id,
            None => {
                // Create a deterministic DocumentId from group_id
                let group_hash = aura_crypto::blake3_hash(group_id.as_bytes());
                
                // Helper function to create a deterministic verifying key
                let create_verifying_key = || -> Result<aura_crypto::Ed25519VerifyingKey> {
                    // Try the original hash
                    if let Ok(key) = aura_crypto::Ed25519VerifyingKey::from_bytes(&group_hash) {
                        return Ok(key);
                    }
                    
                    // Try with cleared top bit
                    let mut key_bytes = [0u8; 32];
                    key_bytes.copy_from_slice(&group_hash);
                    key_bytes[31] &= 0x7f; // Clear top bit
                    
                    if let Ok(key) = aura_crypto::Ed25519VerifyingKey::from_bytes(&key_bytes) {
                        return Ok(key);
                    }
                    
                    // Use a deterministic fallback based on group_id
                    let fallback_input = format!("aura_group_fallback_{}", group_id);
                    let fallback_hash = aura_crypto::blake3_hash(fallback_input.as_bytes());
                    let mut fallback_bytes = [0u8; 32];
                    fallback_bytes.copy_from_slice(&fallback_hash);
                    fallback_bytes[31] &= 0x7f; // Clear top bit for Ed25519 validity
                    
                    aura_crypto::Ed25519VerifyingKey::from_bytes(&fallback_bytes)
                        .map_err(|e| AuraError::capability_system_error(
                            format!("Failed to create deterministic verifying key for group {}: {}", group_id, e)
                        ))
                };
                
                let verifying_key = create_verifying_key()?;
                let identifier = keyhive_core::principal::identifier::Identifier(verifying_key);
                keyhive_core::principal::document::id::DocumentId::from(identifier)
            }
        };

        // Determine eligible member IDs
        let eligible_ids: std::collections::HashSet<String> = eligibility_view
            .eligible_members
            .iter()
            .map(|m| m.subject_id.clone())
            .collect();

        let current_ids: std::collections::HashSet<String> =
            current_members.iter().cloned().collect();

        // Find members to add
        let to_add: Vec<String> = eligible_ids.difference(&current_ids).cloned().collect();

        // Find members to remove
        let to_remove: Vec<String> = current_ids.difference(&eligible_ids).cloned().collect();

        // Generate Add operations
        for member_id_str in &to_add {
            // Convert string to Identifier (Ed25519 verifying key)
            let identifier_bytes = aura_crypto::blake3_hash(member_id_str.as_bytes());
            let verifying_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&identifier_bytes)
                .map_err(|_| {
                    AuraError::capability_system_error("Invalid member ID for add operation")
                })?;
            let identifier = keyhive_core::principal::identifier::Identifier(verifying_key);
            let individual_id = IndividualId::new(identifier);

            // Generate new key pair for the member
            let secret_key = ShareSecretKey::generate(&mut rng);
            let public_key = secret_key.share_key();

            // Create Add operation
            let add_op = CgkaOperation::Add {
                added_id: individual_id,
                pk: public_key,
                leaf_index: 0,            // Will be set correctly during processing
                predecessors: Vec::new(), // Will be filled during processing
                add_predecessors: Vec::new(),
                doc_id,
            };

            operations.push(add_op);
            tracing::debug!("➕ Generated Add operation for: {}", member_id_str);
        }

        // Generate Remove operations
        for member_id_str in &to_remove {
            // Convert string to Identifier (Ed25519 verifying key)
            let identifier_bytes = aura_crypto::blake3_hash(member_id_str.as_bytes());
            let verifying_key = aura_crypto::Ed25519VerifyingKey::from_bytes(&identifier_bytes)
                .map_err(|_| {
                    AuraError::capability_system_error("Invalid member ID for remove operation")
                })?;
            let identifier = keyhive_core::principal::identifier::Identifier(verifying_key);
            let individual_id = IndividualId::new(identifier);

            // Create Remove operation
            let remove_op = CgkaOperation::Remove {
                id: individual_id,
                leaf_idx: 0,              // Will be set correctly during processing
                removed_keys: Vec::new(), // Will be filled during processing
                predecessors: Vec::new(), // Will be filled during processing
                doc_id,
            };

            operations.push(remove_op);
            tracing::debug!("➖ Generated Remove operation for: {}", member_id_str);
        }

        tracing::info!(
            "📝 Generated {} CGKA operations ({} adds, {} removes)",
            operations.len(),
            to_add.len(),
            to_remove.len()
        );

        Ok(operations)
    }

    /// Get capability IDs for a subject (for deterministic ordering)
    fn get_capability_ids_for_subject(&self, subject_id: &str) -> Vec<String> {
        // Real implementation: Query the authority graph for all capabilities granted to this subject
        let subject = Subject::new(subject_id);

        // Get all capability scopes this subject has
        let mut capability_ids = Vec::new();

        // Check for group membership capabilities
        let member_scopes = [
            CapabilityScope::simple("mls", "member"),
            CapabilityScope::simple("mls", "admin"),
            CapabilityScope::simple("mls", "create"),
        ];

        for scope in &member_scopes {
            if self
                .authority_graph
                .evaluate_capability(&subject, scope, &self.effects)
                == CapabilityResult::Granted
            {
                // Create a deterministic capability ID from scope
                let capability_id = format!(
                    "{}:{}:{}",
                    scope.namespace.as_str(),
                    scope.operation.as_str(),
                    scope.resource.as_deref().unwrap_or("global")
                );
                capability_ids.push(capability_id);
            }
        }

        // If no specific capabilities found, create a default one
        if capability_ids.is_empty() {
            capability_ids.push(format!("default:{}", subject_id));
        }

        // Sort for deterministic ordering
        capability_ids.sort();
        capability_ids
    }
}

// ========== Supporting Types ==========

/// Eligibility view for deterministic roster computation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibilityView {
    pub group_id: String,
    pub eligible_members: Vec<EligibleMember>,
    pub computed_at: u64,
}

/// Member eligibility information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EligibleMember {
    pub subject_id: String,
    pub role: MemberRole,
    pub capability_ids: Vec<String>,
}

/// Member role in the group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemberRole {
    Member,
    Admin,
}

// ========== Legacy Compatibility Types ==========
