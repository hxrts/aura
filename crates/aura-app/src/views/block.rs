//! # Block View State
//!
//! This module contains block state types including moderation functionality
//! (bans, mutes, kicks) that were previously in TUI-only demo code.

use crate::workflows::budget::BlockFlowBudget;
use aura_core::identifiers::{AuthorityId, ChannelId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resident role in the block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ResidentRole {
    /// Regular resident
    #[default]
    Resident,
    /// Block admin
    Admin,
    /// Block owner/creator
    Owner,
}

/// A block resident
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Resident {
    /// Resident identifier (authority ID)
    pub id: AuthorityId,
    /// Display name
    pub name: String,
    /// Role in the block
    pub role: ResidentRole,
    /// Whether resident is online
    pub is_online: bool,
    /// When resident joined (ms since epoch)
    pub joined_at: u64,
    /// Last seen time (ms since epoch)
    pub last_seen: Option<u64>,
    /// Storage allocated by this resident in bytes
    pub storage_allocated: u64,
}

impl Resident {
    /// Check if this resident is a steward (admin or owner)
    pub fn is_steward(&self) -> bool {
        matches!(self.role, ResidentRole::Admin | ResidentRole::Owner)
    }
}

// =============================================================================
// Moderation Types
// =============================================================================

/// Ban record for persistent moderation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BanRecord {
    /// Banned user authority ID
    pub authority_id: AuthorityId,
    /// Reason for ban
    pub reason: String,
    /// Actor who issued the ban
    pub actor: AuthorityId,
    /// Timestamp when ban was issued (ms since epoch)
    pub banned_at: u64,
}

/// Mute record for persistent moderation with expiration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MuteRecord {
    /// Muted user authority ID
    pub authority_id: AuthorityId,
    /// Mute duration in seconds (None = permanent)
    pub duration_secs: Option<u64>,
    /// Timestamp when mute was issued (ms since epoch)
    pub muted_at: u64,
    /// Timestamp when mute expires (ms since epoch, None = permanent)
    pub expires_at: Option<u64>,
    /// Actor who issued the mute
    pub actor: AuthorityId,
}

impl MuteRecord {
    /// Check if this mute has expired
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        match self.expires_at {
            Some(expiry) => current_time_ms >= expiry,
            None => false, // Permanent mute never expires
        }
    }
}

/// Kick log entry for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct KickRecord {
    /// Kicked user authority ID
    pub authority_id: AuthorityId,
    /// Channel from which user was kicked
    pub channel: ChannelId,
    /// Reason for kick
    pub reason: String,
    /// Actor who issued the kick
    pub actor: AuthorityId,
    /// Timestamp when kick occurred (ms since epoch)
    pub kicked_at: u64,
}

// =============================================================================
// Block State
// =============================================================================

/// Block state with full moderation support
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BlockState {
    /// Block identifier
    pub id: ChannelId,
    /// Block name
    pub name: String,
    /// All residents
    pub residents: Vec<Resident>,
    /// Current user's role
    pub my_role: ResidentRole,
    /// Storage budget (uses comprehensive BlockFlowBudget from budget module)
    pub storage: BlockFlowBudget,
    /// Number of online residents
    pub online_count: u32,
    /// Total resident count
    pub resident_count: u32,
    /// Whether this is the user's primary block
    pub is_primary: bool,
    /// Channel topic (optional)
    pub topic: Option<String>,
    /// Pinned messages (message IDs)
    pub pinned_messages: Vec<String>,
    /// Channel mode flags (e.g., "moderated", "invite-only")
    pub mode_flags: Option<String>,
    /// Persistent ban list (keyed by authority ID)
    #[serde(default)]
    pub ban_list: HashMap<AuthorityId, BanRecord>,
    /// Persistent mute list with expiration (keyed by authority ID)
    #[serde(default)]
    pub mute_list: HashMap<AuthorityId, MuteRecord>,
    /// Kick log for audit trail
    #[serde(default)]
    pub kick_log: Vec<KickRecord>,
    /// When the block was created (ms since epoch)
    pub created_at: u64,
    /// Relational context identifier for journal integration
    #[serde(default)]
    pub context_id: String,
}

impl BlockState {
    /// Default storage limit: 10 MB
    pub const DEFAULT_STORAGE_BUDGET: u64 = 10 * 1024 * 1024;
    /// Default resident allocation: 200 KB
    pub const RESIDENT_ALLOCATION: u64 = 200 * 1024;

    /// Create a new block with the creator as steward
    pub fn new(
        id: ChannelId,
        name: Option<String>,
        creator_id: AuthorityId,
        created_at: u64,
        context_id: String,
    ) -> Self {
        let steward = Resident {
            id: creator_id,
            name: "You".to_string(),
            role: ResidentRole::Owner,
            is_online: true,
            joined_at: created_at,
            last_seen: Some(created_at),
            storage_allocated: Self::RESIDENT_ALLOCATION,
        };

        // Initialize budget with one resident (the creator)
        let mut budget = BlockFlowBudget::new(id.to_string());
        let _ = budget.add_resident(); // Creator is first resident

        Self {
            id,
            name: name.unwrap_or_default(),
            residents: vec![steward],
            my_role: ResidentRole::Owner,
            storage: budget,
            online_count: 1,
            resident_count: 1,
            is_primary: false,
            topic: None,
            pinned_messages: Vec::new(),
            mode_flags: None,
            ban_list: HashMap::new(),
            mute_list: HashMap::new(),
            kick_log: Vec::new(),
            created_at,
            context_id,
        }
    }

    /// Get resident by ID
    pub fn resident(&self, id: &AuthorityId) -> Option<&Resident> {
        self.residents.iter().find(|r| r.id == *id)
    }

    /// Get mutable reference to resident by ID
    pub fn resident_mut(&mut self, id: &AuthorityId) -> Option<&mut Resident> {
        self.residents.iter_mut().find(|r| r.id == *id)
    }

    /// Add a resident to the block
    pub fn add_resident(&mut self, resident: Resident) {
        // Charge storage budget for new resident
        let _ = self.storage.add_resident();
        self.resident_count += 1;
        if resident.is_online {
            self.online_count += 1;
        }
        self.residents.push(resident);
    }

    /// Remove a resident from the block
    pub fn remove_resident(&mut self, id: &AuthorityId) -> Option<Resident> {
        if let Some(pos) = self.residents.iter().position(|r| r.id == *id) {
            let resident = self.residents.remove(pos);
            // Free storage budget
            let _ = self.storage.remove_resident();
            self.resident_count = self.resident_count.saturating_sub(1);
            if resident.is_online {
                self.online_count = self.online_count.saturating_sub(1);
            }
            Some(resident)
        } else {
            None
        }
    }

    /// Get online residents
    pub fn online_residents(&self) -> Vec<&Resident> {
        self.residents.iter().filter(|r| r.is_online).collect()
    }

    /// Check if current user is admin or owner
    pub fn is_admin(&self) -> bool {
        matches!(self.my_role, ResidentRole::Admin | ResidentRole::Owner)
    }

    /// Set block name
    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    // =========================================================================
    // Moderation Methods
    // =========================================================================

    /// Check if a user is banned
    pub fn is_banned(&self, authority_id: &AuthorityId) -> bool {
        self.ban_list.contains_key(authority_id)
    }

    /// Check if a user is muted (and mute hasn't expired)
    pub fn is_muted(&self, authority_id: &AuthorityId, current_time_ms: u64) -> bool {
        self.mute_list
            .get(authority_id)
            .is_some_and(|record| !record.is_expired(current_time_ms))
    }

    /// Add a ban record
    pub fn add_ban(&mut self, record: BanRecord) {
        self.ban_list.insert(record.authority_id, record);
    }

    /// Remove a ban
    pub fn remove_ban(&mut self, authority_id: &AuthorityId) -> Option<BanRecord> {
        self.ban_list.remove(authority_id)
    }

    /// Add a mute record
    pub fn add_mute(&mut self, record: MuteRecord) {
        self.mute_list.insert(record.authority_id, record);
    }

    /// Remove a mute
    pub fn remove_mute(&mut self, authority_id: &AuthorityId) -> Option<MuteRecord> {
        self.mute_list.remove(authority_id)
    }

    /// Add a kick record to the audit log
    pub fn add_kick(&mut self, record: KickRecord) {
        self.kick_log.push(record);
    }

    /// Add a pinned message
    pub fn pin_message(&mut self, message_id: String) {
        if !self.pinned_messages.contains(&message_id) {
            self.pinned_messages.push(message_id);
        }
    }

    /// Remove a pinned message
    pub fn unpin_message(&mut self, message_id: &str) -> bool {
        if let Some(pos) = self.pinned_messages.iter().position(|id| id == message_id) {
            self.pinned_messages.remove(pos);
            true
        } else {
            false
        }
    }

    /// Clean up expired mutes
    pub fn cleanup_expired_mutes(&mut self, current_time_ms: u64) {
        self.mute_list
            .retain(|_, record| !record.is_expired(current_time_ms));
    }
}

// =============================================================================
// Multi-Block State
// =============================================================================

/// State for managing multiple blocks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BlocksState {
    /// All blocks the user has created or joined (keyed by block ID)
    #[serde(default)]
    pub blocks: HashMap<ChannelId, BlockState>,
    /// Currently selected block ID
    pub current_block_id: Option<ChannelId>,
}

impl BlocksState {
    /// Create a new empty BlocksState
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current block
    pub fn current_block(&self) -> Option<&BlockState> {
        self.current_block_id
            .as_ref()
            .and_then(|id| self.blocks.get(id))
    }

    /// Get a mutable reference to the current block
    pub fn current_block_mut(&mut self) -> Option<&mut BlockState> {
        if let Some(id) = &self.current_block_id {
            self.blocks.get_mut(id)
        } else {
            None
        }
    }

    /// Get a block by ID
    pub fn block(&self, id: &ChannelId) -> Option<&BlockState> {
        self.blocks.get(id)
    }

    /// Get a mutable reference to a block by ID
    pub fn block_mut(&mut self, id: &ChannelId) -> Option<&mut BlockState> {
        self.blocks.get_mut(id)
    }

    /// Add a block
    pub fn add_block(&mut self, block: BlockState) {
        let is_first = self.blocks.is_empty();
        let id = block.id;
        self.blocks.insert(id, block);
        // Auto-select first block
        if is_first {
            self.current_block_id = Some(id);
        }
    }

    /// Remove a block
    pub fn remove_block(&mut self, id: &ChannelId) -> Option<BlockState> {
        let block = self.blocks.remove(id);
        // Clear selection if current block was removed
        if self.current_block_id.as_ref() == Some(id) {
            self.current_block_id = self.blocks.keys().next().cloned();
        }
        block
    }

    /// Select a block by ID
    pub fn select_block(&mut self, id: Option<ChannelId>) {
        self.current_block_id = id;
    }

    /// Check if a block exists
    pub fn has_block(&self, id: &ChannelId) -> bool {
        self.blocks.contains_key(id)
    }

    /// Get number of blocks
    pub fn count(&self) -> usize {
        self.blocks.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Get all block IDs
    pub fn block_ids(&self) -> Vec<&ChannelId> {
        self.blocks.keys().collect()
    }

    /// Iterate over all blocks
    pub fn iter(&self) -> impl Iterator<Item = (&ChannelId, &BlockState)> {
        self.blocks.iter()
    }

    /// Iterate over all blocks mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ChannelId, &mut BlockState)> {
        self.blocks.iter_mut()
    }
}
