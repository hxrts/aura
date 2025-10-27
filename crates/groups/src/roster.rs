// Deterministic roster management from capability graph

use crate::events::RosterDelta;
use crate::types::*;
use crate::{AuraError, Result};
use aura_journal::capability::{authority_graph::AuthorityGraph, types::CapabilityScope};
use std::collections::BTreeSet;
use tracing::{debug, trace};

/// Deterministic roster builder from capability graph
pub struct RosterBuilder {
    /// Required capability scope for group membership
    membership_scope: CapabilityScope,
    /// Last processed epoch for incremental updates
    last_epoch: Option<Epoch>,
}

impl RosterBuilder {
    /// Create new roster builder for a specific capability scope
    pub fn new(group_namespace: &str) -> Self {
        let membership_scope = CapabilityScope::simple(group_namespace, "member");

        Self {
            membership_scope,
            last_epoch: None,
        }
    }

    /// Create roster builder for MLS group membership
    pub fn for_mls_group(group_id: &str) -> Self {
        let membership_scope = CapabilityScope::with_resource("mls", "member", group_id);

        Self {
            membership_scope,
            last_epoch: None,
        }
    }

    /// Build complete roster from capability graph
    pub fn build_roster(
        &mut self,
        authority_graph: &AuthorityGraph,
        epoch: Epoch,
        effects: &aura_crypto::Effects,
    ) -> Result<Roster> {
        debug!(
            "Building roster for epoch {} with scope {:?}",
            epoch.value(),
            self.membership_scope
        );

        // Extract all subjects with the required capability
        let authorized_subjects =
            authority_graph.get_subjects_with_scope(&self.membership_scope, effects);

        // Convert subjects to member IDs
        let member_ids: BTreeSet<MemberId> = authorized_subjects
            .into_iter()
            .map(|subject| MemberId::new(&subject.0))
            .collect();

        // Build roster with deterministic ordering
        let mut roster = Roster::new(epoch);

        // Add members in sorted order for deterministic positioning
        for member_id in member_ids {
            roster.add_member(member_id);
        }

        debug!(
            "Built roster with {} members for epoch {}",
            roster.member_count(),
            epoch.value()
        );
        self.last_epoch = Some(epoch);

        Ok(roster)
    }

    /// Compute roster delta between two epochs
    pub fn compute_delta(&self, old_roster: &Roster, new_roster: &Roster) -> RosterDelta {
        let old_members: BTreeSet<_> = old_roster.members.keys().cloned().collect();
        let new_members: BTreeSet<_> = new_roster.members.keys().cloned().collect();

        let added_members: Vec<_> = new_members.difference(&old_members).cloned().collect();
        let removed_members: Vec<_> = old_members.difference(&new_members).cloned().collect();

        debug!(
            "Roster delta: +{} members, -{} members",
            added_members.len(),
            removed_members.len()
        );

        RosterDelta {
            added_members,
            removed_members,
            previous_size: old_roster.size,
            new_size: new_roster.size,
        }
    }

    /// Check if roster needs update based on capability changes
    pub fn needs_update(
        &self,
        authority_graph: &AuthorityGraph,
        current_roster: &Roster,
        effects: &aura_crypto::Effects,
    ) -> bool {
        // Get current authorized subjects
        let current_subjects =
            authority_graph.get_subjects_with_scope(&self.membership_scope, effects);
        let current_member_ids: BTreeSet<MemberId> = current_subjects
            .into_iter()
            .map(|subject| MemberId::new(&subject.0))
            .collect();

        // Compare with roster members
        let roster_member_ids: BTreeSet<_> = current_roster.members.keys().cloned().collect();

        let needs_update = current_member_ids != roster_member_ids;

        if needs_update {
            debug!("Roster update needed: capability changes detected");
            trace!("Current capabilities: {:?}", current_member_ids);
            trace!("Current roster: {:?}", roster_member_ids);
        }

        needs_update
    }

    /// Extract MLS member capabilities from authority graph
    pub fn extract_mls_members(
        &self,
        authority_graph: &AuthorityGraph,
        group_id: &str,
        effects: &aura_crypto::Effects,
    ) -> Result<Vec<MemberId>> {
        let mls_scope = CapabilityScope::with_resource("mls", "member", group_id);
        let authorized_subjects = authority_graph.get_subjects_with_scope(&mls_scope, effects);

        let members: Vec<MemberId> = authorized_subjects
            .into_iter()
            .map(|subject| MemberId::new(&subject.0))
            .collect();

        debug!(
            "Extracted {} MLS members for group {}",
            members.len(),
            group_id
        );

        Ok(members)
    }

    /// Build roster update from capability graph changes
    pub fn build_update(
        &mut self,
        authority_graph: &AuthorityGraph,
        current_roster: &Roster,
        effects: &aura_crypto::Effects,
    ) -> Result<Option<(Roster, RosterDelta)>> {
        if !self.needs_update(authority_graph, current_roster, effects) {
            return Ok(None);
        }

        let new_epoch = current_roster.epoch.next();
        let new_roster = self.build_roster(authority_graph, new_epoch, effects)?;
        let delta = self.compute_delta(current_roster, &new_roster);

        Ok(Some((new_roster, delta)))
    }

    /// Validate roster consistency with capability graph
    pub fn validate_roster(
        &self,
        roster: &Roster,
        authority_graph: &AuthorityGraph,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        let authorized_subjects =
            authority_graph.get_subjects_with_scope(&self.membership_scope, effects);
        let expected_members: BTreeSet<MemberId> = authorized_subjects
            .into_iter()
            .map(|subject| MemberId::new(&subject.0))
            .collect();

        let actual_members: BTreeSet<_> = roster.members.keys().cloned().collect();

        if expected_members != actual_members {
            let missing: Vec<_> = expected_members.difference(&actual_members).collect();
            let extra: Vec<_> = actual_members.difference(&expected_members).collect();

            return Err(AuraError::coordination_failed(format!(
                "Roster inconsistent with capabilities: missing {:?}, extra {:?}",
                missing, extra
            )));
        }

        Ok(())
    }
}

/// Utility functions for roster ordering
pub mod ordering {
    use super::*;

    /// Sort members deterministically for consistent tree positioning
    pub fn sort_members(members: &mut [MemberId]) {
        members.sort_by(|a, b| a.0.cmp(&b.0));
    }

    /// Compute deterministic tree position for a member
    pub fn compute_tree_position(member_id: &MemberId, roster_size: u32) -> TreePosition {
        // Use hash of member ID for deterministic but pseudo-random positioning
        let hash = blake3::hash(member_id.0.as_bytes());
        let position_index = u32::from_le_bytes([
            hash.as_bytes()[0],
            hash.as_bytes()[1],
            hash.as_bytes()[2],
            hash.as_bytes()[3],
        ]) % roster_size;
        TreePosition::leaf(position_index)
    }

    /// Rebalance tree positions after roster changes
    pub fn rebalance_positions(roster: &mut Roster) {
        let mut members: Vec<_> = roster.members.keys().cloned().collect();
        sort_members(&mut members);

        // Reassign positions sequentially for optimal tree balance
        roster.members.clear();
        for (index, member_id) in members.into_iter().enumerate() {
            let position = TreePosition::leaf(index as u32);
            roster.members.insert(member_id, position);
        }
    }
}
