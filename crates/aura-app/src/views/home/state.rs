#![allow(missing_docs)]

use super::members::{HomeMember, HomeRole};
use super::moderation::{BanRecord, KickRecord, MuteRecord, PinnedMessageMeta};
use super::serde_support::{authority_id_keyed_map, channel_id_keyed_map};
use crate::workflows::budget::HomeFlowBudget;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, HomeId};
use aura_social::SocialFact;
use aura_social::{AccessLevel, AccessLevelCapabilityConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Home state with full moderation support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomeState {
    pub id: ChannelId,
    pub name: String,
    pub members: Vec<HomeMember>,
    pub my_role: HomeRole,
    pub storage: HomeFlowBudget,
    pub online_count: u32,
    pub member_count: u32,
    pub is_primary: bool,
    pub topic: Option<String>,
    pub pinned_messages: Vec<String>,
    #[serde(default)]
    pub pinned_metadata: HashMap<String, PinnedMessageMeta>,
    pub mode_flags: Option<String>,
    #[serde(with = "authority_id_keyed_map", default)]
    pub access_overrides: HashMap<AuthorityId, AccessLevel>,
    #[serde(default)]
    pub access_level_capabilities: Option<AccessLevelCapabilityConfig>,
    #[serde(with = "authority_id_keyed_map", default)]
    pub ban_list: HashMap<AuthorityId, BanRecord>,
    #[serde(with = "authority_id_keyed_map", default)]
    pub mute_list: HashMap<AuthorityId, MuteRecord>,
    #[serde(default)]
    pub kick_log: Vec<KickRecord>,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub context_id: Option<ContextId>,
    /// Neighborhoods this home has joined (id to name), materialized from
    /// `SocialFact::NeighborhoodCreated` / `HomeJoinedNeighborhood` facts.
    #[serde(default)]
    pub neighborhoods: std::collections::BTreeMap<String, String>,
}

impl HomeState {
    pub const DEFAULT_STORAGE_BUDGET: u64 = 10 * 1024 * 1024;
    pub const MEMBER_ALLOCATION: u64 = 200 * 1024;
    pub(crate) const MAX_KICK_LOG: usize = 200;

    /// Record that this home joined `neighborhood_id`, charging the home's
    /// neighborhood budget (at most `MAX_NEIGHBORHOODS`, docs/115). Joining a
    /// neighborhood the home is already in changes nothing and returns
    /// `Ok(false)`, so replayed facts do not double-charge.
    pub fn join_neighborhood(
        &mut self,
        neighborhood_id: &str,
        name: &str,
    ) -> Result<bool, crate::workflows::budget::BudgetError> {
        if self.neighborhoods.contains_key(neighborhood_id) {
            return Ok(false);
        }
        self.storage.join_neighborhood()?;
        self.neighborhoods
            .insert(neighborhood_id.to_string(), name.to_string());
        Ok(true)
    }

    pub fn new(
        id: ChannelId,
        name: Option<String>,
        creator_id: AuthorityId,
        created_at: u64,
        context_id: ContextId,
    ) -> Self {
        let moderator = HomeMember {
            id: creator_id,
            name: "You".to_string(),
            role: HomeRole::Member,
            is_online: true,
            joined_at: created_at,
            last_seen: Some(created_at),
            storage_allocated: Self::MEMBER_ALLOCATION,
        };

        let home_id = HomeId::from_bytes(*id.as_bytes());
        let mut budget = HomeFlowBudget::new(home_id);
        let _ = budget.add_member();

        Self {
            id,
            name: name.unwrap_or_default(),
            members: vec![moderator],
            my_role: HomeRole::Member,
            storage: budget,
            online_count: 1,
            member_count: 1,
            is_primary: false,
            topic: None,
            pinned_messages: Vec::new(),
            pinned_metadata: HashMap::new(),
            mode_flags: None,
            access_overrides: HashMap::new(),
            access_level_capabilities: None,
            ban_list: HashMap::new(),
            mute_list: HashMap::new(),
            kick_log: Vec::new(),
            created_at,
            context_id: Some(context_id),
            neighborhoods: std::collections::BTreeMap::new(),
        }
    }

    pub fn member(&self, id: &AuthorityId) -> Option<&HomeMember> {
        self.members.iter().find(|r| r.id == *id)
    }

    pub fn member_mut(&mut self, id: &AuthorityId) -> Option<&mut HomeMember> {
        self.members.iter_mut().find(|r| r.id == *id)
    }

    pub fn add_member(&mut self, member: HomeMember) {
        if let Err(_e) = self.storage.add_member() {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                home_id = %self.id,
                error = %_e,
                "home budget capacity exceeded — member added but budget projection diverges"
            );
        }
        self.member_count += 1;
        if member.is_online {
            self.online_count += 1;
        }
        self.members.push(member);
    }

    pub fn remove_member(&mut self, id: &AuthorityId) -> Option<HomeMember> {
        if let Some(pos) = self.members.iter().position(|r| r.id == *id) {
            let member = self.members.remove(pos);
            let _ = self.storage.remove_member();
            self.member_count = self.member_count.saturating_sub(1);
            if member.is_online {
                self.online_count = self.online_count.saturating_sub(1);
            }
            Some(member)
        } else {
            None
        }
    }

    pub fn online_members(&self) -> Vec<&HomeMember> {
        self.members.iter().filter(|r| r.is_online).collect()
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.my_role, HomeRole::Moderator | HomeRole::Member)
    }

    pub fn is_moderator(&self) -> bool {
        matches!(self.my_role, HomeRole::Moderator)
    }

    pub fn can_moderate(&self) -> bool {
        self.is_moderator()
    }

    pub fn set_name(&mut self, name: String) {
        self.name = name;
    }

    pub fn access_override(&self, authority_id: &AuthorityId) -> Option<AccessLevel> {
        self.access_overrides.get(authority_id).copied()
    }

    /// Record home creation as the creator's initial moderator designation
    /// (docs/115 §3.3: the creator is the sole member, so creation is that
    /// governance decision) and set the viewer's own role accordingly.
    pub fn designate_creator_moderator(&mut self, creator: &AuthorityId, viewer: &AuthorityId) {
        if let Some(member) = self.member_mut(creator) {
            member.role = HomeRole::Moderator;
        }
        self.my_role = if creator == viewer {
            HomeRole::Moderator
        } else {
            self.member(viewer)
                .map(|member| member.role)
                .unwrap_or(HomeRole::Participant)
        };
    }

    /// Effective access level of an authority in this home, when it can be
    /// determined from home-local state: an explicit override wins, otherwise
    /// a member of the home has Full (same-home) access. Returns `None` for a
    /// non-member without an override, whose hop-based level is not local.
    pub fn effective_access_level(&self, authority_id: &AuthorityId) -> Option<AccessLevel> {
        self.access_override(authority_id)
            .or_else(|| self.member(authority_id).map(|_| AccessLevel::Full))
    }

    /// Whether the home's capability config grants `capability` to the
    /// authority's effective access level. Undetermined levels are not
    /// restricted here.
    pub fn allows_access_capability(&self, authority_id: &AuthorityId, capability: &str) -> bool {
        let Some(level) = self.effective_access_level(authority_id) else {
            return true;
        };
        match &self.access_level_capabilities {
            Some(config) => config.allows(level, capability),
            None => AccessLevelCapabilityConfig::default().allows(level, capability),
        }
    }

    /// Receiver-side check for a moderation or pin fact's actor: the actor
    /// must be a moderator in the known roster and its access level must
    /// grant `capability`. An empty roster is treated as unknown and not
    /// restricted here.
    pub fn actor_may_moderate(&self, actor: &AuthorityId, capability: &str) -> bool {
        if self.members.is_empty() {
            return true;
        }
        match self.member(actor) {
            Some(member) => {
                matches!(member.role, HomeRole::Moderator)
                    && self.allows_access_capability(actor, capability)
            }
            None => false,
        }
    }

    /// Receiver-side check for a moderator designation or revocation: the
    /// actor must be a threshold member (member or moderator) in the known
    /// roster with the `grant_moderator` access capability. An empty roster
    /// is treated as unknown.
    pub fn actor_may_designate_moderators(&self, actor: &AuthorityId) -> bool {
        if self.members.is_empty() {
            return true;
        }
        self.member(actor).is_some_and(|member| {
            member.role.is_threshold_member()
                && self.allows_access_capability(actor, "grant_moderator")
        })
    }

    pub fn set_access_override(&mut self, authority_id: AuthorityId, access_level: AccessLevel) {
        self.access_overrides.insert(authority_id, access_level);
    }

    pub fn set_access_level_capabilities(&mut self, config: AccessLevelCapabilityConfig) {
        self.access_level_capabilities = Some(config);
    }

    pub fn is_banned(&self, authority_id: &AuthorityId) -> bool {
        self.ban_list.contains_key(authority_id)
    }

    pub fn is_muted(&self, authority_id: &AuthorityId, current_time_ms: u64) -> bool {
        self.mute_list
            .get(authority_id)
            .is_some_and(|record| !record.is_expired(current_time_ms))
    }

    pub fn add_ban(&mut self, record: BanRecord) {
        self.ban_list.insert(record.authority_id, record);
    }

    pub fn remove_ban(&mut self, authority_id: &AuthorityId) -> Option<BanRecord> {
        self.ban_list.remove(authority_id)
    }

    pub fn add_mute(&mut self, record: MuteRecord) {
        self.mute_list.insert(record.authority_id, record);
    }

    pub fn remove_mute(&mut self, authority_id: &AuthorityId) -> Option<MuteRecord> {
        self.mute_list.remove(authority_id)
    }

    pub fn add_kick(&mut self, record: KickRecord) {
        self.kick_log.push(record);
        if self.kick_log.len() > Self::MAX_KICK_LOG {
            let overflow = self.kick_log.len() - Self::MAX_KICK_LOG;
            self.kick_log.drain(0..overflow);
        }
    }

    pub fn pin_message_with_meta(&mut self, meta: PinnedMessageMeta) {
        if !self.pinned_messages.contains(&meta.message_id) {
            self.pinned_messages.push(meta.message_id.clone());
        }
        self.pinned_metadata.insert(meta.message_id.clone(), meta);
    }

    pub fn pin_message(&mut self, message_id: String) {
        if !self.pinned_messages.contains(&message_id) {
            self.pinned_messages.push(message_id);
        }
    }

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
}

/// Result returned when adding a home.
#[derive(Debug, Clone)]
pub struct AddHomeResult {
    pub home_id: ChannelId,
    pub was_first: bool,
}

/// Creation fields extracted from a canonical `HomeCreated` fact. Membership
/// and moderation facts cannot construct this witness.
pub struct HomeCreationWitness {
    id: ChannelId,
    context_id: ContextId,
    creator_id: AuthorityId,
    created_at: u64,
    name: String,
}

impl HomeCreationWitness {
    /// The created home's id.
    #[must_use]
    pub fn id(&self) -> ChannelId {
        self.id
    }

    /// The context of the created home.
    #[must_use]
    pub fn context_id(&self) -> ContextId {
        self.context_id
    }

    /// The authority that created the home.
    #[must_use]
    pub fn creator_id(&self) -> AuthorityId {
        self.creator_id
    }

    pub(crate) fn from_created_fact(fact: &SocialFact) -> Option<Self> {
        let SocialFact::HomeCreated {
            home_id,
            context_id,
            creator_id,
            created_at,
            name,
            ..
        } = fact
        else {
            return None;
        };
        Some(Self {
            id: ChannelId::from_bytes(*home_id.as_bytes()),
            context_id: *context_id,
            creator_id: *creator_id,
            created_at: created_at.ts_ms,
            name: name.clone(),
        })
    }
}

/// Result returned when removing a home.
#[derive(Debug, Clone)]
pub struct RemoveHomeResult {
    pub removed: Option<HomeState>,
    pub was_selected: bool,
}

/// State for managing multiple homes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomesState {
    #[serde(with = "channel_id_keyed_map", default)]
    homes: HashMap<ChannelId, HomeState>,
    current_home_id: Option<ChannelId>,
}

impl HomesState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Hydrate a trusted query result. Canonical insertion is owned by the
    /// home workflow or runtime fact materializer, not external observers.
    pub(crate) fn from_parts(
        homes: HashMap<ChannelId, HomeState>,
        current_home_id: Option<ChannelId>,
    ) -> Self {
        Self {
            homes,
            current_home_id,
        }
    }

    pub fn current_home_id(&self) -> Option<&ChannelId> {
        self.current_home_id.as_ref()
    }

    pub fn current_home(&self) -> Option<&HomeState> {
        self.current_home_id
            .as_ref()
            .and_then(|id| self.homes.get(id))
    }

    pub fn current_home_mut(&mut self) -> Option<&mut HomeState> {
        if let Some(id) = &self.current_home_id {
            self.homes.get_mut(id)
        } else {
            None
        }
    }

    pub fn home_state(&self, id: &ChannelId) -> Option<&HomeState> {
        self.homes.get(id)
    }

    pub fn home_mut(&mut self, id: &ChannelId) -> Option<&mut HomeState> {
        self.homes.get_mut(id)
    }

    pub fn has_home(&self, id: &ChannelId) -> bool {
        self.homes.contains_key(id)
    }

    pub fn count(&self) -> usize {
        self.homes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.homes.is_empty()
    }

    pub fn home_ids(&self) -> Vec<&ChannelId> {
        self.homes.keys().collect()
    }

    pub fn all_homes(&self) -> impl Iterator<Item = &HomeState> {
        self.homes.values()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ChannelId, &HomeState)> {
        self.homes.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&ChannelId, &mut HomeState)> {
        self.homes.iter_mut()
    }

    pub fn all_homes_mut(&mut self) -> impl Iterator<Item = &mut HomeState> {
        self.homes.values_mut()
    }

    /// Get the first available home ID without changing current selection.
    pub fn first_home_id(&self) -> Option<ChannelId> {
        self.homes.keys().next().cloned()
    }

    /// Add a home without implicitly changing current selection.
    pub(crate) fn add_home(&mut self, home_state: HomeState) -> AddHomeResult {
        let was_first = self.homes.is_empty();
        let home_id = home_state.id;
        self.homes.insert(home_id, home_state);
        AddHomeResult { home_id, was_first }
    }

    /// Materialize a canonical home from its creation fact. Subsequent facts
    /// may enrich this entry but cannot create another entry by raw ID.
    pub fn materialize_created_home(
        &mut self,
        witness: HomeCreationWitness,
        own_authority: AuthorityId,
    ) -> AddHomeResult {
        if let Some(existing) = self.homes.get_mut(&witness.id) {
            // A home first seen through an invitation already exists; its
            // creation fact still designates the creator moderator.
            if existing.member(&witness.creator_id).is_some() {
                existing.designate_creator_moderator(&witness.creator_id, &own_authority);
            }
            return AddHomeResult {
                home_id: witness.id,
                was_first: false,
            };
        }
        let mut home = HomeState::new(
            witness.id,
            Some(witness.name),
            witness.creator_id,
            witness.created_at,
            witness.context_id,
        );
        if own_authority != witness.creator_id {
            if let Some(creator) = home.member_mut(&witness.creator_id) {
                creator.name = witness.creator_id.to_string();
                creator.is_online = false;
            }
            home.online_count = 0;
            home.my_role = HomeRole::Participant;
        }
        home.designate_creator_moderator(&witness.creator_id, &own_authority);
        self.add_home(home)
    }

    /// Remove a home and clear selection if the removed home was selected.
    pub fn remove_home(&mut self, id: &ChannelId) -> RemoveHomeResult {
        let was_selected = self.current_home_id.as_ref() == Some(id);
        let removed = self.homes.remove(id);
        if was_selected {
            self.current_home_id = None;
        }
        RemoveHomeResult {
            removed,
            was_selected,
        }
    }

    /// Select a home explicitly. `None` means no current selection.
    pub fn select_home(&mut self, id: Option<ChannelId>) {
        self.current_home_id = id;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_with(owner: AuthorityId) -> HomeState {
        HomeState::new(
            ChannelId::from_bytes([7u8; 32]),
            Some("home".to_string()),
            owner,
            0,
            ContextId::new_from_entropy([8u8; 32]),
        )
    }

    #[test]
    fn limited_override_removes_capability_granted_by_default_level() {
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let mut home = home_with(owner);
        assert_eq!(home.effective_access_level(&owner), Some(AccessLevel::Full));
        assert!(home.allows_access_capability(&owner, "send_message"));

        home.set_access_override(owner, AccessLevel::Limited);
        assert!(!home.allows_access_capability(&owner, "send_message"));
        assert!(home.allows_access_capability(&owner, "send_dm"));
    }

    #[test]
    fn capability_config_change_takes_effect() {
        let owner = AuthorityId::new_from_entropy([2u8; 32]);
        let mut home = home_with(owner);
        home.set_access_override(owner, AccessLevel::Partial);
        assert!(!home.allows_access_capability(&owner, "pin_content"));

        let mut config = AccessLevelCapabilityConfig::default();
        config.partial.insert("pin_content".to_string());
        home.set_access_level_capabilities(config);
        assert!(home.allows_access_capability(&owner, "pin_content"));
    }

    #[test]
    fn undetermined_level_is_not_restricted_locally() {
        let home = home_with(AuthorityId::new_from_entropy([3u8; 32]));
        let stranger = AuthorityId::new_from_entropy([4u8; 32]);
        assert_eq!(home.effective_access_level(&stranger), None);
        assert!(home.allows_access_capability(&stranger, "send_message"));
    }
}
