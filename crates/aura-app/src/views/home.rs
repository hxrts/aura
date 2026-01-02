//! # Home View State
//!
//! This module contains home state types including moderation functionality
//! (bans, mutes, kicks) that were previously in TUI-only demo code.

use crate::workflows::budget::HomeFlowBudget;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Resident role in the home
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum ResidentRole {
    /// Regular resident
    #[default]
    Resident,
    /// Home admin
    Admin,
    /// Home owner/creator
    Owner,
}

/// A home resident
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Resident {
    /// Resident identifier (authority ID)
    pub id: AuthorityId,
    /// Display name
    pub name: String,
    /// Role in the home
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

/// Pinned message metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PinnedMessageMeta {
    /// Message identifier
    pub message_id: String,
    /// Authority who pinned the message
    pub pinned_by: AuthorityId,
    /// Timestamp when pin occurred (ms since epoch)
    pub pinned_at: u64,
}

// =============================================================================
// Home State
// =============================================================================

/// Home state with full moderation support
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomeState {
    /// Home identifier
    pub id: ChannelId,
    /// Home name
    pub name: String,
    /// All residents
    pub residents: Vec<Resident>,
    /// Current user's role
    pub my_role: ResidentRole,
    /// Storage budget (uses comprehensive HomeFlowBudget from budget module)
    pub storage: HomeFlowBudget,
    /// Number of online residents
    pub online_count: u32,
    /// Total resident count
    pub resident_count: u32,
    /// Whether this is the user's primary home
    pub is_primary: bool,
    /// Channel topic (optional)
    pub topic: Option<String>,
    /// Pinned messages (message IDs)
    pub pinned_messages: Vec<String>,
    /// Pinned message metadata keyed by message ID
    #[serde(default)]
    pub pinned_metadata: HashMap<String, PinnedMessageMeta>,
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
    /// When the home was created (ms since epoch)
    pub created_at: u64,
    /// Relational context identifier for journal integration
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_id: Option<ContextId>,
}

impl HomeState {
    /// Default storage limit: 10 MB
    pub const DEFAULT_STORAGE_BUDGET: u64 = 10 * 1024 * 1024;
    /// Default resident allocation: 200 KB
    pub const RESIDENT_ALLOCATION: u64 = 200 * 1024;
    /// Maximum number of kick records retained in memory.
    const MAX_KICK_LOG: usize = 200;

    /// Create a new home with the creator as steward
    pub fn new(
        id: ChannelId,
        name: Option<String>,
        creator_id: AuthorityId,
        created_at: u64,
        context_id: ContextId,
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
        let mut budget = HomeFlowBudget::new(id.to_string());
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
            pinned_metadata: HashMap::new(),
            mode_flags: None,
            ban_list: HashMap::new(),
            mute_list: HashMap::new(),
            kick_log: Vec::new(),
            created_at,
            context_id: Some(context_id),
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

    /// Add a resident to the home
    pub fn add_resident(&mut self, resident: Resident) {
        // Charge storage budget for new resident
        let _ = self.storage.add_resident();
        self.resident_count += 1;
        if resident.is_online {
            self.online_count += 1;
        }
        self.residents.push(resident);
    }

    /// Remove a resident from the home
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

    /// Set home name
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
        if self.kick_log.len() > Self::MAX_KICK_LOG {
            let overflow = self.kick_log.len() - Self::MAX_KICK_LOG;
            self.kick_log.drain(0..overflow);
        }
    }

    /// Add a pinned message with metadata
    pub fn pin_message_with_meta(&mut self, meta: PinnedMessageMeta) {
        if !self.pinned_messages.contains(&meta.message_id) {
            self.pinned_messages.push(meta.message_id.clone());
        }
        self.pinned_metadata.insert(meta.message_id.clone(), meta);
    }

    /// Add a pinned message (metadata optional)
    pub fn pin_message(&mut self, message_id: String) {
        if !self.pinned_messages.contains(&message_id) {
            self.pinned_messages.push(message_id);
        }
    }

    /// Remove a pinned message
    pub fn unpin_message(&mut self, message_id: &str) -> bool {
        let had_entry =
            if let Some(pos) = self.pinned_messages.iter().position(|id| id == message_id) {
                self.pinned_messages.remove(pos);
                true
            } else {
                false
            };
        self.pinned_metadata.remove(message_id);
        had_entry
    }

    /// Clean up expired mutes
    pub fn cleanup_expired_mutes(&mut self, current_time_ms: u64) {
        self.mute_list
            .retain(|_, record| !record.is_expired(current_time_ms));
    }
}

// =============================================================================
// Multi-Home State
// =============================================================================

/// State for managing multiple homes
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomesState {
    /// All homes the user has created or joined (keyed by home ID)
    #[serde(default)]
    pub homes: HashMap<ChannelId, HomeState>,
    /// Currently selected home ID
    pub current_home_id: Option<ChannelId>,
}

impl HomesState {
    /// Create a new empty HomesState
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current home
    pub fn current_home(&self) -> Option<&HomeState> {
        self.current_home_id
            .as_ref()
            .and_then(|id| self.homes.get(id))
    }

    /// Get a mutable reference to the current home
    pub fn current_home_mut(&mut self) -> Option<&mut HomeState> {
        if let Some(id) = &self.current_home_id {
            self.homes.get_mut(id)
        } else {
            None
        }
    }

    /// Get a home by ID
    pub fn home_state(&self, id: &ChannelId) -> Option<&HomeState> {
        self.homes.get(id)
    }

    /// Get a mutable reference to a home by ID
    pub fn home_mut(&mut self, id: &ChannelId) -> Option<&mut HomeState> {
        self.homes.get_mut(id)
    }

    /// Add a home
    pub fn add_home(&mut self, home_state: HomeState) {
        let is_first = self.homes.is_empty();
        let id = home_state.id;
        self.homes.insert(id, home_state);
        // Auto-select first home
        if is_first {
            self.current_home_id = Some(id);
        }
    }

    /// Remove a home
    pub fn remove_home(&mut self, id: &ChannelId) -> Option<HomeState> {
        let home = self.homes.remove(id);
        // Clear selection if current home was removed
        if self.current_home_id.as_ref() == Some(id) {
            self.current_home_id = self.homes.keys().next().cloned();
        }
        home
    }

    /// Select a home by ID
    pub fn select_home(&mut self, id: Option<ChannelId>) {
        self.current_home_id = id;
    }

    /// Check if a home exists
    pub fn has_home(&self, id: &ChannelId) -> bool {
        self.homes.contains_key(id)
    }

    /// Get number of homes
    pub fn count(&self) -> usize {
        self.homes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.homes.is_empty()
    }

    /// Get all home IDs
    pub fn home_ids(&self) -> Vec<&ChannelId> {
        self.homes.keys().collect()
    }

    /// Iterate over all homes
    pub fn iter(&self) -> impl Iterator<Item = (&ChannelId, &HomeState)> {
        self.homes.iter()
    }

    /// Iterate over all homes mutably
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ChannelId, &mut HomeState)> {
        self.homes.iter_mut()
    }
}
