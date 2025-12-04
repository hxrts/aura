#![allow(
    clippy::clone_on_copy,
    clippy::needless_borrow,
    clippy::for_kv_map,
    clippy::useless_vec
)]

//! # Effect Command Dispatch
//!
//! Handles the execution of effect commands in the TUI effect bridge.
//! This module contains the core command routing and authorization logic,
//! extracted from bridge.rs for maintainability.
//!
//! ## Architecture
//!
//! Command execution follows this flow:
//! 1. **Authorization Check**: Verify user has required permission level
//! 2. **Command Routing**: Match on EffectCommand variant and execute
//! 3. **Event Emission**: Broadcast AuraEvent to notify subscribers
//! 4. **Error Handling**: Return Result for retry logic
//!
//! ## Current Status
//!
//! This implementation emits deterministic simulated events for offline TUI development.
//! It will be integrated with real effect handlers in future phases.
//!
//! ## Integration Path
//!
//! To wire this to actual Aura effect handlers:
//! 1. Add EffectContext to function signatures
//! 2. Replace simulated implementations with real effect calls
//! 3. Wire effect handlers to emit events through event_tx
//! 4. Add proper Biscuit capability checking via CapGuard
//!
//! See docs/106_effect_system_and_runtime.md for effect system architecture.
//! See crates/aura-protocol/src/guards/ for capability checking.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::ids;
use crate::tui::effects::bridge::{frost_sign_tree_op, frost_sign_tree_op_with_keys};
use crate::tui::effects::command_parser::{AuraEvent, CommandAuthorizationLevel, EffectCommand};
use aura_app::{Intent, IntentChannelType};
use aura_core::effects::{
    amp::{
        AmpChannelEffects, ChannelCloseParams, ChannelCreateParams, ChannelJoinParams,
        ChannelLeaveParams, ChannelSendParams,
    },
    storage::{StorageEffects, StorageError},
    time::PhysicalTimeEffects,
};
use aura_core::hash::{self, hash};
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_journal::semilattice::{InvitationRecordRegistry, InvitationStatus};
use aura_journal::DomainFact;
use aura_protocol::amp::AmpJournalEffects;
use aura_protocol::effects::SyncEffects;
use aura_protocol::moderation::facts::{
    BlockBanFact, BlockKickFact, BlockMuteFact, BlockUnbanFact, BlockUnmuteFact,
};
use aura_relational::RelationalContext;
use tokio::sync::{broadcast, RwLock};

// ============================================================================
// Internal State Types (Shared with bridge.rs)
// ============================================================================
// These types support the current demo implementation.
// They will be refactored when integrating with the real journal system.

/// Block invitation envelope for tracking block membership invitations
#[derive(Debug, Clone)]
pub(super) struct BlockInvitationEnvelope {
    /// Unique invitation identifier
    pub invitation_id: String,
    /// Block being invited to
    pub block_id: String,
    /// Block name (for display)
    pub block_name: Option<String>,
    /// Authority of the inviter (steward)
    pub inviter_authority: aura_core::AuthorityId,
    /// Authority of the invitee
    pub invitee_authority: aura_core::AuthorityId,
    /// When the invitation was created (ms since epoch)
    pub created_at: u64,
    /// When the invitation expires (ms since epoch)
    pub expires_at: u64,
}

/// Ban record for persistent moderation
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct BanRecord {
    /// Banned user authority ID
    pub authority_id: String,
    /// Reason for ban
    pub reason: String,
    /// Actor who issued the ban
    pub actor: String,
    /// Timestamp when ban was issued (ms since epoch)
    pub banned_at: u64,
}

/// Mute record for persistent moderation with expiration
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct MuteRecord {
    /// Muted user authority ID
    pub authority_id: String,
    /// Mute duration in seconds (None = permanent)
    pub duration_secs: Option<u64>,
    /// Timestamp when mute was issued (ms since epoch)
    pub muted_at: u64,
    /// Timestamp when mute expires (ms since epoch, None = permanent)
    pub expires_at: Option<u64>,
    /// Actor who issued the mute
    pub actor: String,
}

impl MuteRecord {
    /// Check if this mute has expired
    #[allow(dead_code)]
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        match self.expires_at {
            Some(expiry) => current_time_ms >= expiry,
            None => false, // Permanent mute never expires
        }
    }
}

/// Kick log entry for audit trail
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct KickRecord {
    /// Kicked user authority ID
    pub authority_id: String,
    /// Channel from which user was kicked
    pub channel: String,
    /// Reason for kick
    pub reason: String,
    /// Actor who issued the kick
    pub actor: String,
    /// Timestamp when kick occurred (ms since epoch)
    pub kicked_at: u64,
}

/// Block resident state for tracking members
#[derive(Debug, Clone)]
pub(super) struct ResidentState {
    /// Authority ID of the resident
    pub authority_id: aura_core::AuthorityId,
    /// Display name (petname)
    #[allow(dead_code)]
    pub name: String,
    /// Whether this resident is a steward
    pub is_steward: bool,
    /// When the resident joined (ms since epoch)
    #[allow(dead_code)]
    pub joined_at: u64,
    /// Storage allocated by this resident
    #[allow(dead_code)]
    pub storage_allocated: u64,
}

/// Block state for internal tracking
#[derive(Debug, Clone)]
pub(super) struct BlockState {
    /// Block identifier
    #[allow(dead_code)]
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// When the block was created (ms since epoch)
    #[allow(dead_code)]
    pub created_at: u64,
    /// Residents of the block
    pub residents: Vec<ResidentState>,
    /// Total storage budget in bytes (default 10MB)
    pub storage_budget: u64,
    /// Storage used in bytes
    pub storage_used: u64,
    /// Whether this is the user's primary block
    pub is_primary: bool,
    /// Channel topic (optional)
    pub topic: Option<String>,
    /// Pinned messages (message IDs)
    pub pinned_messages: Vec<String>,
    /// Channel mode flags (e.g., "moderated", "invite-only")
    pub mode_flags: Option<String>,
    /// Persistent ban list (keyed by authority ID)
    pub ban_list: HashMap<String, BanRecord>,
    /// Persistent mute list with expiration (keyed by authority ID)
    pub mute_list: HashMap<String, MuteRecord>,
    /// Kick log for audit trail
    pub kick_log: Vec<KickRecord>,
    /// Relational context identifier for journal integration
    #[allow(dead_code)]
    pub context_id: aura_core::ContextId,
}

impl BlockState {
    /// Default storage limit: 10 MB
    pub const DEFAULT_STORAGE_BUDGET: u64 = 10 * 1024 * 1024;
    /// Default resident allocation: 200 KB
    pub const RESIDENT_ALLOCATION: u64 = 200 * 1024;

    /// Create a new block with the creator as steward
    pub fn new(
        id: String,
        name: Option<String>,
        creator_authority: aura_core::AuthorityId,
        created_at: u64,
    ) -> Self {
        let steward = ResidentState {
            authority_id: creator_authority,
            name: "You".to_string(), // Default petname for self
            is_steward: true,
            joined_at: created_at,
            storage_allocated: Self::RESIDENT_ALLOCATION,
        };

        let context_entropy = hash(id.as_bytes());

        Self {
            id,
            name,
            created_at,
            residents: vec![steward],
            storage_budget: Self::DEFAULT_STORAGE_BUDGET,
            storage_used: Self::RESIDENT_ALLOCATION, // Steward's initial allocation
            is_primary: true,                        // First block is primary by default
            topic: None,
            pinned_messages: Vec::new(),
            mode_flags: None,
            ban_list: HashMap::new(),
            mute_list: HashMap::new(),
            kick_log: Vec::new(),
            context_id: aura_core::ContextId::new_from_entropy(context_entropy),
        }
    }

    /// Add a new resident to the block
    pub fn add_resident(
        &mut self,
        authority_id: aura_core::AuthorityId,
        name: String,
        joined_at: u64,
    ) -> Result<(), String> {
        // Check if resident already exists
        if self
            .residents
            .iter()
            .any(|r| r.authority_id == authority_id)
        {
            return Err("Resident already exists in block".to_string());
        }

        // Check storage capacity
        let new_allocation = Self::RESIDENT_ALLOCATION;
        if self.storage_used + new_allocation > self.storage_budget {
            return Err("Insufficient storage budget for new resident".to_string());
        }

        self.residents.push(ResidentState {
            authority_id,
            name,
            is_steward: false,
            joined_at,
            storage_allocated: new_allocation,
        });
        self.storage_used += new_allocation;

        Ok(())
    }
}

/// FROST threshold signing state for the current device.
///
/// Stores the key packages needed for threshold signing operations.
/// In a multi-device scenario, each device holds its own key package
/// and the shared public key package for verification.
#[derive(Debug, Clone, Default)]
pub(super) struct FrostState {
    /// This device's key package (index 0 in a 1-of-1 bootstrap, or device-specific share)
    /// Serialized frost_ed25519 KeyPackage
    pub device_key_package: Option<Vec<u8>>,
    /// Group public key package for signature aggregation and verification
    /// Shared across all devices in the threshold group
    pub public_key_package: Option<Vec<u8>>,
    /// Current threshold configuration (e.g., 1-of-1 for bootstrap, 2-of-3 for guardians)
    /// Used when coordinating multi-device signing ceremonies
    #[allow(dead_code)]
    pub threshold: u16,
    /// Total number of signers in the threshold group
    /// Used when coordinating multi-device signing ceremonies
    #[allow(dead_code)]
    pub max_signers: u16,
}

/// Traversal position in neighborhood
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct TraversalPosition {
    /// Neighborhood ID
    pub neighborhood_id: String,
    /// Block ID within neighborhood
    pub block_id: String,
    /// Traversal depth (Street, Frontage, Interior)
    pub depth: String,
}

/// Stored capability token entry with metadata
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoredToken {
    /// Serialized Biscuit token bytes
    pub token_bytes: Vec<u8>,
    /// Role associated with this token (e.g., "steward", "member")
    pub role: String,
    /// Capabilities granted by this token
    pub capabilities: Vec<String>,
    /// When the token was stored (ms since epoch)
    pub stored_at: u64,
    /// Optional expiration time (ms since epoch)
    pub expires_at: Option<u64>,
}

/// In-memory capability token store with optional file persistence.
///
/// Stores Biscuit tokens keyed by user ID for authorization within blocks.
/// Tokens are created during GrantSteward/RevokeSteward operations and
/// verified at command execution time.
///
/// ## Storage Model
///
/// - In-memory HashMap for fast lookup
/// - Optional JSON file persistence for durability across restarts
/// - Tokens are scoped per-user (block membership handled separately)
///
/// ## Integration Path
///
/// Future versions will integrate with:
/// - RelationalContext for cross-device token synchronization
/// - Journal facts for auditable capability changes
/// - Biscuit verification in guard chain
#[derive(Debug, Clone, Default)]
pub struct CapabilityTokenStore {
    /// Tokens keyed by user ID
    tokens: HashMap<String, StoredToken>,
}

#[allow(dead_code)]
impl CapabilityTokenStore {
    const STORAGE_KEY: &'static str = "tui.capability_tokens";

    /// Create a new in-memory token store
    pub fn new() -> Self {
        Self {
            tokens: HashMap::new(),
        }
    }

    /// Store a capability token for a user
    pub fn store_token(
        &mut self,
        user_id: &str,
        token_bytes: Vec<u8>,
        role: &str,
        capabilities: Vec<String>,
        stored_at: u64,
        expires_at: Option<u64>,
    ) {
        let entry = StoredToken {
            token_bytes,
            role: role.to_string(),
            capabilities,
            stored_at,
            expires_at,
        };
        self.tokens.insert(user_id.to_string(), entry);
    }

    /// Get a stored token for a user
    pub fn get_token(&self, user_id: &str) -> Option<&StoredToken> {
        self.tokens.get(user_id)
    }

    /// Persist tokens through storage effects.
    pub async fn persist<S: StorageEffects + ?Sized>(
        &self,
        storage: &S,
    ) -> Result<(), StorageError> {
        let contents = serde_json::to_vec_pretty(&self.tokens)
            .map_err(|e| StorageError::WriteFailed(format!("failed to serialize tokens: {e}")))?;
        storage.store(Self::STORAGE_KEY, contents).await
    }

    /// Load tokens via storage effects.
    pub async fn load<S: StorageEffects + ?Sized>(
        &mut self,
        storage: &S,
    ) -> Result<(), StorageError> {
        if let Some(bytes) = storage.retrieve(Self::STORAGE_KEY).await? {
            self.tokens = serde_json::from_slice(&bytes).map_err(|e| {
                StorageError::ReadFailed(format!("failed to deserialize tokens: {e}"))
            })?;
        }
        Ok(())
    }

    /// Remove a token for a user
    pub fn remove_token(&mut self, user_id: &str) -> Option<StoredToken> {
        let removed = self.tokens.remove(user_id);
        removed
    }

    /// List all stored user IDs
    pub fn list_users(&self) -> Vec<&String> {
        self.tokens.keys().collect()
    }

    /// Check if a user has a stored token
    pub fn has_token(&self, user_id: &str) -> bool {
        self.tokens.contains_key(user_id)
    }

    /// Check if a user has a specific capability
    pub fn has_capability(&self, user_id: &str, capability: &str) -> bool {
        self.tokens
            .get(user_id)
            .is_some_and(|t| t.capabilities.iter().any(|c| c == capability))
    }

    /// Get token count
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Check if store is empty
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// Status of a guardian invitation
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardianInvitationStatus {
    /// Invitation is pending response
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was declined
    Declined,
    /// Invitation expired
    Expired,
}

/// A guardian invitation record
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GuardianInvitation {
    /// Unique invitation ID
    pub invitation_id: String,
    /// Contact ID if specified
    pub contact_id: Option<String>,
    /// When the invitation was created
    pub created_at: u64,
    /// When the invitation expires
    pub expires_at: u64,
    /// Current status of the invitation
    pub status: GuardianInvitationStatus,
}

/// Internal state for the effect bridge
pub(super) struct BridgeState {
    /// Whether the bridge is connected
    pub connected: bool,
    /// Number of pending commands
    pub pending_commands: u32,
    /// Last error message
    pub last_error: Option<String>,
    /// Active recovery session (Phase 3 state management)
    pub recovery_session: Option<crate::tui::recovery_session::RecoverySession>,
    /// Current account authority (after CreateAccount)
    pub account_authority: Option<aura_core::AuthorityId>,
    /// Whether MFA (m > 1) is required for sensitive ops (UI mirror of threshold policy)
    #[allow(dead_code)]
    pub require_mfa: bool,
    /// Current user's maximum authorized level
    /// Updated when user authenticates or gains capabilities
    pub user_auth_level: CommandAuthorizationLevel,
    /// User's Biscuit token for cryptographic authorization (Phase 4)
    ///
    /// - None: Demo mode using simplified auth levels (CommandAuthorizationLevel)
    /// - Some(token): Production mode with real Biscuit token verification
    ///
    /// Used with ProtocolGuard for guard chain authorization:
    /// - Demo: Use ProtocolGuard::new_for_testing() to bypass token verification
    /// - Prod: Use ProtocolGuard::new() with real token for Biscuit policy enforcement
    #[allow(dead_code)]
    pub user_token: Option<biscuit_auth::Biscuit>,
    /// FROST threshold signing state
    pub frost_state: FrostState,
    /// Invitation record registry for tracking invitation status
    /// NOTE: This duplicates AppCore.view_state.invitations but is kept for demo implementations
    pub invitation_registry: InvitationRecordRegistry,
    /// Blocks created or joined by the user (keyed by block_id)
    /// NOTE: This duplicates AppCore.view_state.block but is kept for demo implementations
    pub blocks: HashMap<String, BlockState>,
    /// Currently active block (if any)
    /// NOTE: This duplicates AppCore selection state but is kept for demo implementations
    pub current_block: Option<String>,
    /// Pending block invitations received (keyed by invitation_id)
    /// NOTE: This should be migrated to AppCore.view_state.invitations
    pub pending_block_invitations: HashMap<String, BlockInvitationEnvelope>,
    /// Sent block invitations (keyed by invitation_id)
    /// NOTE: This should be migrated to AppCore.view_state.invitations
    pub sent_block_invitations: HashMap<String, BlockInvitationEnvelope>,
    /// User's current nickname/display name (local preference)
    pub user_nickname: Option<String>,
    /// Current traversal position in neighborhood (Section 9)
    pub traversal_position: Option<TraversalPosition>,
    /// Current context ID for AMP channel operations
    /// Set when user selects a channel or context
    pub current_context: Option<aura_core::ContextId>,
    /// Capability token store for per-user Biscuit tokens
    /// Used for persistent authorization across sessions
    pub capability_token_store: CapabilityTokenStore,
    /// Tracks whether tokens have already been loaded
    pub tokens_loaded: bool,
    /// Known peers for sync operations
    /// Populated via AddPeer command or peer discovery
    pub known_peers: Vec<uuid::Uuid>,
    // NOTE: contacts and guardian_invitations removed - use AppCore.view_state instead
}

impl Default for BridgeState {
    fn default() -> Self {
        Self {
            connected: false,
            pending_commands: 0,
            last_error: None,
            recovery_session: None,
            account_authority: None,
            require_mfa: false,
            // Start with Basic level - user is authenticated but not elevated
            // Non-demo mode starts at Public and upgrades after auth
            user_auth_level: CommandAuthorizationLevel::Basic,
            // Start with no Biscuit token (demo mode)
            // Production: Set via authentication flow with real token from auth ceremony
            user_token: None,
            frost_state: FrostState::default(),
            invitation_registry: InvitationRecordRegistry::new(),
            blocks: HashMap::new(),
            current_block: None,
            pending_block_invitations: HashMap::new(),
            sent_block_invitations: HashMap::new(),
            user_nickname: None,
            traversal_position: None,
            // Start with default context; set when user selects a channel
            current_context: Some(aura_core::ContextId::from_uuid(crate::ids::uuid(
                "tui:default-context",
            ))),
            // Start with in-memory token store; persistence can be added via with_persistence
            capability_token_store: CapabilityTokenStore::new(),
            tokens_loaded: false,
            // Start with no known peers; add via AddPeer command or discovery
            known_peers: Vec::new(),
        }
    }
}

// ============================================================================
// Authorization Check
// ============================================================================

/// Check if the user is authorized to execute a command
///
/// This function implements a simple authorization level system for demo purposes.
/// Full authorization integrates with Biscuit tokens and CapGuard.
///
/// # Arguments
/// * `command` - The command to authorize
/// * `user_level` - The user's current authorization level
/// * `event_tx` - Channel to emit authorization denied events
///
/// # Returns
/// * `Ok(())` if authorized
/// * `Err(String)` if authorization denied
pub fn check_authorization(
    command: &EffectCommand,
    user_level: CommandAuthorizationLevel,
    event_tx: &broadcast::Sender<AuraEvent>,
) -> Result<(), String> {
    let required_level = command.authorization_level();

    // Public commands always pass
    if required_level == CommandAuthorizationLevel::Public {
        return Ok(());
    }

    // Check if user's level is sufficient
    if user_level >= required_level {
        tracing::debug!(
            "Authorization check passed: user has {:?}, command requires {:?}",
            user_level,
            required_level
        );
        return Ok(());
    }

    // Authorization denied - emit event and return error
    let command_name = format!("{:?}", command);
    // Truncate command name for display (first 50 chars or first word)
    let command_display = command_name
        .split(|c: char| c.is_whitespace() || c == '{')
        .next()
        .unwrap_or(&command_name)
        .to_string();

    let reason = format!(
        "User has {} authorization, but {} requires {}",
        user_level.description(),
        command_display,
        required_level.description()
    );

    tracing::warn!(
        "Authorization denied for {:?}: user has {:?}, requires {:?}",
        command_display,
        user_level,
        required_level
    );

    // Emit authorization denied event
    let _ = event_tx.send(AuraEvent::AuthorizationDenied {
        command: command_display.clone(),
        required_level,
        reason: reason.clone(),
    });

    Err(format!("Authorization denied: {}", reason))
}

// ============================================================================
// Command Execution
// ============================================================================

/// Execute a single effect command
///
/// This is the core command dispatch function that routes EffectCommand variants
/// to their appropriate implementations. It handles state updates, event emission,
/// and error reporting.
///
/// # Arguments
/// * `command` - The command to execute
/// * `event_tx` - Broadcast channel for emitting events
/// * `state` - Shared bridge state
/// * `time_effects` - Time effect handler for timestamps
/// * `amp_effects` - Optional AMP channel effect handler
/// * `effect_system` - Optional full effect system for complex operations
///
/// # Returns
/// * `Ok(())` on success
/// * `Err(String)` on failure (used for retry logic)
pub(super) async fn execute_command(
    command: &EffectCommand,
    event_tx: &broadcast::Sender<AuraEvent>,
    state: &Arc<RwLock<BridgeState>>,
    time_effects: &Arc<dyn PhysicalTimeEffects>,
    amp_effects: Option<&(dyn AmpChannelEffects + Send + Sync)>,
    effect_system: Option<&Arc<aura_agent::AuraEffectSystem>>,
    app_core: Option<&Arc<tokio::sync::RwLock<aura_app::AppCore>>>,
) -> Result<(), String> {
    tracing::debug!("Executing command: {:?}", command);

    // Phase 4: Authorization check before executing any command
    // Reads the user's current authorization level from state and checks
    // if it's sufficient for the command being executed.
    let (user_auth_level, current_context) = {
        let bridge_state = state.read().await;
        (bridge_state.user_auth_level, bridge_state.current_context)
    };

    let effect_system = effect_system.cloned();
    if let Some(effect_system) = &effect_system {
        let mut bridge_state = state.write().await;
        if !bridge_state.tokens_loaded {
            if let Err(e) = bridge_state
                .capability_token_store
                .load(effect_system.as_ref())
                .await
            {
                tracing::warn!("Failed to load capability tokens: {}", e);
            }
            bridge_state.tokens_loaded = true;
        }
    }
    check_authorization(command, user_auth_level, event_tx)?;

    // Use current_context from state, falling back to a default if not set
    let context =
        current_context.unwrap_or_else(|| ContextId::from_uuid(ids::uuid("tui:default-context")));

    let now_secs = || async {
        time_effects
            .physical_time()
            .await
            .map(|t| t.ts_ms / 1000)
            .map_err(|e| format!("time error: {e}"))
    };

    match command {
        // Recovery commands
        // NOTE: These commands demonstrate the integration pattern with aura-recovery::RecoveryProtocol
        // Full implementation requires:
        // - Recovery session state (account_authority, guardians, threshold)
        // - RelationalContext for recovery facts
        // - Persistent protocol instance across commands
        // See work/PHASE2_STATUS.md for detailed implementation plan
        EffectCommand::StartRecovery => {
            tracing::info!("StartRecovery command executing");

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::InitiateRecovery;
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for InitiateRecovery: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Create recovery session from AppCore guardian configuration
            let session_id = ids::uuid("tui:recovery-session").to_string();
            let account_authority = ids::authority_id("tui:demo-account");

            // Get guardians and threshold from AppCore if available, otherwise use demo data
            let (guardians, threshold) = if let Some(core) = app_core.as_ref() {
                let core_guard = core.read().await;
                let snapshot = core_guard.snapshot();
                let recovery_state = &snapshot.recovery;

                // Convert configured guardians to AuthorityIds
                let configured_guardians: Vec<aura_core::AuthorityId> = recovery_state
                    .guardians
                    .iter()
                    .filter(|g| g.status == aura_app::views::GuardianStatus::Active)
                    .map(|g| ids::authority_id(&g.id))
                    .collect();

                let threshold = recovery_state.threshold as usize;

                if configured_guardians.is_empty() || threshold == 0 {
                    // No guardians configured - use demo data
                    tracing::info!("No guardians configured, using demo data for recovery");
                    let guardian1 = ids::authority_id("tui:demo-guardian-1");
                    let guardian2 = ids::authority_id("tui:demo-guardian-2");
                    let guardian3 = ids::authority_id("tui:demo-guardian-3");
                    (vec![guardian1, guardian2, guardian3], 2)
                } else {
                    tracing::info!(
                        "Using configured guardians: count={}, threshold={}",
                        configured_guardians.len(),
                        threshold
                    );
                    (configured_guardians, threshold)
                }
            } else {
                // No AppCore - use demo data
                let guardian1 = ids::authority_id("tui:demo-guardian-1");
                let guardian2 = ids::authority_id("tui:demo-guardian-2");
                let guardian3 = ids::authority_id("tui:demo-guardian-3");
                (vec![guardian1, guardian2, guardian3], 2)
            };

            // Create recovery context (relational context is derived from participants)
            let recovery_context = Arc::new(RelationalContext::new(guardians.clone()));

            // Get current timestamp
            let started_at = time_effects
                .physical_time()
                .await
                .map_err(|e| format!("Failed to get timestamp: {}", e))?
                .ts_ms;

            // Create recovery session
            let session = crate::tui::recovery_session::RecoverySession::new(
                session_id.clone(),
                account_authority,
                guardians,
                threshold,
                recovery_context,
                started_at,
            );

            // Store in state
            {
                let mut bridge_state = state.write().await;
                bridge_state.recovery_session = Some(session);
            }

            tracing::info!(
                "Recovery session created and stored: session_id={}",
                session_id
            );

            // Emit event
            let _ = event_tx.send(AuraEvent::RecoveryStarted {
                session_id: session_id.clone(),
            });

            Ok(())
        }

        EffectCommand::SubmitGuardianApproval { guardian_id } => {
            tracing::info!(
                "SubmitGuardianApproval command executing for guardian: {}",
                guardian_id
            );

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                // Use a deterministic recovery context ID for now
                let recovery_context = ContextId::default();
                let intent = Intent::ApproveRecovery { recovery_context };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for ApproveRecovery: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get current recovery session from state
            let mut bridge_state = state.write().await;
            let session = bridge_state
                .recovery_session
                .as_mut()
                .ok_or("No active recovery session")?;

            // Parse guardian ID
            let guardian_authority = ids::authority_id(&guardian_id);

            // Get current timestamp
            let timestamp = time_effects
                .physical_time()
                .await
                .map_err(|e| format!("Failed to get timestamp: {}", e))?
                .ts_ms;

            // Create guardian approval
            let approval = crate::tui::recovery_session::GuardianApproval {
                guardian_id: guardian_authority,
                recovery_id: session.session_id.clone(),
                signature: vec![0xDE, 0xAD, 0xBE, 0xEF], // Demo signature
                timestamp,
            };

            // Add approval to session
            session.add_approval(approval).map_err(|e| {
                tracing::error!("Failed to add guardian approval: {}", e);
                e
            })?;

            let current = session.approval_count();
            let threshold = session.threshold;
            let session_id = session.session_id.clone();

            tracing::info!(
                "Guardian approval added: guardian={}, current={}/{}, session={}",
                guardian_id,
                current,
                threshold,
                session_id
            );

            // Drop the write lock before sending event
            drop(bridge_state);

            // Emit event
            let _ = event_tx.send(AuraEvent::GuardianApproved {
                guardian_id: guardian_id.clone(),
                current: current as u32,
                threshold: threshold as u32,
            });

            Ok(())
        }

        EffectCommand::CompleteRecovery => {
            tracing::info!("CompleteRecovery command executing");

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let recovery_context = ContextId::default();
                let intent = Intent::CompleteRecovery { recovery_context };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for CompleteRecovery: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get recovery session from state
            let mut bridge_state = state.write().await;
            let session = bridge_state
                .recovery_session
                .as_mut()
                .ok_or("No active recovery session")?;

            // Check if threshold is met
            if !session.is_threshold_met() {
                let current = session.approval_count();
                let threshold = session.threshold;
                return Err(format!(
                    "Recovery threshold not met: {}/{} approvals",
                    current, threshold
                ));
            }

            // Mark session as completed
            session.mark_completed();
            let session_id = session.session_id.clone();

            tracing::info!("Recovery completed: session={}", session_id);

            // Drop lock before emitting event
            drop(bridge_state);

            // Emit completion event
            let _ = event_tx.send(AuraEvent::RecoveryCompleted {
                session_id: session_id.clone(),
            });

            Ok(())
        }

        EffectCommand::CancelRecovery => {
            tracing::info!("CancelRecovery command executing");

            // Remove recovery session from state
            let mut bridge_state = state.write().await;
            let session = bridge_state
                .recovery_session
                .take()
                .ok_or("No active recovery session")?;

            let session_id = session.session_id.clone();

            tracing::info!("Recovery cancelled: session={}", session_id);

            // Drop lock before emitting event
            drop(bridge_state);

            // Emit cancellation event
            let _ = event_tx.send(AuraEvent::RecoveryCancelled {
                session_id: session_id.clone(),
            });

            Ok(())
        }

        // Chat commands
        EffectCommand::SendMessage { channel, content } => {
            let channel_id = ChannelId::from_str(&channel)
                .unwrap_or_else(|_| ChannelId::from_bytes(hash::hash(channel.as_bytes())));
            let sender = ids::authority_id("tui:self");

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                // Convert channel string to ContextId - use string parse or generate from hash
                let context_id = ContextId::from_str(&channel).unwrap_or_else(|_| {
                    let h = hash::hash(channel.as_bytes());
                    let mut uuid_bytes = [0u8; 16];
                    uuid_bytes.copy_from_slice(&h[..16]);
                    ContextId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
                });
                let intent = Intent::SendMessage {
                    channel_id: context_id,
                    content: content.clone(),
                    reply_to: None,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Also send via amp_effects for network side effects
            if let Some(amp) = amp_effects {
                let params = ChannelSendParams {
                    context,
                    channel: channel_id,
                    sender,
                    plaintext: content.as_bytes().to_vec(),
                    reply_to: None,
                };
                amp.send_message(params)
                    .await
                    .map_err(|e| format!("send_message error: {e:?}"))?;
            }

            let ts = now_secs().await?;
            let _ = event_tx.send(AuraEvent::MessageReceived {
                channel: channel.clone(),
                from: "self".to_string(),
                content: content.clone(),
                timestamp: ts,
            });
            Ok(())
        }

        EffectCommand::CreateChannel {
            name,
            topic,
            members,
        } => {
            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let channel_type = if members.len() == 1 {
                    IntentChannelType::DirectMessage
                } else {
                    IntentChannelType::Block
                };
                let intent = Intent::CreateChannel {
                    name: name.clone(),
                    channel_type,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for CreateChannel: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Also create via amp_effects for network side effects
            if let Some(amp) = amp_effects {
                let params = ChannelCreateParams {
                    context,
                    channel: None,
                    skip_window: None,
                    topic: topic.clone(),
                };

                match amp.create_channel(params).await {
                    Ok(channel_id) => {
                        let now_ms = time_effects
                            .physical_time()
                            .await
                            .map_err(|e| format!("time error: {e}"))?
                            .ts_ms;

                        let channel = crate::tui::reactive::Channel {
                            id: channel_id.to_string(),
                            name: name.clone(),
                            topic: topic.clone(),
                            channel_type: aura_app::ChannelType::Block,
                            unread_count: 0,
                            is_dm: members.len() == 1,
                            member_count: (members.len() as u32).saturating_add(1),
                            last_message: None,
                            last_message_time: None,
                            last_activity: now_ms,
                        };

                        let _ = event_tx.send(AuraEvent::ChannelCreated { channel });
                    }
                    Err(e) => {
                        return Err(format!("create_channel error: {e:?}"));
                    }
                }
            }

            Ok(())
        }

        EffectCommand::CloseChannel { channel } => {
            if let Some(amp) = amp_effects {
                let channel_id = ChannelId::from_str(&channel)
                    .unwrap_or_else(|_| ChannelId::from_bytes(hash(channel.as_bytes())));

                let params = ChannelCloseParams {
                    context,
                    channel: channel_id,
                };

                amp.close_channel(params)
                    .await
                    .map_err(|e| format!("close_channel error: {e:?}"))?;
            }

            let _ = event_tx.send(AuraEvent::ChannelClosed {
                channel_id: channel.clone(),
            });

            Ok(())
        }

        // Chat extra commands
        // NOTE: These commands use AMP (Asynchronous Messaging Protocol) similar to SendMessage
        // Full implementation requires:
        // - AmpChannelEffects for channel operations
        // - 1:1 channel creation for direct messages
        // - Channel membership management
        EffectCommand::SendDirectMessage { target, content } => {
            // Create a 1:1 DM channel with target and send message
            let context = ContextId::from_uuid(ids::uuid("tui:dm-context"));
            let sender = ids::authority_id("tui:self");
            let target_authority = ids::authority_id(&target);

            // Derive deterministic DM channel ID from both authorities
            // Sort to ensure both parties derive the same channel ID
            let mut parties = vec![sender.to_string(), target_authority.to_string()];
            parties.sort();
            let dm_channel_key = format!("dm:{}:{}", parties[0], parties[1]);
            let channel_id = ChannelId::from_bytes(hash::hash(dm_channel_key.as_bytes()));

            if let Some(amp) = amp_effects {
                // Send message on DM channel
                let params = ChannelSendParams {
                    context,
                    channel: channel_id,
                    sender,
                    plaintext: content.as_bytes().to_vec(),
                    reply_to: None,
                };

                amp.send_message(params)
                    .await
                    .map_err(|e| format!("DM send_message error: {e:?}"))?;

                tracing::info!("Direct message sent to {}", target);
            } else {
                tracing::warn!("AMP effects not available for direct message");
            }

            // Emit event for UI update
            let ts = now_secs().await?;
            let _ = event_tx.send(AuraEvent::MessageReceived {
                channel: format!("DM:{}", target),
                from: "self".to_string(),
                content: content.clone(),
                timestamp: ts,
            });

            Ok(())
        }

        EffectCommand::SendAction { channel, action } => {
            // Actions are special messages formatted with "* action" (emote style)
            // Send via AMP like regular messages
            let channel_id = ChannelId::from_str(&channel)
                .unwrap_or_else(|_| ChannelId::from_bytes(hash::hash(channel.as_bytes())));
            let sender = ids::authority_id("tui:self");
            let action_content = format!("* {}", action);

            if let Some(amp) = amp_effects {
                let params = ChannelSendParams {
                    context,
                    channel: channel_id,
                    sender,
                    plaintext: action_content.as_bytes().to_vec(),
                    reply_to: None,
                };

                amp.send_message(params)
                    .await
                    .map_err(|e| format!("send_action error: {e:?}"))?;

                tracing::info!("Action sent to {}: {}", channel, action);
            } else {
                tracing::warn!("AMP effects not available for action");
            }

            // Emit event for UI update
            let ts = now_secs().await?;
            let _ = event_tx.send(AuraEvent::MessageReceived {
                channel: channel.clone(),
                from: "self".to_string(),
                content: action_content,
                timestamp: ts,
            });

            Ok(())
        }

        EffectCommand::JoinChannel { channel } => {
            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let context_id = ContextId::from_str(&channel).unwrap_or_else(|_| {
                    let h = hash::hash(channel.as_bytes());
                    let mut uuid_bytes = [0u8; 16];
                    uuid_bytes.copy_from_slice(&h[..16]);
                    ContextId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
                });
                let intent = Intent::JoinChannel {
                    channel_id: context_id,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for JoinChannel: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Wire to AmpChannelEffects for channel membership
            if let Some(amp) = amp_effects {
                let channel_id = ChannelId::from_bytes(hash(channel.as_bytes()));
                let participant = ids::authority_id("tui:self");

                let params = ChannelJoinParams {
                    context,
                    channel: channel_id,
                    participant,
                };

                if let Err(e) = amp.join_channel(params).await {
                    tracing::warn!("Failed to join channel via AMP: {}", e);
                    // Continue anyway - channel may not exist yet or AMP not fully configured
                } else {
                    tracing::info!("Joined channel via AMP: {}", channel);
                }
            } else {
                tracing::debug!("AMP effects not available for join_channel");
            }

            // Emit event for UI updates
            let _ = event_tx.send(AuraEvent::UserJoined {
                channel: channel.clone(),
                user: "self".to_string(),
            });

            tracing::info!("JoinChannel completed for {}", channel);
            Ok(())
        }

        EffectCommand::LeaveChannel { channel } => {
            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let context_id = ContextId::from_str(&channel).unwrap_or_else(|_| {
                    let h = hash::hash(channel.as_bytes());
                    let mut uuid_bytes = [0u8; 16];
                    uuid_bytes.copy_from_slice(&h[..16]);
                    ContextId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
                });
                let intent = Intent::LeaveChannel {
                    channel_id: context_id,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for LeaveChannel: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Wire to AmpChannelEffects for channel membership
            if let Some(amp) = amp_effects {
                let channel_id = ChannelId::from_bytes(hash(channel.as_bytes()));
                let participant = ids::authority_id("tui:self");

                let params = ChannelLeaveParams {
                    context,
                    channel: channel_id,
                    participant,
                };

                if let Err(e) = amp.leave_channel(params).await {
                    tracing::warn!("Failed to leave channel via AMP: {}", e);
                    // Continue anyway - graceful degradation
                } else {
                    tracing::info!("Left channel via AMP: {}", channel);
                }
            } else {
                tracing::debug!("AMP effects not available for leave_channel");
            }

            // Emit event for UI updates
            let _ = event_tx.send(AuraEvent::UserLeft {
                channel: channel.clone(),
                user: "self".to_string(),
            });

            tracing::info!("LeaveChannel completed for {}", channel);
            Ok(())
        }

        // User/moderation commands
        EffectCommand::UpdateNickname { name } => {
            // Update in-memory state
            let mut bridge_state = state.write().await;
            bridge_state.user_nickname = Some(name.clone());
            drop(bridge_state);

            // Persist nickname to storage via effect system
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let nickname_key = "tui/settings/user_nickname";
                if let Err(e) = effects.store(nickname_key, name.as_bytes().to_vec()).await {
                    tracing::warn!("Failed to persist nickname to storage: {:?}", e);
                }
            }

            // Emit nickname updated event
            let _ = event_tx.send(AuraEvent::NicknameUpdated {
                nickname: name.to_string(),
            });

            tracing::info!("Nickname updated successfully");
            Ok(())
        }

        // === Contact Commands ===
        // NOTE: Contact state is now managed via AppCore.view_state.contacts
        // These commands dispatch intents to AppCore for state updates
        EffectCommand::UpdateContactPetname {
            contact_id,
            petname,
        } => {
            tracing::info!("Updating petname for contact {}: {}", contact_id, petname);

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::SetPetname {
                    contact_id: contact_id.clone(),
                    petname: petname.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for SetPetname: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Persist petname to storage (backup to journal)
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let petname_key = format!("tui/contacts/{}/petname", contact_id);
                if let Err(e) = effects
                    .store(&petname_key, petname.as_bytes().to_vec())
                    .await
                {
                    tracing::warn!("Failed to persist contact petname to storage: {:?}", e);
                }
            }

            // Emit event for UI update
            let _ = event_tx.send(AuraEvent::ContactPetnameUpdated {
                contact_id: contact_id.clone(),
                petname: petname.clone(),
            });

            tracing::info!("Contact petname updated successfully");
            Ok(())
        }

        EffectCommand::ToggleContactGuardian { contact_id } => {
            tracing::info!("Toggling guardian status for contact: {}", contact_id);

            // Read current guardian status from AppCore
            let current_is_guardian = if let Some(core) = app_core {
                let core_guard = core.read().await;
                let snapshot = core_guard.snapshot();
                snapshot
                    .contacts
                    .contacts
                    .iter()
                    .find(|c| c.id == *contact_id)
                    .map(|c| c.is_guardian)
                    .unwrap_or(false)
            } else {
                false
            };

            let new_is_guardian = !current_is_guardian;

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::ToggleGuardian {
                    contact_id: contact_id.clone(),
                    is_guardian: new_is_guardian,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for ToggleGuardian: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Persist guardian status to storage (backup to journal)
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let guardian_key = format!("tui/contacts/{}/is_guardian", contact_id);
                let value = if new_is_guardian {
                    b"true".to_vec()
                } else {
                    b"false".to_vec()
                };
                if let Err(e) = effects.store(&guardian_key, value).await {
                    tracing::warn!(
                        "Failed to persist contact guardian status to storage: {:?}",
                        e
                    );
                }
            }

            // Emit event for UI update
            let _ = event_tx.send(AuraEvent::ContactGuardianToggled {
                contact_id: contact_id.clone(),
                is_guardian: new_is_guardian,
            });

            tracing::info!(
                "Contact {} guardian status toggled to {}",
                contact_id,
                new_is_guardian
            );

            Ok(())
        }

        EffectCommand::InviteGuardian { contact_id } => {
            tracing::info!("Initiating guardian invitation: {:?}", contact_id);

            // Get current timestamp for invitation
            let now_ts = time_effects
                .physical_time()
                .await
                .map(|t| t.ts_ms)
                .unwrap_or(0);

            let invitation_id = format!("guardian-inv-{}", now_ts);

            // NOTE: Guardian invitation state is now managed via AppCore.view_state.invitations
            // We only persist to storage and emit event here

            // Persist to storage
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let inv_key = format!("tui/guardian_invitations/{}", invitation_id);
                #[derive(serde::Serialize)]
                struct StoredGuardianInvitation {
                    contact_id: Option<String>,
                    created_at: u64,
                    expires_at: u64,
                }
                let stored = StoredGuardianInvitation {
                    contact_id: contact_id.clone(),
                    created_at: now_ts,
                    expires_at: now_ts + (7 * 24 * 60 * 60 * 1000),
                };
                let inv_data = serde_json::to_vec(&stored).unwrap_or_default();
                if let Err(e) = effects.store(&inv_key, inv_data).await {
                    tracing::warn!("Failed to persist guardian invitation to storage: {:?}", e);
                }
            }

            // Emit event for UI update
            let _ = event_tx.send(AuraEvent::GuardianInvitationSent {
                invitation_id: invitation_id.clone(),
                contact_id: contact_id.clone(),
            });

            tracing::info!("Guardian invitation {} created", invitation_id);
            Ok(())
        }

        // Invitation commands - wired to aura-invitation protocol
        // Uses BridgeState.invitation_registry for status tracking
        // NOTE: Invitation state is now primarily managed via AppCore.view_state.invitations
        EffectCommand::AcceptInvitation { invitation_id } => {
            tracing::info!("Accepting invitation: {}", invitation_id);

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::AcceptInvitation {
                    invitation_fact: invitation_id.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for AcceptInvitation: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get current timestamp for registry update
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });
            let timestamp = TimeStamp::PhysicalClock(now);

            // Update invitation registry
            let mut bridge_state = state.write().await;

            // Check registry status to validate invitation
            let registry_record = bridge_state.invitation_registry.get(&invitation_id);
            let current_status = registry_record.map(|r| r.status);

            // Validate: can only accept pending invitations
            match current_status {
                Some(InvitationStatus::Accepted) => {
                    return Err(format!(
                        "Invitation {} has already been accepted",
                        invitation_id
                    ));
                }
                Some(InvitationStatus::Declined) => {
                    return Err(format!(
                        "Invitation {} has been declined and cannot be accepted",
                        invitation_id
                    ));
                }
                Some(InvitationStatus::Expired) => {
                    return Err(format!("Invitation {} has expired", invitation_id));
                }
                Some(InvitationStatus::Pending) | None => {
                    // OK to accept - pending or not yet tracked
                }
            }

            // Mark as accepted in registry
            bridge_state
                .invitation_registry
                .mark_accepted(&invitation_id, timestamp);

            tracing::info!("Accepted invitation {} and updated registry", invitation_id);

            drop(bridge_state);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::InvitationAccepted {
                invitation_id: invitation_id.clone(),
            });

            Ok(())
        }

        EffectCommand::DeclineInvitation { invitation_id } => {
            tracing::info!("Declining invitation: {}", invitation_id);

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::RejectInvitation {
                    invitation_fact: invitation_id.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for RejectInvitation: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get current timestamp for registry update
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });
            let timestamp = TimeStamp::PhysicalClock(now);

            // Update state
            let mut bridge_state = state.write().await;

            // Check registry status to validate invitation
            let registry_record = bridge_state.invitation_registry.get(&invitation_id);
            let current_status = registry_record.map(|r| r.status);

            // Validate: can only decline pending invitations
            match current_status {
                Some(InvitationStatus::Accepted) => {
                    return Err(format!(
                        "Invitation {} has already been accepted and cannot be declined",
                        invitation_id
                    ));
                }
                Some(InvitationStatus::Declined) => {
                    return Err(format!(
                        "Invitation {} has already been declined",
                        invitation_id
                    ));
                }
                Some(InvitationStatus::Expired) => {
                    return Err(format!(
                        "Invitation {} has expired and cannot be declined",
                        invitation_id
                    ));
                }
                Some(InvitationStatus::Pending) | None => {
                    // OK to decline
                }
            }

            // Mark as declined in registry
            bridge_state
                .invitation_registry
                .mark_declined(&invitation_id, timestamp);

            tracing::info!("Declined invitation {} and updated registry", invitation_id);

            drop(bridge_state);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::InvitationDeclined {
                invitation_id: invitation_id.clone(),
            });

            Ok(())
        }

        // Moderation commands
        EffectCommand::ListParticipants { channel } => {
            tracing::info!("ListParticipants for channel: {}", channel);

            // Find the block by channel ID (channel == block_id for now)
            let participants: Vec<String> = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .get(channel)
                    .map(|block| {
                        block
                            .residents
                            .iter()
                            .map(|r| r.authority_id.to_string())
                            .collect()
                    })
                    .unwrap_or_default()
            };

            let count = participants.len();
            tracing::info!("Found {} participants in channel {}", count, channel);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::ParticipantsList {
                channel: channel.clone(),
                participants,
                count,
            });

            Ok(())
        }

        EffectCommand::GetUserInfo { target } => {
            tracing::info!("GetUserInfo for target: {}", target);

            // Search for the user across all blocks
            let bridge_state = state.read().await;
            for (_block_id, block) in &bridge_state.blocks {
                if let Some(resident) = block
                    .residents
                    .iter()
                    .find(|r| r.authority_id.to_string() == *target)
                {
                    tracing::info!(
                        "Found user info for {}: steward={}, joined={}, storage={}",
                        target,
                        resident.is_steward,
                        resident.joined_at,
                        resident.storage_allocated
                    );

                    // Emit event with user information
                    let _ = event_tx.send(AuraEvent::UserInfo {
                        user_id: target.clone(),
                        name: resident.name.clone(),
                        is_steward: resident.is_steward,
                        joined_at: resident.joined_at,
                        storage_allocated: resident.storage_allocated,
                    });

                    return Ok(());
                }
            }

            // User not found
            tracing::warn!("User {} not found in any block", target);
            let _ = event_tx.send(AuraEvent::Error {
                code: "USER_NOT_FOUND".to_string(),
                message: format!("User {} not found", target),
            });

            Ok(())
        }

        // === Moderation Actions ===
        // Check steward permissions and perform moderation actions
        EffectCommand::KickUser {
            channel,
            target,
            reason,
        } => {
            tracing::info!("KickUser: channel={}, target={}", channel, target);

            // Check if actor is a steward
            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "KickUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "KickUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for moderation actions".to_string(),
                });
                return Ok(());
            }

            // Get current timestamp
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

            // Log kick to persistent audit trail
            let mut bridge_state = state.write().await;
            for block in bridge_state.blocks.values_mut() {
                // Only update blocks where the actor is a steward
                if block
                    .residents
                    .iter()
                    .any(|r| r.authority_id == actor_id && r.is_steward)
                {
                    block.kick_log.push(KickRecord {
                        authority_id: target.clone(),
                        channel: channel.clone(),
                        reason: reason.clone().unwrap_or_default(),
                        actor: actor_id.to_string(),
                        kicked_at: now.ts_ms,
                    });
                }
            }
            drop(bridge_state);

            // Journal integration: Write kick facts for distributed persistence
            if let Some(effect_system) = effect_system {
                // Parse target string to AuthorityId
                let kicked_authority = match target.parse::<aura_core::AuthorityId>() {
                    Ok(auth_id) => auth_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse target as AuthorityId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_AUTHORITY".to_string(),
                            message: format!("Invalid authority ID format: {}", target),
                        });
                        return Ok(());
                    }
                };

                // Parse channel string to ChannelId
                let channel_id = match channel.parse::<aura_core::ChannelId>() {
                    Ok(chan_id) => chan_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse channel as ChannelId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_CHANNEL".to_string(),
                            message: format!("Invalid channel ID format: {}", channel),
                        });
                        return Ok(());
                    }
                };

                // Write a kick fact for each block where actor is steward
                let bridge_state = state.read().await;
                for block in bridge_state.blocks.values() {
                    if block
                        .residents
                        .iter()
                        .any(|r| r.authority_id == actor_id && r.is_steward)
                    {
                        let kick_fact = BlockKickFact {
                            context_id: block.context_id.clone(),
                            channel_id: channel_id.clone(),
                            kicked_authority: kicked_authority.clone(),
                            actor_authority: actor_id.clone(),
                            reason: reason.clone().unwrap_or_default(),
                            kicked_at_ms: now.ts_ms,
                        }
                        .to_generic();

                        // Write fact to journal for distributed sync
                        if let Err(e) = effect_system.insert_relational_fact(kick_fact).await {
                            tracing::error!("Failed to persist kick fact to journal: {:?}", e);
                            // Continue anyway - local state is already updated
                        } else {
                            tracing::info!(
                                "KickUser fact persisted to journal for block {}: {} from {}",
                                block.id,
                                target,
                                channel
                            );
                        }

                        // Wire to AmpChannelEffects to record channel leave
                        if let Some(amp) = amp_effects {
                            let leave_params = ChannelLeaveParams {
                                context: block.context_id.clone(),
                                channel: channel_id.clone(),
                                participant: kicked_authority.clone(),
                            };
                            if let Err(e) = amp.leave_channel(leave_params).await {
                                tracing::warn!(
                                    "Failed to record leave_channel for kicked user: {:?}",
                                    e
                                );
                                // Continue - kick fact is already recorded
                            }
                        }
                    }
                }
                drop(bridge_state);
            }

            tracing::info!(
                "KickUser successful: {} kicked persistently from {}",
                target,
                channel
            );
            let _ = event_tx.send(AuraEvent::UserKicked {
                channel: channel.clone(),
                target: target.clone(),
                actor: actor_id.to_string(),
                reason: reason.clone(),
            });

            Ok(())
        }

        EffectCommand::BanUser { target, reason } => {
            tracing::info!("BanUser: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "BanUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "BanUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for moderation actions".to_string(),
                });
                return Ok(());
            }

            // Get current timestamp
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

            // Store ban in all blocks where actor is steward
            let mut bridge_state = state.write().await;
            for block in bridge_state.blocks.values_mut() {
                // Only update blocks where the actor is a steward
                if block
                    .residents
                    .iter()
                    .any(|r| r.authority_id == actor_id && r.is_steward)
                {
                    block.ban_list.insert(
                        target.clone(),
                        BanRecord {
                            authority_id: target.clone(),
                            reason: reason.clone().unwrap_or_default(),
                            actor: actor_id.to_string(),
                            banned_at: now.ts_ms,
                        },
                    );
                }
            }
            // Journal integration: Write ban facts for distributed persistence
            if let Some(effect_system) = effect_system {
                // Parse target string to AuthorityId
                let banned_authority = match target.parse::<aura_core::AuthorityId>() {
                    Ok(auth_id) => auth_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse target as AuthorityId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_AUTHORITY".to_string(),
                            message: format!("Invalid authority ID format: {}", target),
                        });
                        return Ok(());
                    }
                };

                // Write a ban fact for each block where actor is steward
                let bridge_state = state.read().await;
                for block in bridge_state.blocks.values() {
                    if block
                        .residents
                        .iter()
                        .any(|r| r.authority_id == actor_id && r.is_steward)
                    {
                        let ban_fact = BlockBanFact {
                            context_id: block.context_id.clone(),
                            channel_id: None, // Block-wide ban
                            banned_authority: banned_authority.clone(),
                            actor_authority: actor_id.clone(),
                            reason: reason.clone().unwrap_or_default(),
                            banned_at_ms: now.ts_ms,
                            expires_at_ms: None, // Permanent ban
                        }
                        .to_generic();

                        // Write fact to journal for distributed sync
                        if let Err(e) = effect_system.insert_relational_fact(ban_fact).await {
                            tracing::error!("Failed to persist ban fact to journal: {:?}", e);
                            // Continue anyway - local state is already updated
                        } else {
                            tracing::info!(
                                "BanUser fact persisted to journal for block {}: {}",
                                block.id,
                                target
                            );
                        }
                    }
                }
                drop(bridge_state);
            }

            tracing::info!("BanUser successful: {} banned persistently", target);
            let _ = event_tx.send(AuraEvent::UserBanned {
                target: target.clone(),
                actor: actor_id.to_string(),
                reason: Some(reason.clone().unwrap_or_default()),
            });

            Ok(())
        }

        EffectCommand::UnbanUser { target } => {
            tracing::info!("UnbanUser: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "UnbanUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "UnbanUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for moderation actions".to_string(),
                });
                return Ok(());
            }

            // Get current timestamp for unban fact
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

            // Remove ban from all blocks where actor is steward
            let mut bridge_state = state.write().await;
            for block in bridge_state.blocks.values_mut() {
                // Only update blocks where the actor is a steward
                if block
                    .residents
                    .iter()
                    .any(|r| r.authority_id == actor_id && r.is_steward)
                {
                    block.ban_list.remove(target.as_str());
                }
            }
            drop(bridge_state);

            // Journal integration: Write unban facts for distributed persistence
            if let Some(effect_system) = effect_system {
                // Parse target string to AuthorityId
                let unbanned_authority = match target.parse::<aura_core::AuthorityId>() {
                    Ok(auth_id) => auth_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse target as AuthorityId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_AUTHORITY".to_string(),
                            message: format!("Invalid authority ID format: {}", target),
                        });
                        return Ok(());
                    }
                };

                // Write an unban fact for each block where actor is steward
                let bridge_state = state.read().await;
                for block in bridge_state.blocks.values() {
                    if block
                        .residents
                        .iter()
                        .any(|r| r.authority_id == actor_id && r.is_steward)
                    {
                        let unban_fact = BlockUnbanFact {
                            context_id: block.context_id.clone(),
                            channel_id: None, // Block-wide unban
                            unbanned_authority: unbanned_authority.clone(),
                            actor_authority: actor_id.clone(),
                            unbanned_at_ms: now.ts_ms,
                        }
                        .to_generic();

                        // Write fact to journal for distributed sync
                        if let Err(e) = effect_system.insert_relational_fact(unban_fact).await {
                            tracing::error!("Failed to persist unban fact to journal: {:?}", e);
                            // Continue anyway - local state is already updated
                        } else {
                            tracing::info!(
                                "UnbanUser fact persisted to journal for block {}: {}",
                                block.id,
                                target
                            );
                        }
                    }
                }
                drop(bridge_state);
            }

            tracing::info!("UnbanUser successful: {} unbanned persistently", target);
            let _ = event_tx.send(AuraEvent::UserUnbanned {
                target: target.clone(),
                actor: actor_id.to_string(),
            });

            Ok(())
        }

        EffectCommand::MuteUser {
            target,
            duration_secs,
        } => {
            tracing::info!("MuteUser: target={}, duration={:?}", target, duration_secs);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "MuteUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "MuteUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for moderation actions".to_string(),
                });
                return Ok(());
            }

            // Get current timestamp
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

            // Calculate expiration timestamp if duration is specified
            let expires_at = duration_secs.map(|secs| now.ts_ms + (secs * 1000));

            // Store mute in all blocks where actor is steward
            let mut bridge_state = state.write().await;
            for block in bridge_state.blocks.values_mut() {
                // Only update blocks where the actor is a steward
                if block
                    .residents
                    .iter()
                    .any(|r| r.authority_id == actor_id && r.is_steward)
                {
                    block.mute_list.insert(
                        target.clone(),
                        MuteRecord {
                            authority_id: target.clone(),
                            duration_secs: *duration_secs,
                            muted_at: now.ts_ms,
                            expires_at,
                            actor: actor_id.to_string(),
                        },
                    );
                }
            }
            drop(bridge_state);

            // Journal integration: Write mute facts for distributed persistence
            if let Some(effect_system) = effect_system {
                // Parse target string to AuthorityId
                let muted_authority = match target.parse::<aura_core::AuthorityId>() {
                    Ok(auth_id) => auth_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse target as AuthorityId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_AUTHORITY".to_string(),
                            message: format!("Invalid authority ID format: {}", target),
                        });
                        return Ok(());
                    }
                };

                // Write a mute fact for each block where actor is steward
                let bridge_state = state.read().await;
                for block in bridge_state.blocks.values() {
                    if block
                        .residents
                        .iter()
                        .any(|r| r.authority_id == actor_id && r.is_steward)
                    {
                        let mute_fact = BlockMuteFact {
                            context_id: block.context_id.clone(),
                            channel_id: None,
                            muted_authority: muted_authority.clone(),
                            actor_authority: actor_id.clone(),
                            duration_secs: *duration_secs,
                            muted_at_ms: now.ts_ms,
                            expires_at_ms: expires_at,
                        };
                        if let Err(e) = effect_system
                            .insert_relational_fact(mute_fact.to_generic())
                            .await
                        {
                            tracing::error!("Failed to persist mute fact to journal: {:?}", e);
                        } else {
                            tracing::info!(
                                "MuteUser fact persisted to journal for block {}: {}",
                                block.id,
                                target
                            );
                        }
                    }
                }
                drop(bridge_state);
            }

            tracing::info!(
                "MuteUser successful: {} muted persistently for {:?} seconds",
                target,
                duration_secs
            );
            let _ = event_tx.send(AuraEvent::UserMuted {
                target: target.clone(),
                actor: actor_id.to_string(),
                duration_secs: *duration_secs,
            });

            Ok(())
        }

        EffectCommand::UnmuteUser { target } => {
            tracing::info!("UnmuteUser: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "UnmuteUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "UnmuteUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for moderation actions".to_string(),
                });
                return Ok(());
            }

            // Get current timestamp for unmute fact
            let now = time_effects.physical_time().await.unwrap_or(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            });

            // Remove mute from all blocks where actor is steward
            let mut bridge_state = state.write().await;
            for block in bridge_state.blocks.values_mut() {
                // Only update blocks where the actor is a steward
                if block
                    .residents
                    .iter()
                    .any(|r| r.authority_id == actor_id && r.is_steward)
                {
                    block.mute_list.remove(target.as_str());
                }
            }
            drop(bridge_state);

            // Journal integration: Write unmute facts for distributed persistence
            if let Some(effect_system) = effect_system {
                // Parse target string to AuthorityId
                let unmuted_authority = match target.parse::<aura_core::AuthorityId>() {
                    Ok(auth_id) => auth_id,
                    Err(e) => {
                        tracing::warn!("Failed to parse target as AuthorityId: {}", e);
                        let _ = event_tx.send(AuraEvent::Error {
                            code: "INVALID_AUTHORITY".to_string(),
                            message: format!("Invalid authority ID format: {}", target),
                        });
                        return Ok(());
                    }
                };

                // Write an unmute fact for each block where actor is steward
                let bridge_state = state.read().await;
                for block in bridge_state.blocks.values() {
                    if block
                        .residents
                        .iter()
                        .any(|r| r.authority_id == actor_id && r.is_steward)
                    {
                        let unmute_fact = BlockUnmuteFact {
                            context_id: block.context_id.clone(),
                            channel_id: None, // Block-wide unmute
                            unmuted_authority: unmuted_authority.clone(),
                            actor_authority: actor_id.clone(),
                            unmuted_at_ms: now.ts_ms,
                        };

                        // Write fact to journal for distributed sync
                        if let Err(e) = effect_system
                            .insert_relational_fact(unmute_fact.to_generic())
                            .await
                        {
                            tracing::error!("Failed to persist unmute fact to journal: {:?}", e);
                            // Continue anyway - local state is already updated
                        } else {
                            tracing::info!(
                                "UnmuteUser fact persisted to journal for block {}: {}",
                                block.id,
                                target
                            );
                        }
                    }
                }
                drop(bridge_state);
            }

            tracing::info!("UnmuteUser successful: {} unmuted persistently", target);
            let _ = event_tx.send(AuraEvent::UserUnmuted {
                target: target.clone(),
                actor: actor_id.to_string(),
            });

            Ok(())
        }

        EffectCommand::InviteUser { target } => {
            tracing::info!("InviteUser: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();
            let is_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_steward {
                tracing::warn!(
                    "InviteUser denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "InviteUser".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required for inviting users".to_string(),
                });
                return Ok(());
            }

            // Get current block and create invitation
            let now_ts = time_effects
                .physical_time()
                .await
                .map(|t| t.ts_ms)
                .unwrap_or(0);
            let expires_at = now_ts + (7 * 24 * 60 * 60 * 1000); // 7 days

            let invitation_id = format!("inv-{}", now_ts);
            let target_authority = ids::authority_id(&format!("tui:user:{}", target));

            let mut bridge_state = state.write().await;
            let current_block_id = bridge_state.current_block.clone();
            let block_name = current_block_id
                .as_ref()
                .and_then(|id| bridge_state.blocks.get(id))
                .and_then(|b| b.name.clone());

            if let Some(block_id) = current_block_id {
                let envelope = BlockInvitationEnvelope {
                    invitation_id: invitation_id.clone(),
                    block_id: block_id.clone(),
                    block_name,
                    inviter_authority: actor_id.clone(),
                    invitee_authority: target_authority,
                    created_at: now_ts,
                    expires_at,
                };
                bridge_state
                    .sent_block_invitations
                    .insert(invitation_id.clone(), envelope);
                drop(bridge_state);

                tracing::info!(
                    "InviteUser successful: {} invited to block {}",
                    target,
                    block_id
                );
                let _ = event_tx.send(AuraEvent::UserInvited {
                    target: target.clone(),
                    actor: actor_id.to_string(),
                });
            } else {
                drop(bridge_state);
                tracing::warn!("InviteUser failed: no current block set");
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_CURRENT_BLOCK".to_string(),
                    message: "No current block to invite user to".to_string(),
                });
            }

            Ok(())
        }

        // === Role Management ===
        EffectCommand::GrantSteward { target } => {
            tracing::info!("GrantSteward: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Check if actor is a steward
            let is_actor_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_actor_steward {
                tracing::warn!(
                    "GrantSteward denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "GrantSteward".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to grant stewardship".to_string(),
                });
                return Ok(());
            }

            // Find target and update steward flag
            let mut block_id_found: Option<String> = None;
            {
                let mut bridge_state = state.write().await;
                for (block_id, block) in &mut bridge_state.blocks {
                    if let Some(resident) = block
                        .residents
                        .iter_mut()
                        .find(|r| r.authority_id.to_string() == *target)
                    {
                        resident.is_steward = true;
                        block_id_found = Some(block_id.clone());
                        break;
                    }
                }
            }

            if let Some(block_id) = block_id_found {
                // Create Biscuit token with steward capability
                // NOTE: This creates the token but doesn't store it yet, as the full
                // token storage and relational context infrastructure is not complete.
                // Full implementation requires:
                // 1. Persistent token storage (per-user Biscuit token store)
                // 2. RelationalContext API for capability updates
                // 3. Token verification at command execution time

                use biscuit_auth::{macros::*, KeyPair};

                // Generate keypair for demonstration (demo keypair; real use: stored account keypair)
                let keypair = KeyPair::new();

                // Create Biscuit token with steward role
                let target_user = target.to_string();
                let biscuit_target = target_user.clone();
                match biscuit!(
                    r#"
                        user({biscuit_target});
                        role("steward");
                        capability("moderate");
                        capability("invite");
                        capability("configure_block");
                        "#
                )
                .build(&keypair)
                {
                    Ok(token) => {
                        // Serialize token for storage
                        if let Ok(token_bytes) = token.to_vec() {
                            // Get current timestamp for storage
                            let stored_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);

                            // Store the token in capability token store
                            let capabilities = vec![
                                "moderate".to_string(),
                                "invite".to_string(),
                                "configure_block".to_string(),
                            ];

                            {
                                let mut bridge_state = state.write().await;
                                bridge_state.capability_token_store.store_token(
                                    &target_user,
                                    token_bytes.clone(),
                                    "steward",
                                    capabilities,
                                    stored_at,
                                    None, // No expiration for steward tokens
                                );
                                if let Some(effect_system) = &effect_system {
                                    if let Err(e) = bridge_state
                                        .capability_token_store
                                        .persist(effect_system.as_ref())
                                        .await
                                    {
                                        tracing::warn!(
                                            "Failed to persist capability tokens: {}",
                                            e
                                        );
                                    }
                                }
                            }

                            tracing::info!(
                                "Created and stored Biscuit steward token for {}: {} bytes",
                                target_user,
                                token_bytes.len()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to create Biscuit token for {}: {}", target, e);
                    }
                }

                tracing::info!(
                    "GrantSteward successful: {} granted steward role in block {}",
                    target,
                    block_id
                );
                let _ = event_tx.send(AuraEvent::StewardGranted {
                    target: target.clone(),
                    actor: actor_id.to_string(),
                    block_id,
                });
            } else {
                tracing::warn!(
                    "GrantSteward failed: target {} not found in any block",
                    target
                );
                let _ = event_tx.send(AuraEvent::Error {
                    code: "USER_NOT_FOUND".to_string(),
                    message: format!("Target user {} not found in any block", target),
                });
            }

            Ok(())
        }

        EffectCommand::RevokeSteward { target } => {
            tracing::info!("RevokeSteward: target={}", target);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Check if actor is a steward
            let is_actor_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .values()
                    .flat_map(|block| &block.residents)
                    .find(|r| r.authority_id == actor_id)
                    .map(|r| r.is_steward)
                    .unwrap_or(false)
            };

            if !is_actor_steward {
                tracing::warn!(
                    "RevokeSteward denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "RevokeSteward".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to revoke stewardship".to_string(),
                });
                return Ok(());
            }

            // Find target and update steward flag
            let mut block_id_found: Option<String> = None;
            {
                let mut bridge_state = state.write().await;
                for (block_id, block) in &mut bridge_state.blocks {
                    if let Some(resident) = block
                        .residents
                        .iter_mut()
                        .find(|r| r.authority_id.to_string() == *target)
                    {
                        resident.is_steward = false;
                        block_id_found = Some(block_id.clone());
                        break;
                    }
                }
            }

            if let Some(block_id) = block_id_found {
                // Create Biscuit token revoking steward capability
                // NOTE: Biscuit doesn't support removing facts, so revocation is done by
                // adding constraints that prevent steward actions. This creates a new token
                // without steward capabilities.
                // Full implementation requires:
                // 1. Fetching the user's current token from storage
                // 2. Creating a new attenuated token without steward capabilities
                // 3. Updating the stored token via RelationalContext API

                use biscuit_auth::{macros::*, KeyPair};

                // Generate keypair for demonstration (demo keypair; real use: stored account keypair)
                let keypair = KeyPair::new();

                // Create Biscuit token with revoked steward permissions
                let target_user = target.to_string();
                let biscuit_target = target_user.clone();
                // User now only has basic capabilities, no moderation/admin rights
                match biscuit!(
                    r#"
                        user({biscuit_target});
                        role("member");
                        capability("read");
                        capability("write");
                        check if role($r), $r != "steward";
                        "#
                )
                .build(&keypair)
                {
                    Ok(token) => {
                        // Serialize token for storage
                        if let Ok(token_bytes) = token.to_vec() {
                            // Get current timestamp for storage
                            let stored_at = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis() as u64)
                                .unwrap_or(0);

                            // Store the updated token (replaces steward token with member token)
                            let capabilities = vec!["read".to_string(), "write".to_string()];

                            {
                                let mut bridge_state = state.write().await;
                                bridge_state.capability_token_store.store_token(
                                    &target_user,
                                    token_bytes.clone(),
                                    "member",
                                    capabilities,
                                    stored_at,
                                    None,
                                );
                                if let Some(effect_system) = &effect_system {
                                    if let Err(e) = bridge_state
                                        .capability_token_store
                                        .persist(effect_system.as_ref())
                                        .await
                                    {
                                        tracing::warn!(
                                            "Failed to persist capability tokens: {}",
                                            e
                                        );
                                    }
                                }
                            }

                            tracing::info!(
                                "Created and stored Biscuit member token for {} (steward revoked): {} bytes",
                                target_user,
                                token_bytes.len()
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to create revocation Biscuit token for {}: {}",
                            target,
                            e
                        );
                    }
                }

                tracing::info!(
                    "RevokeSteward successful: {} revoked steward role in block {}",
                    target,
                    block_id
                );
                let _ = event_tx.send(AuraEvent::StewardRevoked {
                    target: target.clone(),
                    actor: actor_id.to_string(),
                    block_id,
                });
            } else {
                tracing::warn!(
                    "RevokeSteward failed: target {} not found in any block",
                    target
                );
                let _ = event_tx.send(AuraEvent::Error {
                    code: "USER_NOT_FOUND".to_string(),
                    message: format!("Target user {} not found in any block", target),
                });
            }

            Ok(())
        }

        // NOTE: These commands use AMP channel metadata
        // Full implementation requires:
        // - AmpChannelEffects for metadata management
        // - Channel metadata CRDT for settings
        // Channel Management Commands
        EffectCommand::SetTopic { channel, text } => {
            tracing::info!("SetTopic: channel={}, text={}", channel, text);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Check if actor is a steward in the channel/block
            let is_actor_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .get(channel.as_str())
                    .and_then(|block| {
                        block
                            .residents
                            .iter()
                            .find(|r| r.authority_id == actor_id)
                            .map(|r| r.is_steward)
                    })
                    .unwrap_or(false)
            };

            if !is_actor_steward {
                tracing::warn!(
                    "SetTopic denied: actor {} is not a steward in channel {}",
                    actor_id.to_string(),
                    channel
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "SetTopic".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to set channel topic".to_string(),
                });
                return Ok(());
            }

            // Update topic
            {
                let mut bridge_state = state.write().await;
                if let Some(block) = bridge_state.blocks.get_mut(channel.as_str()) {
                    block.topic = Some(text.clone());
                    tracing::info!(
                        "SetTopic successful: channel {} topic set to '{}'",
                        channel,
                        text
                    );
                    let _ = event_tx.send(AuraEvent::TopicSet {
                        channel: channel.clone(),
                        text: text.clone(),
                        actor: actor_id.to_string(),
                    });
                } else {
                    tracing::warn!("SetTopic failed: channel {} not found", channel);
                    let _ = event_tx.send(AuraEvent::Error {
                        code: "CHANNEL_NOT_FOUND".to_string(),
                        message: format!("Channel {} not found", channel),
                    });
                }
            }

            // Persist topic to storage via effect system
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let topic_key = format!("tui/blocks/{}/topic", channel);
                if let Err(e) = effects.store(&topic_key, text.as_bytes().to_vec()).await {
                    tracing::warn!("Failed to persist topic to storage: {:?}", e);
                }
            }

            Ok(())
        }

        EffectCommand::PinMessage { message_id } => {
            tracing::info!("PinMessage: message_id={}", message_id);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Find which channel contains this message and check steward permission
            let mut channel_found: Option<String> = None;
            let mut is_steward = false;
            {
                let bridge_state = state.read().await;
                for (block_id, block) in &bridge_state.blocks {
                    if let Some(resident) =
                        block.residents.iter().find(|r| r.authority_id == actor_id)
                    {
                        is_steward = resident.is_steward;
                        channel_found = Some(block_id.clone());
                        break;
                    }
                }
            }

            if !is_steward {
                tracing::warn!(
                    "PinMessage denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "PinMessage".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to pin messages".to_string(),
                });
                return Ok(());
            }

            if let Some(channel) = channel_found {
                // Add message to pinned list
                {
                    let mut bridge_state = state.write().await;
                    if let Some(block) = bridge_state.blocks.get_mut(channel.as_str()) {
                        if !block.pinned_messages.contains(&message_id) {
                            block.pinned_messages.push(message_id.clone());
                            tracing::info!(
                                "PinMessage successful: message {} pinned in channel {}",
                                message_id,
                                channel
                            );
                            let _ = event_tx.send(AuraEvent::MessagePinned {
                                message_id: message_id.clone(),
                                channel: channel.clone(),
                                actor: actor_id.to_string(),
                            });
                        } else {
                            tracing::info!("PinMessage: message {} already pinned", message_id);
                        }
                    }
                }

                // Persist pinned messages to storage via effect system
                if let Some(effects) = &effect_system {
                    use aura_core::effects::StorageEffects;
                    let bridge_state = state.read().await;
                    if let Some(block) = bridge_state.blocks.get(channel.as_str()) {
                        let pins_key = format!("tui/blocks/{}/pinned", channel);
                        let pins_data =
                            serde_json::to_vec(&block.pinned_messages).unwrap_or_default();
                        if let Err(e) = effects.store(&pins_key, pins_data).await {
                            tracing::warn!("Failed to persist pinned messages to storage: {:?}", e);
                        }
                    }
                }
            } else {
                tracing::warn!("PinMessage failed: actor not found in any channel");
                let _ = event_tx.send(AuraEvent::Error {
                    code: "CHANNEL_NOT_FOUND".to_string(),
                    message: "Could not determine channel for message".to_string(),
                });
            }

            Ok(())
        }

        EffectCommand::UnpinMessage { message_id } => {
            tracing::info!("UnpinMessage: message_id={}", message_id);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Find which channel contains this message and check steward permission
            let mut channel_found: Option<String> = None;
            let mut is_steward = false;
            {
                let bridge_state = state.read().await;
                for (block_id, block) in &bridge_state.blocks {
                    if block.pinned_messages.contains(&message_id) {
                        if let Some(resident) =
                            block.residents.iter().find(|r| r.authority_id == actor_id)
                        {
                            is_steward = resident.is_steward;
                            channel_found = Some(block_id.clone());
                            break;
                        }
                    }
                }
            }

            if !is_steward {
                tracing::warn!(
                    "UnpinMessage denied: actor {} is not a steward",
                    actor_id.to_string()
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "UnpinMessage".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to unpin messages".to_string(),
                });
                return Ok(());
            }

            if let Some(channel) = channel_found {
                // Remove message from pinned list
                {
                    let mut bridge_state = state.write().await;
                    if let Some(block) = bridge_state.blocks.get_mut(channel.as_str()) {
                        if let Some(idx) =
                            block.pinned_messages.iter().position(|id| id == message_id)
                        {
                            block.pinned_messages.remove(idx);
                            tracing::info!(
                                "UnpinMessage successful: message {} unpinned from channel {}",
                                message_id,
                                channel
                            );
                            let _ = event_tx.send(AuraEvent::MessageUnpinned {
                                message_id: message_id.clone(),
                                channel: channel.clone(),
                                actor: actor_id.to_string(),
                            });
                        } else {
                            tracing::info!(
                                "UnpinMessage: message {} not found in pinned list",
                                message_id
                            );
                        }
                    }
                }

                // Persist pinned messages to storage via effect system
                if let Some(effects) = &effect_system {
                    use aura_core::effects::StorageEffects;
                    let bridge_state = state.read().await;
                    if let Some(block) = bridge_state.blocks.get(channel.as_str()) {
                        let pins_key = format!("tui/blocks/{}/pinned", channel);
                        let pins_data =
                            serde_json::to_vec(&block.pinned_messages).unwrap_or_default();
                        if let Err(e) = effects.store(&pins_key, pins_data).await {
                            tracing::warn!("Failed to persist pinned messages to storage: {:?}", e);
                        }
                    }
                }
            } else {
                tracing::warn!("UnpinMessage failed: message not found in any pinned list");
                let _ = event_tx.send(AuraEvent::Error {
                    code: "MESSAGE_NOT_FOUND".to_string(),
                    message: format!("Message {} not found in any pinned list", message_id),
                });
            }

            Ok(())
        }

        EffectCommand::SetChannelMode { channel, flags } => {
            tracing::info!("SetChannelMode: channel={}, flags={}", channel, flags);

            let actor = {
                let bridge_state = state.read().await;
                bridge_state.account_authority.clone()
            };

            if actor.is_none() {
                let _ = event_tx.send(AuraEvent::Error {
                    code: "NO_AUTHORITY".to_string(),
                    message: "No authority set - not authenticated".to_string(),
                });
                return Ok(());
            }

            let actor_id = actor.unwrap();

            // Check if actor is a steward in the channel/block
            let is_actor_steward = {
                let bridge_state = state.read().await;
                bridge_state
                    .blocks
                    .get(channel.as_str())
                    .and_then(|block| {
                        block
                            .residents
                            .iter()
                            .find(|r| r.authority_id == actor_id)
                            .map(|r| r.is_steward)
                    })
                    .unwrap_or(false)
            };

            if !is_actor_steward {
                tracing::warn!(
                    "SetChannelMode denied: actor {} is not a steward in channel {}",
                    actor_id.to_string(),
                    channel
                );
                let _ = event_tx.send(AuraEvent::AuthorizationDenied {
                    command: "SetChannelMode".to_string(),
                    required_level: CommandAuthorizationLevel::Admin,
                    reason: "Steward role required to set channel mode".to_string(),
                });
                return Ok(());
            }

            // Update mode flags
            {
                let mut bridge_state = state.write().await;
                if let Some(block) = bridge_state.blocks.get_mut(channel.as_str()) {
                    block.mode_flags = Some(flags.clone());
                    tracing::info!(
                        "SetChannelMode successful: channel {} mode set to '{}'",
                        channel,
                        flags
                    );
                    let _ = event_tx.send(AuraEvent::ChannelModeSet {
                        channel: channel.clone(),
                        flags: flags.clone(),
                        actor: actor_id.to_string(),
                    });
                } else {
                    tracing::warn!("SetChannelMode failed: channel {} not found", channel);
                    let _ = event_tx.send(AuraEvent::Error {
                        code: "CHANNEL_NOT_FOUND".to_string(),
                        message: format!("Channel {} not found", channel),
                    });
                }
            }

            // Persist channel mode to storage via effect system
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let mode_key = format!("tui/blocks/{}/mode", channel);
                if let Err(e) = effects.store(&mode_key, flags.as_bytes().to_vec()).await {
                    tracing::warn!("Failed to persist channel mode to storage: {:?}", e);
                }
            }

            Ok(())
        }

        // Account commands
        // NOTE: These commands use TreeOperationEffects (Layer 1 effect trait)
        // Account operations are direct effect calls, not protocol coordinators
        // Full implementation requires:
        // - TreeOperationEffects for commitment tree queries and updates
        // - Journal state for account authority tracking
        EffectCommand::RefreshAccount => {
            tracing::info!("RefreshAccount command executing - would call TreeOperationEffects::get_current_state()");

            // Demo: What a full implementation would look like:
            // ```
            // use aura_core::effects::tree::TreeOperationEffects;
            //
            // // Query current tree state via effects
            // let tree_state_bytes = effect_system
            //     .get_current_state()
            //     .await
            //     .map_err(|e| e.to_string())?;
            //
            // // Deserialize TreeState to extract account information
            // let tree_state: TreeState = serde_json::from_slice(&tree_state_bytes)
            //     .map_err(|e| format!("Failed to deserialize tree state: {}", e))?;
            //
            // // Extract authority ID and device information
            // let authority_id = tree_state.root_authority_id();
            // let devices = tree_state.active_devices();
            //
            // event_tx.send(AuraEvent::AccountUpdated {
            //     authority_id: authority_id.to_string(),
            // })?;
            // ```

            let authority_id = ids::uuid("tui:authority").to_string();
            let _ = event_tx.send(AuraEvent::AccountUpdated { authority_id });
            Ok(())
        }

        EffectCommand::CreateAccount { display_name } => {
            use aura_core::effects::CryptoEffects;
            use aura_core::{AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp};
            use aura_protocol::effects::TreeEffects;

            tracing::info!(
                "CreateAccount: Creating single-device account for '{}'",
                display_name
            );

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::CreateAccount {
                    name: display_name.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for CreateAccount: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // HYBRID IMPLEMENTATION: Real TreeEffects + FROST threshold signing
            // Uses real tree operations with real FROST 1-of-1 ceremony for bootstrap

            let authority_id = if let Some(ref effect_sys) = effect_system {
                tracing::info!("Using TreeEffects for account creation");

                // 1. Generate FROST 1-of-1 keys for single-device bootstrap
                // These keys will be stored and used for all future signing operations
                let frost_keys = effect_sys
                    .frost_generate_keys(1, 1) // threshold=1, max_signers=1
                    .await
                    .map_err(|e| format!("FROST key generation failed: {}", e))?;

                if frost_keys.key_packages.is_empty() {
                    return Err("FROST key generation returned no key packages".to_string());
                }

                tracing::info!(
                        "Generated FROST 1-of-1 keys: public_key_package={} bytes, key_package={} bytes",
                        frost_keys.public_key_package.len(),
                        frost_keys.key_packages[0].len()
                    );

                // 2. Store FROST keys in BridgeState for future signing operations
                {
                    let mut bridge_state = state.write().await;
                    bridge_state.frost_state = FrostState {
                        device_key_package: Some(frost_keys.key_packages[0].clone()),
                        public_key_package: Some(frost_keys.public_key_package.clone()),
                        threshold: 1,
                        max_signers: 1,
                    };
                    tracing::info!("Stored FROST keys in bridge state");
                }

                // 3. Create initial device leaf with REAL FROST public key
                let device_id = ids::device_id(&format!("device:{}", display_name));
                let leaf_id = LeafId(0); // Bootstrap leaf (first device)
                let leaf = LeafNode {
                    leaf_id,
                    device_id,
                    role: LeafRole::Device,
                    public_key: frost_keys.public_key_package.clone(), // Real FROST public key
                    meta: display_name.as_bytes().to_vec(),
                };

                // 4. Propose adding leaf to tree (returns TreeOpKind directly)
                let tree_op_kind = effect_sys
                    .add_leaf(leaf, NodeIndex(0))
                    .await
                    .map_err(|e| e.to_string())?;

                // 5. Get current tree state for parent info
                let parent_epoch = effect_sys
                    .get_current_epoch()
                    .await
                    .map_err(|e| e.to_string())?;
                let parent_commitment = effect_sys
                    .get_current_commitment()
                    .await
                    .map_err(|e| e.to_string())?;

                // 6. Wrap TreeOpKind in full TreeOp with parent state
                let tree_op = TreeOp {
                    parent_epoch,
                    parent_commitment: parent_commitment.0, // Extract inner [u8; 32]
                    op: tree_op_kind,
                    version: 1, // Protocol version
                };

                // 7. Create REAL FROST signature using the stored keys
                let agg_sig = frost_sign_tree_op_with_keys(
                    &tree_op,
                    effect_sys.as_ref(),
                    &frost_keys.key_packages[0],
                    &frost_keys.public_key_package,
                )
                .await?;

                let attested_op = AttestedOp {
                    op: tree_op,
                    agg_sig,         // Real FROST aggregate signature
                    signer_count: 1, // Single device signed
                };

                // 8. Apply attested operation to tree
                let cid = effect_sys
                    .apply_attested_op(attested_op)
                    .await
                    .map_err(|e| e.to_string())?;

                // 9. Derive authority from tree commitment
                let authority = ids::authority_id(&format!("tree:{}", hex::encode(&cid.0[..8])));
                tracing::info!("Account created with tree-derived authority: {}", authority);
                authority
            } else {
                // Fallback: Demo authority if no effect system
                tracing::warn!("No effect system available, using demo authority");
                ids::authority_id(&format!("account:{}", display_name))
            };

            // Store account in state
            {
                let mut bridge_state = state.write().await;
                bridge_state.account_authority = Some(authority_id.clone());
            }

            // Emit AccountUpdated event
            let _ = event_tx.send(AuraEvent::AccountUpdated {
                authority_id: authority_id.to_string(),
            });

            Ok(())
        }

        // Context selection command - updates current_context in BridgeState
        EffectCommand::SetContext { context_id } => {
            // Parse context_id: support named contexts or raw UUID
            let new_context = match context_id.as_str() {
                "default" | "chat" => ContextId::from_uuid(ids::uuid("tui:default-context")),
                "dm" => ContextId::from_uuid(ids::uuid("tui:dm-context")),
                _ => {
                    // Try to parse as UUID, fallback to hash-based context
                    if let Ok(uuid) = uuid::Uuid::parse_str(&context_id) {
                        ContextId::from_uuid(uuid)
                    } else {
                        ContextId::from_uuid(ids::uuid(&context_id))
                    }
                }
            };

            // Update the bridge state with new context
            {
                let mut bridge_state = state.write().await;
                bridge_state.current_context = Some(new_context);
            }

            tracing::info!("Context set to {} ({:?})", context_id, new_context);

            Ok(())
        }

        // Block commands
        // NOTE: Blocks are TUI-level social features composing multiple protocols
        // Blocks use: AMP (messaging), Invitation (membership), Relational contexts (tracking)
        // No dedicated "block protocol" exists - blocks are compositional
        // Full implementation requires:
        // - AMP for block-wide messaging channels
        // - InvitationAcceptanceCoordinator for block invitations
        // - Relational contexts for resident membership tracking
        // - Flow budget tracking for storage allocation
        EffectCommand::CreateBlock { name } => {
            tracing::info!(
                "CreateBlock: {:?} - creating block with state tracking",
                name
            );

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::CreateBlock {
                    name: name.clone().unwrap_or_else(|| "unnamed".to_string()),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for CreateBlock: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get current timestamp for block creation
            let now = time_effects
                .physical_time()
                .await
                .map_err(|e| e.to_string())?;
            let created_at = now.ts_ms;

            let mut bridge_state = state.write().await;

            // Require account authority - user must be authenticated
            let creator_authority = bridge_state.account_authority.clone().ok_or_else(|| {
                "Cannot create block: no account authority. Create an account first.".to_string()
            })?;

            // Generate unique block ID
            let block_id = ids::uuid(&format!(
                "block:{}:{}",
                name.clone().unwrap_or_else(|| "unnamed".to_string()),
                created_at
            ))
            .to_string();

            // Check if this is the first block (will be primary)
            let is_first_block = bridge_state.blocks.is_empty();

            // Create block state with creator as steward
            let mut block_state = BlockState::new(
                block_id.clone(),
                name.clone(),
                creator_authority,
                created_at,
            );

            // Only the first block is primary by default
            block_state.is_primary = is_first_block;

            // Store in blocks map
            bridge_state.blocks.insert(block_id.clone(), block_state);

            // Set as current block
            bridge_state.current_block = Some(block_id.clone());

            tracing::info!(
                "Block created: id={}, name={:?}, is_primary={}, residents=1, storage_budget=10MB",
                block_id,
                name,
                is_first_block
            );

            drop(bridge_state);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::BlockCreated {
                block_id,
                name: name.clone(),
            });

            Ok(())
        }

        EffectCommand::AcceptPendingBlockInvitation => {
            tracing::info!("AcceptPendingBlockInvitation - accepting block invitation");

            // Get current timestamp
            let now = time_effects
                .physical_time()
                .await
                .map_err(|e| e.to_string())?;
            let joined_at = now.ts_ms;

            let mut bridge_state = state.write().await;

            // Require account authority
            let user_authority = bridge_state.account_authority.clone().ok_or_else(|| {
                "Cannot accept block invitation: no account authority".to_string()
            })?;

            // Find the first pending block invitation for this user
            let invitation = bridge_state
                .pending_block_invitations
                .values()
                .find(|inv| inv.invitee_authority == user_authority && inv.expires_at > joined_at)
                .cloned();

            let envelope =
                invitation.ok_or_else(|| "No pending block invitation found".to_string())?;

            // Remove from pending invitations
            bridge_state
                .pending_block_invitations
                .remove(&envelope.invitation_id);

            // Check if block already exists (we might already have it from being steward)
            if !bridge_state.blocks.contains_key(&envelope.block_id) {
                // Create a new block state for the joined block
                // The user joins as a regular resident (not steward)
                let block_state = BlockState {
                    id: envelope.block_id.clone(),
                    name: envelope.block_name.clone(),
                    created_at: envelope.created_at,
                    topic: None,
                    mode_flags: None,
                    pinned_messages: Vec::new(),
                    ban_list: HashMap::new(),
                    mute_list: HashMap::new(),
                    kick_log: Vec::new(),
                    context_id: aura_core::ContextId::new_from_entropy(hash(
                        envelope.block_id.as_bytes(),
                    )),
                    residents: vec![
                        ResidentState {
                            authority_id: envelope.inviter_authority.clone(),
                            name: "Steward".to_string(), // The inviter is the steward
                            is_steward: true,
                            joined_at: envelope.created_at,
                            storage_allocated: BlockState::RESIDENT_ALLOCATION,
                        },
                        ResidentState {
                            authority_id: user_authority.clone(),
                            name: "You".to_string(),
                            is_steward: false, // Joining as resident, not steward
                            joined_at,
                            storage_allocated: BlockState::RESIDENT_ALLOCATION,
                        },
                    ],
                    storage_budget: BlockState::DEFAULT_STORAGE_BUDGET,
                    storage_used: BlockState::RESIDENT_ALLOCATION * 2, // Two residents
                    is_primary: false, // Joined blocks are not primary by default
                };

                bridge_state
                    .blocks
                    .insert(envelope.block_id.clone(), block_state);
            } else {
                // Block exists, add user as resident
                if let Some(block) = bridge_state.blocks.get_mut(&envelope.block_id) {
                    let _ =
                        block.add_resident(user_authority.clone(), "You".to_string(), joined_at);
                }
            }

            // Set as current block if no current block
            if bridge_state.current_block.is_none() {
                bridge_state.current_block = Some(envelope.block_id.clone());
            }

            tracing::info!(
                "Block joined: id={}, name={:?}",
                envelope.block_id,
                envelope.block_name
            );

            let block_id = envelope.block_id.clone();
            drop(bridge_state);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::BlockJoined { block_id });

            Ok(())
        }

        EffectCommand::SendBlockInvitation { contact_id } => {
            tracing::info!(
                "SendBlockInvitation to: {} - creating block invitation",
                contact_id
            );

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                // Get current block ID from bridge state
                let block_id_str = {
                    let bridge_state = state.read().await;
                    bridge_state.current_block.clone()
                };
                if let Some(block_id_str) = block_id_str {
                    let block_context_id =
                        ContextId::from_str(&block_id_str).unwrap_or_else(|_| {
                            let h = hash::hash(block_id_str.as_bytes());
                            let mut uuid_bytes = [0u8; 16];
                            uuid_bytes.copy_from_slice(&h[..16]);
                            ContextId::from_uuid(uuid::Uuid::from_bytes(uuid_bytes))
                        });
                    let intent = Intent::InviteToBlock {
                        block_id: block_context_id,
                        invitee_id: contact_id.clone(),
                    };
                    let mut core_guard = core.write().await;
                    if let Err(e) = core_guard.dispatch(intent) {
                        tracing::warn!("AppCore dispatch failed for InviteToBlock: {}", e);
                    }
                    // Commit pending facts and persist to storage
                    if let Err(e) = core_guard.commit_and_persist() {
                        tracing::warn!("Failed to persist facts: {}", e);
                    }
                }
            }

            // Get current timestamp
            let now = time_effects
                .physical_time()
                .await
                .map_err(|e| e.to_string())?;
            let created_at = now.ts_ms;
            let expires_at = created_at + (7 * 24 * 60 * 60 * 1000); // 7 days

            let mut bridge_state = state.write().await;

            // Require account authority
            let inviter_authority = bridge_state
                .account_authority
                .clone()
                .ok_or_else(|| "Cannot send block invitation: no account authority".to_string())?;

            // Require a current block
            let block_id = bridge_state.current_block.clone().ok_or_else(|| {
                "Cannot send block invitation: no current block. Create a block first.".to_string()
            })?;

            // Get block info
            let block = bridge_state
                .blocks
                .get(&block_id)
                .ok_or_else(|| "Block not found in state".to_string())?;
            let block_name = block.name.clone();

            // Verify user is a steward of the block (can send invitations)
            let is_steward = block
                .residents
                .iter()
                .any(|r| r.authority_id == inviter_authority && r.is_steward);
            if !is_steward {
                return Err(
                    "Cannot send block invitation: you are not a steward of this block".to_string(),
                );
            }

            // Parse invitee authority from contact_id
            // For demo purposes, create a synthetic authority from the contact_id
            let invitee_authority =
                aura_core::AuthorityId::from_str(&contact_id).unwrap_or_else(|_| {
                    // Create synthetic authority from contact_id
                    aura_core::AuthorityId::from_uuid(ids::uuid(&format!("contact:{}", contact_id)))
                });

            // Generate unique invitation ID
            let invitation_id =
                ids::uuid(&format!("block-invitation:{}:{}", block_id, created_at)).to_string();

            // Create block invitation envelope
            let envelope = BlockInvitationEnvelope {
                invitation_id: invitation_id.clone(),
                block_id: block_id.clone(),
                block_name: block_name.clone(),
                inviter_authority,
                invitee_authority,
                created_at,
                expires_at,
            };

            // Store in sent invitations
            bridge_state
                .sent_block_invitations
                .insert(invitation_id.clone(), envelope);

            tracing::info!(
                "Block invitation sent: id={}, block={}, recipient={}",
                invitation_id,
                block_id,
                contact_id
            );

            drop(bridge_state);

            // Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::InvitationSent {
                invitation_id,
                recipient: contact_id.clone(),
            });

            Ok(())
        }

        // Sync commands
        // NOTE: These commands use aura-sync protocols (JournalSyncProtocol, AntiEntropyProtocol)
        // Full implementation requires:
        // - JournalSyncProtocol for explicit state synchronization
        // - AntiEntropyProtocol for background reconciliation
        // - PeerManager for tracking sync peers
        // - SessionManager for protocol coordination
        EffectCommand::ForceSync => {
            // Wire to SyncEffects (AuraEffectSystem implements SyncEffects)
            //
            // Sync with all known peers. If no peers are registered, fall back to demo peer.
            // Use AddPeer command to register peers for sync.

            // Get known peers from state
            let bridge_state = state.read().await;
            let known_peers = bridge_state.known_peers.clone();
            drop(bridge_state);

            // Determine which peers to sync with
            let peers_to_sync: Vec<uuid::Uuid> = if known_peers.is_empty() {
                // No known peers - use demo peer for backwards compatibility
                tracing::info!("No known peers registered, using demo peer");
                vec![ids::uuid("tui:sync-peer")]
            } else {
                tracing::info!("Syncing with {} known peers", known_peers.len());
                known_peers
            };

            let mut total_changes = 0u32;
            let mut sync_errors = 0u32;

            // Sync with each peer
            for peer_uuid in peers_to_sync {
                let peer_id = peer_uuid.to_string();

                let _ = event_tx.send(AuraEvent::SyncStarted {
                    peer_id: peer_id.clone(),
                });

                // Call sync_with_peer if effect_system is available
                if let Some(ref effect_system) = effect_system {
                    match effect_system.sync_with_peer(peer_uuid).await {
                        Ok(metrics) => {
                            tracing::info!(
                                "Sync completed with peer {}: {} applied, {} duplicates, {} rounds",
                                peer_id,
                                metrics.applied,
                                metrics.duplicates,
                                metrics.rounds
                            );
                            total_changes += metrics.applied as u32;
                            let _ = event_tx.send(AuraEvent::SyncCompleted {
                                peer_id,
                                changes: metrics.applied as u32,
                            });
                        }
                        Err(e) => {
                            tracing::error!("Sync failed with peer {}: {:?}", peer_id, e);
                            sync_errors += 1;
                            // Emit failed event for this peer
                            let _ = event_tx.send(AuraEvent::SyncFailed {
                                peer_id,
                                reason: format!("{:?}", e),
                            });
                        }
                    }
                } else {
                    tracing::debug!("Effect system not available for sync, emitting stub events");
                    let _ = event_tx.send(AuraEvent::SyncCompleted {
                        peer_id,
                        changes: 0,
                    });
                }
            }

            if sync_errors > 0 {
                tracing::warn!(
                    "ForceSync completed with {} errors, {} total changes",
                    sync_errors,
                    total_changes
                );
            } else {
                tracing::info!("ForceSync completed: {} total changes", total_changes);
            }

            // Phase 5.1: After sync completes with changes, reload journal to update ViewState
            // This enables the AuraEvent → ViewDelta flow: sync'd facts get applied to AppCore
            if total_changes > 0 {
                if let Some(ref core) = app_core {
                    let mut app = core.write().await;
                    if let Some(path) = app.journal_path() {
                        let path = std::path::PathBuf::from(path);
                        match app.load_from_storage(&path) {
                            Ok(count) => {
                                tracing::info!(
                                    "Reloaded {} facts from journal after sync ({} changes)",
                                    count,
                                    total_changes
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to reload journal after sync: {}", e);
                            }
                        }
                    }
                }
            }

            Ok(())
        }

        EffectCommand::RequestState { peer_id } => {
            // Wire to SyncEffects for state sync with a specific peer
            //
            // This command requests full state synchronization from a specific peer.
            // Uses sync_with_peer which handles anti-entropy protocol internally.

            let peer_id_owned = peer_id.clone();
            let _ = event_tx.send(AuraEvent::SyncStarted {
                peer_id: peer_id_owned.clone(),
            });

            // Parse peer_id string to UUID and call sync_with_peer
            let peer_uuid = uuid::Uuid::parse_str(&peer_id_owned).map_err(|e| {
                tracing::error!("Invalid peer UUID '{}': {:?}", peer_id_owned, e);
                format!("Invalid peer UUID: {:?}", e)
            })?;

            let mut changes_applied = 0u32;

            if let Some(effect_system) = effect_system {
                match effect_system.sync_with_peer(peer_uuid).await {
                    Ok(metrics) => {
                        changes_applied = metrics.applied as u32;
                        tracing::info!(
                            "State sync completed with peer {}: {} applied, {} duplicates, {} rounds",
                            peer_id_owned, metrics.applied, metrics.duplicates, metrics.rounds
                        );
                        let _ = event_tx.send(AuraEvent::SyncCompleted {
                            peer_id: peer_id_owned,
                            changes: changes_applied,
                        });
                    }
                    Err(e) => {
                        tracing::error!("State sync failed with peer {}: {:?}", peer_id_owned, e);
                        // Emit completed with 0 changes to indicate sync attempted
                        let _ = event_tx.send(AuraEvent::SyncCompleted {
                            peer_id: peer_id_owned,
                            changes: 0,
                        });
                    }
                }
            } else {
                tracing::debug!(
                    "Effect system not available for state sync with peer {}",
                    peer_id_owned
                );
                let _ = event_tx.send(AuraEvent::SyncCompleted {
                    peer_id: peer_id_owned,
                    changes: 0,
                });
            }

            // Phase 5.1: After sync completes with changes, reload journal to update ViewState
            if changes_applied > 0 {
                if let Some(ref core) = app_core {
                    let mut app = core.write().await;
                    if let Some(path) = app.journal_path() {
                        let path = std::path::PathBuf::from(path);
                        match app.load_from_storage(&path) {
                            Ok(count) => {
                                tracing::info!(
                                    "Reloaded {} facts from journal after RequestState ({} changes)",
                                    count,
                                    changes_applied
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Failed to reload journal after RequestState: {}", e);
                            }
                        }
                    }
                }
            }

            Ok(())
        }

        EffectCommand::AddPeer { peer_id } => {
            tracing::info!("AddPeer command: {}", peer_id);

            // Parse peer_id string to UUID
            let peer_uuid = uuid::Uuid::parse_str(&peer_id).map_err(|e| {
                tracing::error!("Invalid peer UUID '{}': {:?}", peer_id, e);
                format!("Invalid peer UUID: {:?}", e)
            })?;

            // Add to known peers list
            let mut bridge_state = state.write().await;
            if !bridge_state.known_peers.contains(&peer_uuid) {
                bridge_state.known_peers.push(peer_uuid);
                tracing::info!("Added peer {} to known peers list", peer_id);
            } else {
                tracing::debug!("Peer {} already in known peers list", peer_id);
            }
            drop(bridge_state);

            // Emit event
            let _ = event_tx.send(AuraEvent::PeerAdded {
                peer_id: peer_id.clone(),
            });

            Ok(())
        }

        EffectCommand::RemovePeer { peer_id } => {
            tracing::info!("RemovePeer command: {}", peer_id);

            // Parse peer_id string to UUID
            let peer_uuid = uuid::Uuid::parse_str(&peer_id).map_err(|e| {
                tracing::error!("Invalid peer UUID '{}': {:?}", peer_id, e);
                format!("Invalid peer UUID: {:?}", e)
            })?;

            // Remove from known peers list
            let mut bridge_state = state.write().await;
            bridge_state.known_peers.retain(|p| *p != peer_uuid);
            drop(bridge_state);

            tracing::info!("Removed peer {} from known peers list", peer_id);

            // Emit event
            let _ = event_tx.send(AuraEvent::PeerRemoved {
                peer_id: peer_id.clone(),
            });

            Ok(())
        }

        EffectCommand::ListPeers => {
            tracing::info!("ListPeers command");

            // Get known peers
            let bridge_state = state.read().await;
            let peers: Vec<String> = bridge_state
                .known_peers
                .iter()
                .map(|p| p.to_string())
                .collect();
            let peer_count = peers.len();
            drop(bridge_state);

            tracing::info!("Known peers: {}", peer_count);

            // Emit event
            let _ = event_tx.send(AuraEvent::PeersListed { peers });

            Ok(())
        }

        EffectCommand::DiscoverPeers => {
            // Discover peers from the sync effect system and add them to known_peers
            //
            // This command queries the effect system for connected/discovered peers
            // and adds any new ones to the known_peers list. The actual discovery
            // source depends on the effect system configuration:
            // - In production: queries rendezvous service for cached peer descriptors
            // - In testing/demo: may return empty or mock peers
            //
            // After discovery, ForceSync will use these peers instead of the demo peer.

            tracing::info!("DiscoverPeers command - querying effect system for peers");

            let mut discovered_count = 0u32;
            let mut new_peers_added = 0u32;

            if let Some(ref effects) = effect_system {
                // Query the sync effect system for connected peers
                match effects.get_connected_peers().await {
                    Ok(peers) => {
                        discovered_count = peers.len() as u32;
                        tracing::info!("Discovered {} peers from effect system", discovered_count);

                        // Add new peers to known_peers list
                        let mut bridge_state = state.write().await;
                        for peer_uuid in peers {
                            if !bridge_state.known_peers.contains(&peer_uuid) {
                                bridge_state.known_peers.push(peer_uuid);
                                new_peers_added += 1;
                                tracing::debug!("Added discovered peer: {}", peer_uuid);
                            }
                        }
                        drop(bridge_state);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to discover peers: {:?}", e);
                    }
                }
            } else {
                tracing::debug!("Effect system not available for peer discovery");
            }

            // Get the final list of known peers
            let bridge_state = state.read().await;
            let all_peers: Vec<String> = bridge_state
                .known_peers
                .iter()
                .map(|p| p.to_string())
                .collect();
            drop(bridge_state);

            tracing::info!(
                "Peer discovery complete: {} discovered, {} new, {} total known",
                discovered_count,
                new_peers_added,
                all_peers.len()
            );

            // Emit discovery completed event
            let _ = event_tx.send(AuraEvent::PeersDiscovered {
                discovered: discovered_count,
                new_peers: new_peers_added,
                total: all_peers.len() as u32,
            });

            Ok(())
        }

        // === Section 9: Neighborhood Traversal ===
        EffectCommand::MovePosition {
            neighborhood_id,
            block_id,
            depth,
        } => {
            // Traversal protocol implementation:
            // - Basic position tracking in local state + events
            // - Persists traversal position to storage
            // - Full adjacency/authorization validation deferred to neighborhood protocol

            tracing::info!(
                "MovePosition: neighborhood={}, block={}, depth={}",
                neighborhood_id,
                block_id,
                depth
            );

            // Update in-memory state
            let mut state = state.write().await;
            state.traversal_position = Some(TraversalPosition {
                neighborhood_id: neighborhood_id.clone(),
                block_id: block_id.clone(),
                depth: depth.clone(),
            });
            drop(state);

            // Emit position updated event
            let _ = event_tx.send(AuraEvent::PositionUpdated {
                neighborhood_id: neighborhood_id.clone(),
                block_id: block_id.clone(),
                depth: depth.clone(),
            });

            // Persist traversal position to storage via effect system
            if let Some(effects) = &effect_system {
                use aura_core::effects::StorageEffects;
                let pos_key = format!("tui/traversal/{}", neighborhood_id);
                #[derive(serde::Serialize)]
                struct StoredPosition {
                    neighborhood_id: String,
                    block_id: String,
                    depth: String,
                }
                let stored = StoredPosition {
                    neighborhood_id: neighborhood_id.clone(),
                    block_id: block_id.clone(),
                    depth: depth.clone(),
                };
                let pos_data = serde_json::to_vec(&stored).unwrap_or_default();
                if let Err(e) = effects.store(&pos_key, pos_data).await {
                    tracing::warn!("Failed to persist traversal position to storage: {:?}", e);
                }
            }

            tracing::info!("Position updated in local state");
            Ok(())
        }

        // Settings commands
        // NOTE: These commands use TreeOperationEffects for tree modifications
        // Settings operations directly call Layer 1 effect traits
        // Full implementation requires:
        // - TreeOperationEffects for policy changes and leaf operations
        // - Threshold ceremonies for operation attestation
        // - Consensus coordination for multi-device approval
        EffectCommand::UpdateThreshold {
            threshold_k,
            threshold_n,
        } => {
            use aura_core::{AttestedOp, NodeIndex, Policy, TreeOp};
            use aura_protocol::effects::TreeEffects;

            tracing::info!(
                "UpdateThreshold command executing: changing to {}-of-{}",
                threshold_k,
                threshold_n
            );

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                let intent = Intent::UpdateThreshold {
                    threshold: *threshold_k as u32,
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for UpdateThreshold: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // Get effect system reference
            let effect_sys =
                effect_system.ok_or("Effect system not available for UpdateThreshold")?;

            // 1. Create new policy with updated threshold
            let new_policy = Policy::Threshold {
                m: *threshold_k as u16, // Convert u8 to u16
                n: *threshold_n as u16, // Convert u8 to u16
            };

            // 2. Call change_policy to create the policy change operation
            let tree_op_kind = effect_sys
                .change_policy(NodeIndex(0), new_policy)
                .await
                .map_err(|e| format!("Failed to change policy: {}", e))?;

            tracing::debug!("Created policy change operation: {:?}", tree_op_kind);

            // 3. Get current tree state for parent binding
            let parent_epoch = effect_sys
                .get_current_epoch()
                .await
                .map_err(|e| format!("Failed to get current epoch: {}", e))?;
            let parent_commitment = effect_sys
                .get_current_commitment()
                .await
                .map_err(|e| format!("Failed to get current commitment: {}", e))?;

            // 4. Create TreeOp with parent binding
            let tree_op = TreeOp {
                parent_epoch,
                parent_commitment: parent_commitment.0,
                op: tree_op_kind,
                version: 1,
            };

            tracing::debug!(
                "TreeOp created with parent_epoch={}, parent_commitment={:?}",
                tree_op.parent_epoch,
                tree_op.parent_commitment
            );

            // 5. Create REAL FROST 1-of-1 attestation
            // NOTE: Multi-device mode uses the OLD threshold to authorize the change
            // (i.e., if changing from 2-of-3 to 3-of-5, need 2 signers from old policy)
            // For single-device demo, we use 1-of-1 FROST signing
            let agg_sig = frost_sign_tree_op(&tree_op, effect_sys.as_ref()).await?;

            let attested_op = AttestedOp {
                op: tree_op,
                agg_sig,         // Real FROST aggregate signature
                signer_count: 1, // Single device signed
            };

            tracing::info!("Created attested policy change operation with real FROST signature");

            // 6. Apply the attested operation to the tree
            let new_commitment = effect_sys
                .apply_attested_op(attested_op)
                .await
                .map_err(|e| format!("Failed to apply policy change: {}", e))?;

            tracing::info!(
                "Successfully changed threshold to {}-of-{}, new commitment: {:?}",
                threshold_k,
                threshold_n,
                new_commitment
            );

            // 7. Emit event for reactive view updates
            let _ = event_tx.send(AuraEvent::ThresholdUpdated {
                threshold_k: *threshold_k,
                threshold_n: *threshold_n,
            });

            Ok(())
        }

        EffectCommand::AddDevice { device_name } => {
            use aura_core::{AttestedOp, LeafId, LeafNode, LeafRole, NodeIndex, TreeOp};
            use aura_protocol::effects::TreeEffects;

            tracing::info!("AddDevice: Adding new device '{}' to account", device_name);

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                // Get authority ID from bridge state
                let authority_id = {
                    let bridge_state = state.read().await;
                    bridge_state
                        .account_authority
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "default".to_string())
                };
                // Note: Intent::AddDevice expects public_key, using device_name as placeholder
                // Real implementation would generate/receive actual public key
                let intent = Intent::AddDevice {
                    authority_id,
                    public_key: device_name.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for AddDevice: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // HYBRID IMPLEMENTATION: Real TreeEffects + FROST threshold signing
            // Uses real tree operations with stored FROST keys for authorization

            let device_id = if let Some(ref effect_sys) = effect_system {
                tracing::info!("Using TreeEffects for device addition");

                // 1. Retrieve stored FROST keys for signing authorization
                let (key_package, public_key_package) = {
                    let bridge_state = state.read().await;
                    let key_pkg = bridge_state
                        .frost_state
                        .device_key_package
                        .clone()
                        .ok_or_else(|| {
                            "No FROST key package stored. Create an account first.".to_string()
                        })?;
                    let pub_pkg = bridge_state
                        .frost_state
                        .public_key_package
                        .clone()
                        .ok_or_else(|| {
                            "No FROST public key stored. Create an account first.".to_string()
                        })?;
                    (key_pkg, pub_pkg)
                };

                tracing::info!("Retrieved stored FROST keys for AddDevice signing");

                // 2. Get current tree state to determine next leaf ID
                let tree_state = effect_sys
                    .get_current_state()
                    .await
                    .map_err(|e| e.to_string())?;

                // Compute next leaf ID from current leaf count
                let next_leaf_id = LeafId(tree_state.num_leaves() as u32 + 1);

                // 3. Create new device leaf with the stored FROST public key
                // NOTE: In a real multi-device setup, the new device would generate its own
                // key share via DKG resharing. For now, we use the same public key.
                let new_device_id = ids::device_id(&format!("device:{}", device_name));
                let leaf = LeafNode {
                    leaf_id: next_leaf_id,
                    device_id: new_device_id.clone(),
                    role: LeafRole::Device,
                    public_key: public_key_package.clone(), // Use stored FROST public key
                    meta: device_name.as_bytes().to_vec(),
                };

                // 4. Propose adding leaf to tree
                let tree_op_kind = effect_sys
                    .add_leaf(leaf, NodeIndex(0)) // Add under root
                    .await
                    .map_err(|e| e.to_string())?;

                // 5. Get current tree state for parent info
                let parent_epoch = effect_sys
                    .get_current_epoch()
                    .await
                    .map_err(|e| e.to_string())?;
                let parent_commitment = effect_sys
                    .get_current_commitment()
                    .await
                    .map_err(|e| e.to_string())?;

                // 6. Wrap TreeOpKind in full TreeOp with parent state
                let tree_op = TreeOp {
                    parent_epoch,
                    parent_commitment: parent_commitment.0,
                    op: tree_op_kind,
                    version: 1,
                };

                // 7. Sign with stored FROST keys
                // Uses the account's FROST keys to authorize the device addition
                // FUTURE: For multi-device, coordinate signing ceremony with other devices
                let agg_sig = frost_sign_tree_op_with_keys(
                    &tree_op,
                    effect_sys.as_ref(),
                    &key_package,
                    &public_key_package,
                )
                .await?;

                let attested_op = AttestedOp {
                    op: tree_op,
                    agg_sig,         // Real FROST aggregate signature
                    signer_count: 1, // Single device signed
                };

                // 8. Apply attested operation to tree
                let cid = effect_sys
                    .apply_attested_op(attested_op)
                    .await
                    .map_err(|e| e.to_string())?;

                tracing::info!(
                    "Device added to tree with CID: {}",
                    hex::encode(&cid.0[..8])
                );
                new_device_id.to_string()
            } else {
                // Fallback: Demo device ID if no effect system
                tracing::warn!("No effect system available, using demo device ID");
                ids::uuid(&format!("device:{}", device_name)).to_string()
            };

            // Emit DeviceAdded event for UI updates
            let _ = event_tx.send(AuraEvent::DeviceAdded { device_id });

            Ok(())
        }

        EffectCommand::RemoveDevice { device_id } => {
            use aura_core::{AttestedOp, TreeOp};
            use aura_protocol::effects::TreeEffects;

            tracing::info!("RemoveDevice: Removing device '{}'", device_id);

            // Dispatch intent to AppCore for state management
            if let Some(core) = app_core {
                // Get authority ID from bridge state
                let authority_id = {
                    let bridge_state = state.read().await;
                    bridge_state
                        .account_authority
                        .as_ref()
                        .map(|a| a.to_string())
                        .unwrap_or_else(|| "default".to_string())
                };
                let intent = Intent::RemoveDevice {
                    authority_id,
                    device_id: device_id.clone(),
                };
                let mut core_guard = core.write().await;
                if let Err(e) = core_guard.dispatch(intent) {
                    tracing::warn!("AppCore dispatch failed for RemoveDevice: {}", e);
                }
                // Commit pending facts and persist to storage
                if let Err(e) = core_guard.commit_and_persist() {
                    tracing::warn!("Failed to persist facts: {}", e);
                }
            }

            // HYBRID IMPLEMENTATION: Real TreeEffects + simplified attestation
            // Uses real tree operations but simplified signing for demo/testing

            if let Some(ref effect_sys) = effect_system {
                tracing::info!("Using TreeEffects for device removal");

                // 1. Look up leaf ID from device ID in tree state
                let tree_state = effect_sys
                    .get_current_state()
                    .await
                    .map_err(|e| e.to_string())?;

                // Find the leaf with matching device_id
                let leaf_id = tree_state
                    .leaves
                    .iter()
                    .find(|(_, leaf)| leaf.device_id.to_string() == *device_id)
                    .map(|(id, _)| *id)
                    .ok_or_else(|| format!("Device '{}' not found in tree", device_id))?;

                // 2. Propose removing leaf from tree
                // Reason codes: 0 = voluntary, 1 = compromised, 2 = lost
                let tree_op_kind = effect_sys
                    .remove_leaf(leaf_id, 0) // Voluntary removal
                    .await
                    .map_err(|e| e.to_string())?;

                // 3. Get current tree state for parent info
                let parent_epoch = effect_sys
                    .get_current_epoch()
                    .await
                    .map_err(|e| e.to_string())?;
                let parent_commitment = effect_sys
                    .get_current_commitment()
                    .await
                    .map_err(|e| e.to_string())?;

                // 4. Wrap TreeOpKind in full TreeOp with parent state
                let tree_op = TreeOp {
                    parent_epoch,
                    parent_commitment: parent_commitment.0,
                    op: tree_op_kind,
                    version: 1,
                };

                // 5. Create REAL FROST 1-of-1 attestation
                // Uses genuine FROST threshold cryptography
                // FUTURE: Run real FROST threshold ceremony with remaining devices
                // IMPORTANT: Device being removed CANNOT participate in signing
                let agg_sig = frost_sign_tree_op(&tree_op, effect_sys.as_ref()).await?;

                let attested_op = AttestedOp {
                    op: tree_op,
                    agg_sig,         // Real FROST aggregate signature
                    signer_count: 1, // Single device signed (excluding removed device)
                };

                // 6. Apply attested operation to tree
                let cid = effect_sys
                    .apply_attested_op(attested_op)
                    .await
                    .map_err(|e| e.to_string())?;

                tracing::info!(
                    "Device removed from tree with CID: {}",
                    hex::encode(&cid.0[..8])
                );
            } else {
                tracing::warn!("No effect system available, using demo device removal");
            }

            // Emit DeviceRemoved event for UI updates
            let _ = event_tx.send(AuraEvent::DeviceRemoved {
                device_id: device_id.clone(),
            });

            Ok(())
        }

        EffectCommand::UpdateMfaPolicy { require_mfa } => {
            // NOTE: In Aura, "MFA" is achieved through m-of-n threshold cryptography.
            // The threshold itself (m, n) IS the multi-factor policy - each signer/device
            // is a "factor", and requiring m-of-n provides multi-factor authentication.
            //
            // The actual threshold is set via UpdateThreshold command. This UpdateMfaPolicy
            // command exists primarily for UI display purposes - `require_mfa: true` indicates
            // that the account has m > 1 (multi-factor), while `require_mfa: false` indicates
            // m = 1 (single factor).
            //
            // Users start with 1-of-1 (single device) and can expand to m-of-n (multi-device)
            // via UpdateThreshold at any time. There's no separate "demo" vs "production" mode.

            tracing::info!(
                "UpdateMfaPolicy: MFA is {}",
                if *require_mfa {
                    "enabled (m > 1 threshold)"
                } else {
                    "disabled (1-of-1 threshold)"
                }
            );

            // Emit event for reactive view updates (UI display)
            let _ = event_tx.send(AuraEvent::MfaPolicyUpdated {
                require_mfa: *require_mfa,
            });

            Ok(())
        }

        // System commands
        EffectCommand::Ping => {
            let _ = event_tx.send(AuraEvent::Pong { latency_ms: 10 });
            Ok(())
        }

        EffectCommand::Shutdown => {
            let _ = event_tx.send(AuraEvent::ShuttingDown);
            Ok(())
        }
    }
}
