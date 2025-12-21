//! # Intents: User Actions as Facts
//!
//! An intent represents a user action that will become a fact in the journal.
//! This follows the CQRS pattern where intents are the "write" side.
//!
//! ## Flow
//!
//! ```text
//! Intent → Authorize (Biscuit) → Journal → Reduce → View → Sync
//! ```

use aura_core::effects::intent::{AuthorizationLevel, IntentMetadata};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::TimeStamp;
use aura_journal::JournalFact;
use serde::{Deserialize, Serialize};

/// Screen identifier for navigation intents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Screen {
    /// Block/home screen
    Block,
    /// Chat screen
    Chat,
    /// Recovery screen
    Recovery,
    /// Invitations screen
    Invitations,
    /// Neighborhood screen
    Neighborhood,
    /// Contacts screen
    Contacts,
    /// Settings screen
    Settings,
    /// Help screen
    Help,
}

/// Channel type for chat
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ChannelType {
    /// Block-level messaging
    Block,
    /// Direct message
    DirectMessage,
    /// Guardian chat
    Guardian,
}

/// Invitation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum InvitationType {
    /// Invite to block
    Block,
    /// Invite as guardian
    Guardian,
    /// Invite to chat
    Chat,
}

/// An intent is a user action that becomes a fact.
///
/// Intents flow through: Intent → Authorize → Journal → Reduce → View → Sync
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum Intent {
    // =========================================================================
    // Chat Intents
    // =========================================================================
    /// Send a message to a channel
    SendMessage {
        /// Target channel
        channel_id: ContextId,
        /// Message content
        content: String,
        /// Optional message to reply to (fact ID as string)
        reply_to: Option<String>,
    },

    /// Create a new channel
    CreateChannel {
        /// Channel name
        name: String,
        /// Channel type
        channel_type: ChannelType,
    },

    /// Mark messages as read
    MarkAsRead {
        /// Channel to mark as read
        channel_id: ContextId,
        /// Read up to this message (fact ID as string)
        up_to_message: String,
    },

    /// Edit an existing message
    EditMessage {
        /// Channel containing the message
        channel_id: ContextId,
        /// Message to edit (fact ID as string)
        message_id: String,
        /// New content
        content: String,
    },

    /// Delete a message
    DeleteMessage {
        /// Channel containing the message
        channel_id: ContextId,
        /// Message to delete (fact ID as string)
        message_id: String,
    },

    /// Invite a member to a channel
    InviteMember {
        /// Target channel
        channel_id: ContextId,
        /// Authority to invite
        member_id: String,
    },

    /// Join a channel
    JoinChannel {
        /// Channel to join
        channel_id: ContextId,
    },

    /// Leave a channel
    LeaveChannel {
        /// Channel to leave
        channel_id: ContextId,
    },

    /// Remove a member from a channel
    RemoveMember {
        /// Target channel
        channel_id: ContextId,
        /// Authority to remove
        member_id: String,
    },

    /// Update channel details
    UpdateChannel {
        /// Channel to update
        channel_id: ContextId,
        /// New name (if any)
        name: Option<String>,
        /// New description (if any)
        description: Option<String>,
    },

    // =========================================================================
    // Recovery Intents
    // =========================================================================
    /// Initiate account recovery
    InitiateRecovery,

    /// Approve a recovery request as a guardian
    ApproveRecovery {
        /// Recovery context to approve
        recovery_context: ContextId,
    },

    /// Reject a recovery request as a guardian
    RejectRecovery {
        /// Recovery context to reject
        recovery_context: ContextId,
        /// Reason for rejection
        reason: String,
    },

    /// Complete the recovery process after threshold approvals
    ///
    /// This is called after guardians have provided enough approvals via FROST
    /// threshold signatures. The `recovered_authority_id` is the ORIGINAL
    /// authority_id reconstructed by guardians - this must be cryptographically
    /// identical to the authority before catastrophic device loss.
    CompleteRecovery {
        /// Recovery context to complete
        recovery_context: ContextId,
        /// The original authority_id reconstructed by guardians via FROST
        /// This is NOT derived from device_id - it's the actual recovered identity
        recovered_authority_id: Option<AuthorityId>,
    },

    /// Set the guardian threshold for recovery
    SetGuardianThreshold {
        /// Required number of guardian approvals
        threshold: u32,
    },

    // =========================================================================
    // Invitation Intents
    // =========================================================================
    /// Create an invitation
    CreateInvitation {
        /// Type of invitation
        invitation_type: InvitationType,
    },

    /// Accept an invitation
    AcceptInvitation {
        /// Invitation fact ID as string
        invitation_fact: String,
    },

    /// Reject an invitation
    RejectInvitation {
        /// Invitation fact ID as string
        invitation_fact: String,
    },

    /// Revoke a sent invitation
    RevokeInvitation {
        /// Invitation fact ID as string
        invitation_fact: String,
    },

    // =========================================================================
    // Contact Intents
    // =========================================================================
    /// Set a nickname for a contact
    SetNickname {
        /// Contact authority ID as string
        contact_id: String,
        /// Nickname to set
        nickname: String,
    },

    /// Remove a contact
    RemoveContact {
        /// Contact authority ID as string
        contact_id: String,
    },

    /// Toggle guardian status for a contact
    ToggleGuardian {
        /// Contact authority ID as string
        contact_id: String,
        /// Whether to enable or disable guardian status
        is_guardian: bool,
    },

    // =========================================================================
    // Block Intents
    // =========================================================================
    /// Create a new block
    CreateBlock {
        /// Block name
        name: String,
    },

    /// Invite someone to a block
    InviteToBlock {
        /// Block ID to invite to
        block_id: ContextId,
        /// Authority ID to invite
        invitee_id: String,
    },

    /// Set the block name
    SetBlockName {
        /// Block ID
        block_id: ContextId,
        /// New name
        name: String,
    },

    /// Update block storage settings
    UpdateBlockStorage {
        /// Block ID
        block_id: ContextId,
        /// New storage budget in bytes
        storage_budget: u64,
    },

    /// Set block topic
    SetBlockTopic {
        /// Block ID
        block_id: ContextId,
        /// Topic text
        topic: String,
    },

    // =========================================================================
    // Block Moderation Intents
    // =========================================================================
    /// Ban a user from a block
    BanUser {
        /// Block ID
        block_id: ContextId,
        /// Authority ID to ban
        target_id: String,
        /// Reason for ban
        reason: String,
    },

    /// Unban a user from a block
    UnbanUser {
        /// Block ID
        block_id: ContextId,
        /// Authority ID to unban
        target_id: String,
    },

    /// Mute a user in a block
    MuteUser {
        /// Block ID
        block_id: ContextId,
        /// Authority ID to mute
        target_id: String,
        /// Duration in seconds (None = permanent)
        duration_secs: Option<u64>,
    },

    /// Unmute a user in a block
    UnmuteUser {
        /// Block ID
        block_id: ContextId,
        /// Authority ID to unmute
        target_id: String,
    },

    /// Kick a user from a block
    KickUser {
        /// Block ID
        block_id: ContextId,
        /// Authority ID to kick
        target_id: String,
        /// Reason for kick
        reason: String,
    },

    /// Pin a message in a block
    PinMessage {
        /// Block ID
        block_id: ContextId,
        /// Message ID to pin
        message_id: String,
    },

    /// Unpin a message in a block
    UnpinMessage {
        /// Block ID
        block_id: ContextId,
        /// Message ID to unpin
        message_id: String,
    },

    /// Grant steward (admin) privileges to a resident
    GrantSteward {
        /// Block ID
        block_id: ContextId,
        /// Target authority ID to grant steward status
        target_id: String,
    },

    /// Revoke steward (admin) privileges from a resident
    RevokeSteward {
        /// Block ID
        block_id: ContextId,
        /// Target authority ID to revoke steward status
        target_id: String,
    },

    // =========================================================================
    // Navigation Intents
    // =========================================================================
    /// Navigate to a screen
    NavigateTo {
        /// Target screen
        screen: Screen,
    },

    /// Go back to previous screen
    GoBack,

    // =========================================================================
    // Admin/Maintenance Intents
    // =========================================================================
    /// Replace admin for an account
    ReplaceAdmin {
        /// Account ID
        account: String,
        /// New admin authority ID
        new_admin: String,
        /// Activation epoch for the change
        activation_epoch: u64,
    },

    /// Propose a snapshot
    ProposeSnapshot,

    // =========================================================================
    // Authority Intents
    // =========================================================================
    /// Create a new authority
    CreateAuthority {
        /// Threshold required for signing
        threshold: u32,
    },

    /// Show authority details (query)
    ShowAuthority {
        /// Authority ID to show
        authority_id: String,
    },

    /// List all authorities (query)
    ListAuthorities,

    /// Add a device to an authority
    AddDevice {
        /// Target authority ID
        authority_id: String,
        /// Public key of the device to add
        public_key: String,
    },

    /// Remove a device from an authority
    RemoveDevice {
        /// Target authority ID
        authority_id: String,
        /// Device ID to remove
        device_id: String,
    },

    /// Update the signing threshold for an authority
    UpdateThreshold {
        /// New threshold value
        threshold: u32,
    },

    /// Create a new account (simplified account initialization)
    CreateAccount {
        /// Account name/label
        name: String,
    },

    // =========================================================================
    // Context/Inspection Intents (Queries)
    // =========================================================================
    /// Inspect a relational context
    InspectContext {
        /// Context ID to inspect
        context: String,
        /// Path to state file
        state_file: String,
    },

    /// Show receipts for a context
    ShowReceipts {
        /// Context ID
        context: String,
        /// Path to state file
        state_file: String,
        /// Show detailed output
        detailed: bool,
    },

    // =========================================================================
    // AMP Channel Intents
    // =========================================================================
    /// Inspect AMP channel state (query)
    InspectAmpChannel {
        /// Context ID
        context: String,
        /// Channel ID
        channel: String,
    },

    /// Propose a channel epoch bump
    BumpChannelEpoch {
        /// Context ID
        context: String,
        /// Channel ID
        channel: String,
        /// Reason for bump
        reason: String,
    },

    /// Create a channel checkpoint
    CheckpointChannel {
        /// Context ID
        context: String,
        /// Channel ID
        channel: String,
    },

    // =========================================================================
    // OTA Upgrade Intents
    // =========================================================================
    /// Propose an OTA upgrade
    ProposeUpgrade {
        /// Current version (semantic version string)
        from_version: String,
        /// Target version (semantic version string)
        to_version: String,
        /// Upgrade type ("soft" or "hard")
        upgrade_type: String,
        /// Download URL for the upgrade package
        download_url: String,
        /// Description of the upgrade
        description: String,
    },

    /// Set OTA policy
    SetOtaPolicy {
        /// Policy to set
        policy: String,
    },

    /// Get OTA status (query)
    GetOtaStatus,

    /// Opt in to an upgrade proposal
    OptInUpgrade {
        /// Proposal ID to opt into
        proposal_id: String,
    },

    /// List upgrade proposals (query)
    ListUpgradeProposals,

    /// Get upgrade stats (query)
    GetUpgradeStats,

    // =========================================================================
    // Node Operation Intents
    // =========================================================================
    /// Start a node
    StartNode {
        /// Port to listen on
        port: u16,
        /// Run in daemon mode
        daemon: bool,
        /// Path to config file
        config_path: String,
    },

    // =========================================================================
    // Threshold Operation Intents
    // =========================================================================
    /// Run a threshold operation
    RunThreshold {
        /// Comma-separated list of config paths
        configs: String,
        /// Threshold value
        threshold: u32,
        /// Mode: "sign", "verify", "keygen", or "dkd"
        mode: String,
    },

    // =========================================================================
    // Initialization Intents
    // =========================================================================
    /// Initialize a new threshold account
    InitAccount {
        /// Number of devices
        num_devices: u32,
        /// Threshold required for signing
        threshold: u32,
        /// Output directory path
        output: String,
    },

    // =========================================================================
    // System Query Intents
    // =========================================================================
    /// Get account status (query)
    GetStatus {
        /// Path to config file
        config_path: String,
    },

    /// Get version information (query)
    GetVersion,
}

impl Intent {
    /// Get the context this intent belongs to, if applicable
    pub fn context_id(&self) -> Option<ContextId> {
        match self {
            Self::SendMessage { channel_id, .. } => Some(*channel_id),
            Self::MarkAsRead { channel_id, .. } => Some(*channel_id),
            Self::EditMessage { channel_id, .. } => Some(*channel_id),
            Self::DeleteMessage { channel_id, .. } => Some(*channel_id),
            Self::InviteMember { channel_id, .. } => Some(*channel_id),
            Self::JoinChannel { channel_id } => Some(*channel_id),
            Self::LeaveChannel { channel_id } => Some(*channel_id),
            Self::RemoveMember { channel_id, .. } => Some(*channel_id),
            Self::UpdateChannel { channel_id, .. } => Some(*channel_id),
            Self::ApproveRecovery {
                recovery_context, ..
            } => Some(*recovery_context),
            Self::RejectRecovery {
                recovery_context, ..
            } => Some(*recovery_context),
            Self::CompleteRecovery {
                recovery_context, ..
            } => Some(*recovery_context),
            Self::SetBlockName { block_id, .. } => Some(*block_id),
            Self::UpdateBlockStorage { block_id, .. } => Some(*block_id),
            Self::InviteToBlock { block_id, .. } => Some(*block_id),
            Self::SetBlockTopic { block_id, .. } => Some(*block_id),
            Self::BanUser { block_id, .. } => Some(*block_id),
            Self::UnbanUser { block_id, .. } => Some(*block_id),
            Self::MuteUser { block_id, .. } => Some(*block_id),
            Self::UnmuteUser { block_id, .. } => Some(*block_id),
            Self::KickUser { block_id, .. } => Some(*block_id),
            Self::PinMessage { block_id, .. } => Some(*block_id),
            Self::UnpinMessage { block_id, .. } => Some(*block_id),
            Self::GrantSteward { block_id, .. } => Some(*block_id),
            Self::RevokeSteward { block_id, .. } => Some(*block_id),
            _ => None,
        }
    }

    /// Get a human-readable description of this intent
    pub fn description(&self) -> &'static str {
        match self {
            Self::SendMessage { .. } => "send message",
            Self::CreateChannel { .. } => "create channel",
            Self::MarkAsRead { .. } => "mark as read",
            Self::EditMessage { .. } => "edit message",
            Self::DeleteMessage { .. } => "delete message",
            Self::InviteMember { .. } => "invite member",
            Self::JoinChannel { .. } => "join channel",
            Self::LeaveChannel { .. } => "leave channel",
            Self::RemoveMember { .. } => "remove member",
            Self::UpdateChannel { .. } => "update channel",
            Self::InitiateRecovery => "initiate recovery",
            Self::ApproveRecovery { .. } => "approve recovery",
            Self::RejectRecovery { .. } => "reject recovery",
            Self::CompleteRecovery { .. } => "complete recovery",
            Self::SetGuardianThreshold { .. } => "set guardian threshold",
            Self::CreateInvitation { .. } => "create invitation",
            Self::AcceptInvitation { .. } => "accept invitation",
            Self::RejectInvitation { .. } => "reject invitation",
            Self::RevokeInvitation { .. } => "revoke invitation",
            Self::SetNickname { .. } => "set nickname",
            Self::RemoveContact { .. } => "remove contact",
            Self::ToggleGuardian { .. } => "toggle guardian",
            Self::CreateBlock { .. } => "create block",
            Self::InviteToBlock { .. } => "invite to block",
            Self::SetBlockName { .. } => "set block name",
            Self::UpdateBlockStorage { .. } => "update block storage",
            Self::SetBlockTopic { .. } => "set block topic",
            Self::BanUser { .. } => "ban user",
            Self::UnbanUser { .. } => "unban user",
            Self::MuteUser { .. } => "mute user",
            Self::UnmuteUser { .. } => "unmute user",
            Self::KickUser { .. } => "kick user",
            Self::PinMessage { .. } => "pin message",
            Self::UnpinMessage { .. } => "unpin message",
            Self::GrantSteward { .. } => "grant steward",
            Self::RevokeSteward { .. } => "revoke steward",
            Self::NavigateTo { .. } => "navigate",
            Self::GoBack => "go back",
            // Admin/Maintenance
            Self::ReplaceAdmin { .. } => "replace admin",
            Self::ProposeSnapshot => "propose snapshot",
            // Authority
            Self::CreateAuthority { .. } => "create authority",
            Self::ShowAuthority { .. } => "show authority",
            Self::ListAuthorities => "list authorities",
            Self::AddDevice { .. } => "add device",
            Self::RemoveDevice { .. } => "remove device",
            Self::UpdateThreshold { .. } => "update threshold",
            Self::CreateAccount { .. } => "create account",
            // Context/Inspection
            Self::InspectContext { .. } => "inspect context",
            Self::ShowReceipts { .. } => "show receipts",
            // AMP
            Self::InspectAmpChannel { .. } => "inspect AMP channel",
            Self::BumpChannelEpoch { .. } => "bump channel epoch",
            Self::CheckpointChannel { .. } => "checkpoint channel",
            // OTA
            Self::ProposeUpgrade { .. } => "propose upgrade",
            Self::SetOtaPolicy { .. } => "set OTA policy",
            Self::GetOtaStatus => "get OTA status",
            Self::OptInUpgrade { .. } => "opt in to upgrade",
            Self::ListUpgradeProposals => "list upgrade proposals",
            Self::GetUpgradeStats => "get upgrade stats",
            // Node
            Self::StartNode { .. } => "start node",
            // Threshold
            Self::RunThreshold { .. } => "run threshold operation",
            // Init
            Self::InitAccount { .. } => "initialize account",
            // System queries
            Self::GetStatus { .. } => "get status",
            Self::GetVersion => "get version",
        }
    }

    /// Convert this intent to a JournalFact for recording
    ///
    /// Takes the source authority and timestamp and produces a fact
    /// that can be added to the journal.
    pub fn to_journal_fact(
        &self,
        source_authority: AuthorityId,
        timestamp: TimeStamp,
    ) -> JournalFact {
        // Serialize the intent to a content string
        let content = self.to_fact_content();

        JournalFact {
            content,
            timestamp,
            source_authority,
        }
    }

    /// Convert this intent to a fact content string
    ///
    /// Uses the format: `FactType::key1=value1&key2=value2`
    /// This format is parsed by the reducer in `crate::core::reducer::reduce_fact`.
    fn to_fact_content(&self) -> String {
        match self {
            // Chat intents
            Self::SendMessage {
                channel_id,
                content,
                reply_to,
            } => {
                let reply = reply_to.as_deref().unwrap_or("None");
                format!(
                    "SendMessage::channel_id={}&content={}&reply_to={}",
                    channel_id, content, reply
                )
            }
            Self::CreateChannel { name, channel_type } => {
                let ct = match channel_type {
                    ChannelType::Block => "Block",
                    ChannelType::DirectMessage => "DirectMessage",
                    ChannelType::Guardian => "Guardian",
                };
                format!("CreateChannel::name={}&channel_type={}", name, ct)
            }
            Self::MarkAsRead {
                channel_id,
                up_to_message,
            } => {
                format!(
                    "MarkAsRead::channel_id={}&up_to_message={}",
                    channel_id, up_to_message
                )
            }
            Self::EditMessage {
                channel_id,
                message_id,
                content,
            } => {
                format!(
                    "EditMessage::channel_id={}&message_id={}&content={}",
                    channel_id, message_id, content
                )
            }
            Self::DeleteMessage {
                channel_id,
                message_id,
            } => {
                format!(
                    "DeleteMessage::channel_id={}&message_id={}",
                    channel_id, message_id
                )
            }
            Self::InviteMember {
                channel_id,
                member_id,
            } => {
                format!(
                    "InviteMember::channel_id={}&member_id={}",
                    channel_id, member_id
                )
            }
            Self::JoinChannel { channel_id } => {
                format!("JoinChannel::channel_id={}", channel_id)
            }
            Self::LeaveChannel { channel_id } => {
                format!("LeaveChannel::channel_id={}", channel_id)
            }
            Self::RemoveMember {
                channel_id,
                member_id,
            } => {
                format!(
                    "RemoveMember::channel_id={}&member_id={}",
                    channel_id, member_id
                )
            }
            Self::UpdateChannel {
                channel_id,
                name,
                description,
            } => {
                format!(
                    "UpdateChannel::channel_id={}&name={}&description={}",
                    channel_id,
                    name.as_deref().unwrap_or(""),
                    description.as_deref().unwrap_or("")
                )
            }

            // Recovery intents
            Self::InitiateRecovery => {
                // Generate a session ID for tracking
                "InitiateRecovery::session_id=recovery_session".to_string()
            }
            Self::ApproveRecovery { recovery_context } => {
                format!(
                    "ApproveRecovery::guardian_id={}&recovery_context={}",
                    "self", recovery_context
                )
            }
            Self::RejectRecovery {
                recovery_context,
                reason,
            } => {
                format!(
                    "RejectRecovery::recovery_context={}&reason={}",
                    recovery_context, reason
                )
            }
            Self::CompleteRecovery {
                recovery_context,
                recovered_authority_id,
            } => {
                let auth_str = recovered_authority_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "None".to_string());
                format!(
                    "CompleteRecovery::recovery_context={}&recovered_authority_id={}",
                    recovery_context, auth_str
                )
            }
            Self::SetGuardianThreshold { threshold } => {
                format!("SetGuardianThreshold::threshold={}", threshold)
            }

            // Invitation intents
            // Note: Invitation ID is generated deterministically from type for reproducibility.
            // Actual unique IDs are assigned when the fact is created via content hash.
            Self::CreateInvitation { invitation_type } => {
                let it = match invitation_type {
                    InvitationType::Block => "Block",
                    InvitationType::Guardian => "Guardian",
                    InvitationType::Chat => "Chat",
                };
                format!("CreateInvitation::invitation_type={}", it)
            }
            Self::AcceptInvitation { invitation_fact } => {
                format!("AcceptInvitation::invitation_id={}", invitation_fact)
            }
            Self::RejectInvitation { invitation_fact } => {
                format!("RejectInvitation::invitation_id={}", invitation_fact)
            }
            Self::RevokeInvitation { invitation_fact } => {
                format!("RevokeInvitation::invitation_id={}", invitation_fact)
            }

            // Contact intents
            Self::SetNickname {
                contact_id,
                nickname,
            } => {
                format!("SetNickname::target={}&nickname={}", contact_id, nickname)
            }
            Self::RemoveContact { contact_id } => {
                format!("RemoveContact::contact_id={}", contact_id)
            }
            Self::ToggleGuardian {
                contact_id,
                is_guardian,
            } => {
                format!(
                    "ToggleGuardian::contact_id={}&is_guardian={}",
                    contact_id, is_guardian
                )
            }

            // Block intents
            Self::CreateBlock { name } => {
                format!("CreateBlock::name={}", name)
            }
            Self::InviteToBlock {
                block_id,
                invitee_id,
            } => {
                format!(
                    "InviteToBlock::block_id={}&invitee_id={}",
                    block_id, invitee_id
                )
            }
            Self::SetBlockName { block_id, name } => {
                format!("SetBlockName::block_id={}&name={}", block_id, name)
            }
            Self::UpdateBlockStorage {
                block_id,
                storage_budget,
            } => {
                format!(
                    "UpdateBlockStorage::block_id={}&storage_budget={}",
                    block_id, storage_budget
                )
            }
            Self::SetBlockTopic { block_id, topic } => {
                format!("SetBlockTopic::block_id={}&topic={}", block_id, topic)
            }

            // Block moderation intents
            Self::BanUser {
                block_id,
                target_id,
                reason,
            } => {
                format!(
                    "BanUser::block_id={}&target_id={}&reason={}",
                    block_id, target_id, reason
                )
            }
            Self::UnbanUser {
                block_id,
                target_id,
            } => {
                format!("UnbanUser::block_id={}&target_id={}", block_id, target_id)
            }
            Self::MuteUser {
                block_id,
                target_id,
                duration_secs,
            } => {
                let duration = duration_secs
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| "permanent".to_string());
                format!(
                    "MuteUser::block_id={}&target_id={}&duration={}",
                    block_id, target_id, duration
                )
            }
            Self::UnmuteUser {
                block_id,
                target_id,
            } => {
                format!("UnmuteUser::block_id={}&target_id={}", block_id, target_id)
            }
            Self::KickUser {
                block_id,
                target_id,
                reason,
            } => {
                format!(
                    "KickUser::block_id={}&target_id={}&reason={}",
                    block_id, target_id, reason
                )
            }
            Self::PinMessage {
                block_id,
                message_id,
            } => {
                format!(
                    "PinMessage::block_id={}&message_id={}",
                    block_id, message_id
                )
            }
            Self::UnpinMessage {
                block_id,
                message_id,
            } => {
                format!(
                    "UnpinMessage::block_id={}&message_id={}",
                    block_id, message_id
                )
            }
            Self::GrantSteward {
                block_id,
                target_id,
            } => {
                format!(
                    "GrantSteward::block_id={}&target_id={}",
                    block_id, target_id
                )
            }
            Self::RevokeSteward {
                block_id,
                target_id,
            } => {
                format!(
                    "RevokeSteward::block_id={}&target_id={}",
                    block_id, target_id
                )
            }

            // Navigation intents (typically not journaled, but included for completeness)
            Self::NavigateTo { screen } => format!("NavigateTo::screen={:?}", screen),
            Self::GoBack => "GoBack".to_string(),

            // Admin/Maintenance
            Self::ReplaceAdmin {
                account,
                new_admin,
                activation_epoch,
            } => {
                format!(
                    "ReplaceAdmin::account={}&new_admin={}&activation_epoch={}",
                    account, new_admin, activation_epoch
                )
            }
            Self::ProposeSnapshot => "ProposeSnapshot".to_string(),

            // Authority intents
            Self::CreateAuthority { threshold } => {
                format!("CreateAuthority::threshold={}", threshold)
            }
            Self::ShowAuthority { authority_id } => {
                format!("ShowAuthority::authority_id={}", authority_id)
            }
            Self::ListAuthorities => "ListAuthorities".to_string(),
            Self::AddDevice {
                authority_id,
                public_key,
            } => {
                format!(
                    "AddDevice::authority_id={}&public_key={}",
                    authority_id, public_key
                )
            }
            Self::RemoveDevice {
                authority_id,
                device_id,
            } => {
                format!(
                    "RemoveDevice::authority_id={}&device_id={}",
                    authority_id, device_id
                )
            }
            Self::UpdateThreshold { threshold } => {
                format!("UpdateThreshold::threshold={}", threshold)
            }
            Self::CreateAccount { name } => format!("CreateAccount::name={}", name),

            // Context/Inspection
            Self::InspectContext {
                context,
                state_file,
            } => {
                format!(
                    "InspectContext::context={}&state_file={}",
                    context, state_file
                )
            }
            Self::ShowReceipts {
                context,
                state_file,
                detailed,
            } => {
                format!(
                    "ShowReceipts::context={}&state_file={}&detailed={}",
                    context, state_file, detailed
                )
            }

            // AMP
            Self::InspectAmpChannel { context, channel } => {
                format!("InspectAmpChannel::context={}&channel={}", context, channel)
            }
            Self::BumpChannelEpoch {
                context,
                channel,
                reason,
            } => {
                format!(
                    "BumpChannelEpoch::context={}&channel={}&reason={}",
                    context, channel, reason
                )
            }
            Self::CheckpointChannel { context, channel } => {
                format!("CheckpointChannel::context={}&channel={}", context, channel)
            }

            // OTA
            Self::ProposeUpgrade {
                from_version,
                to_version,
                upgrade_type,
                download_url,
                description,
            } => {
                format!(
                    "ProposeUpgrade::from_version={}&to_version={}&upgrade_type={}&download_url={}&description={}",
                    from_version, to_version, upgrade_type, download_url, description
                )
            }
            Self::SetOtaPolicy { policy } => format!("SetOtaPolicy::policy={}", policy),
            Self::GetOtaStatus => "GetOtaStatus".to_string(),
            Self::OptInUpgrade { proposal_id } => {
                format!("OptInUpgrade::proposal_id={}", proposal_id)
            }
            Self::ListUpgradeProposals => "ListUpgradeProposals".to_string(),
            Self::GetUpgradeStats => "GetUpgradeStats".to_string(),

            // Node
            Self::StartNode {
                port,
                daemon,
                config_path,
            } => {
                format!(
                    "StartNode::port={}&daemon={}&config_path={}",
                    port, daemon, config_path
                )
            }

            // Threshold
            Self::RunThreshold {
                configs,
                threshold,
                mode,
            } => {
                format!(
                    "RunThreshold::configs={}&threshold={}&mode={}",
                    configs, threshold, mode
                )
            }

            // Init
            Self::InitAccount {
                num_devices,
                threshold,
                output,
            } => {
                format!(
                    "InitAccount::num_devices={}&threshold={}&output={}",
                    num_devices, threshold, output
                )
            }

            // System queries
            Self::GetStatus { config_path } => {
                format!("GetStatus::config_path={}", config_path)
            }
            Self::GetVersion => "GetVersion".to_string(),
        }
    }

    /// Check if this intent should be journaled
    ///
    /// Some intents (like navigation or pure queries) don't need to be
    /// recorded in the journal as they don't produce state changes.
    pub fn should_journal(&self) -> bool {
        !matches!(
            self,
            Self::NavigateTo { .. }
                | Self::GoBack
                | Self::ShowAuthority { .. }
                | Self::ListAuthorities
                | Self::InspectContext { .. }
                | Self::ShowReceipts { .. }
                | Self::InspectAmpChannel { .. }
                | Self::GetOtaStatus
                | Self::ListUpgradeProposals
                | Self::GetUpgradeStats
                | Self::GetStatus { .. }
                | Self::GetVersion
        )
    }

    /// Get the authorization level required for this intent
    pub fn authorization_level(&self) -> AuthorizationLevel {
        match self {
            // Navigation and queries are public
            Self::NavigateTo { .. }
            | Self::GoBack
            | Self::ShowAuthority { .. }
            | Self::ListAuthorities
            | Self::InspectContext { .. }
            | Self::ShowReceipts { .. }
            | Self::InspectAmpChannel { .. }
            | Self::GetOtaStatus
            | Self::ListUpgradeProposals
            | Self::GetUpgradeStats
            | Self::GetStatus { .. }
            | Self::GetVersion => AuthorizationLevel::Public,

            // Most user operations need basic authorization
            Self::SendMessage { .. }
            | Self::CreateChannel { .. }
            | Self::MarkAsRead { .. }
            | Self::EditMessage { .. }
            | Self::DeleteMessage { .. }
            | Self::JoinChannel { .. }
            | Self::LeaveChannel { .. }
            | Self::UpdateChannel { .. }
            | Self::CreateInvitation { .. }
            | Self::AcceptInvitation { .. }
            | Self::RejectInvitation { .. }
            | Self::RevokeInvitation { .. }
            | Self::SetNickname { .. }
            | Self::RemoveContact { .. }
            | Self::CreateBlock { .. }
            | Self::SetBlockName { .. }
            | Self::SetBlockTopic { .. } => AuthorizationLevel::Basic,

            // Recovery and moderation are sensitive
            Self::InitiateRecovery
            | Self::ApproveRecovery { .. }
            | Self::RejectRecovery { .. }
            | Self::CompleteRecovery { .. }
            | Self::SetGuardianThreshold { .. }
            | Self::ToggleGuardian { .. }
            | Self::BanUser { .. }
            | Self::UnbanUser { .. }
            | Self::MuteUser { .. }
            | Self::UnmuteUser { .. }
            | Self::KickUser { .. }
            | Self::InviteMember { .. }
            | Self::RemoveMember { .. }
            | Self::InviteToBlock { .. }
            | Self::PinMessage { .. }
            | Self::UnpinMessage { .. }
            | Self::UpdateBlockStorage { .. } => AuthorizationLevel::Sensitive,

            // Admin operations require elevated privileges
            Self::ReplaceAdmin { .. }
            | Self::ProposeSnapshot
            | Self::CreateAuthority { .. }
            | Self::AddDevice { .. }
            | Self::RemoveDevice { .. }
            | Self::UpdateThreshold { .. }
            | Self::CreateAccount { .. }
            | Self::BumpChannelEpoch { .. }
            | Self::CheckpointChannel { .. }
            | Self::ProposeUpgrade { .. }
            | Self::SetOtaPolicy { .. }
            | Self::OptInUpgrade { .. }
            | Self::StartNode { .. }
            | Self::RunThreshold { .. }
            | Self::InitAccount { .. }
            | Self::GrantSteward { .. }
            | Self::RevokeSteward { .. } => AuthorizationLevel::Admin,
        }
    }
}

// =============================================================================
// IntentMetadata Implementation
// =============================================================================
// This allows Intent to be used with the IntentEffects trait from aura-core.

impl IntentMetadata for Intent {
    fn description(&self) -> &str {
        Intent::description(self)
    }

    fn should_journal(&self) -> bool {
        Intent::should_journal(self)
    }

    fn authorization_level(&self) -> AuthorizationLevel {
        Intent::authorization_level(self)
    }
}
