// BeeKEM CGKA state management

use crate::events::*;
use crate::types::*;
use crate::{AuraError, Result};
use std::collections::BTreeMap;
use tracing::{debug, warn};
// Note: This module should be updated to use effects for time

/// BeeKEM group state with concurrent operation support
#[derive(Debug, Clone)]
pub struct CgkaState {
    /// Group identifier
    pub group_id: String,
    /// Current epoch number
    pub current_epoch: Epoch,
    /// Group member roster
    pub roster: Roster,
    /// Binary tree state
    pub tree: BeeKemTree,
    /// Pending operations (not yet committed)
    pub pending_operations: BTreeMap<OperationId, KeyhiveCgkaOperation>,
    /// Application secrets for each epoch
    pub application_secrets: BTreeMap<Epoch, ApplicationSecret>,
    /// Last state update timestamp
    pub last_updated: u64,
}

impl CgkaState {
    /// Create new CGKA state for a group
    pub fn new(
        group_id: String,
        initial_members: Vec<MemberId>,
        effects: &aura_crypto::Effects,
    ) -> Result<Self> {
        let epoch = Epoch::initial();
        let mut roster = Roster::new(epoch);

        // Add initial members to roster
        for member_id in initial_members {
            roster.add_member(member_id);
        }

        // Initialize tree with roster
        let tree = BeeKemTree::new(roster.size)?;

        Ok(Self {
            group_id,
            current_epoch: epoch,
            roster,
            tree,
            pending_operations: BTreeMap::new(),
            application_secrets: BTreeMap::new(),
            last_updated: effects.now().unwrap_or(0),
        })
    }

    /// Apply a CGKA operation to the state
    pub fn apply_operation(
        &mut self,
        operation: KeyhiveCgkaOperation,
        effects: &aura_crypto::Effects,
    ) -> Result<()> {
        debug!(
            "Applying CGKA operation {:?} for epoch {}",
            operation.operation_id,
            operation.target_epoch.value()
        );

        // Validate operation can be applied
        self.validate_operation(&operation)?;

        // Apply roster changes
        self.apply_roster_delta(&operation.roster_delta)?;

        // Apply tree updates
        for tree_update in &operation.tree_updates {
            self.tree.apply_update(tree_update)?;
        }

        // Advance epoch
        self.current_epoch = operation.target_epoch;
        self.roster.epoch = operation.target_epoch;

        // Derive new application secret
        let app_secret = self.derive_application_secret()?;
        self.application_secrets
            .insert(self.current_epoch, app_secret);

        // Remove from pending if it was there
        self.pending_operations.remove(&operation.operation_id);

        self.last_updated = effects.now().unwrap_or(0);

        debug!(
            "CGKA operation applied successfully, now at epoch {}",
            self.current_epoch.value()
        );

        Ok(())
    }

    /// Add pending operation (not yet committed)
    pub fn add_pending_operation(&mut self, operation: KeyhiveCgkaOperation) {
        debug!("Adding pending CGKA operation {:?}", operation.operation_id);
        self.pending_operations
            .insert(operation.operation_id, operation);
    }

    /// Remove pending operation
    pub fn remove_pending_operation(
        &mut self,
        operation_id: &OperationId,
    ) -> Option<KeyhiveCgkaOperation> {
        self.pending_operations.remove(operation_id)
    }

    /// Get application secret for specific epoch
    pub fn get_application_secret(&self, epoch: Epoch) -> Option<&ApplicationSecret> {
        self.application_secrets.get(&epoch)
    }

    /// Get current application secret
    pub fn current_application_secret(&self) -> Option<&ApplicationSecret> {
        self.get_application_secret(self.current_epoch)
    }

    /// Check if member exists in current roster
    pub fn is_member(&self, member_id: &MemberId) -> bool {
        self.roster.is_member(member_id)
    }

    /// Get tree position for a member
    pub fn get_member_position(&self, member_id: &MemberId) -> Option<TreePosition> {
        self.roster.get_position(member_id)
    }

    /// Validate that an operation can be applied to current state
    fn validate_operation(&self, operation: &KeyhiveCgkaOperation) -> Result<()> {
        // Check epoch is correct
        if operation.current_epoch != self.current_epoch {
            return Err(AuraError::epoch_mismatch(format!(
                "Epoch mismatch: expected {}, actual {}",
                self.current_epoch.value(),
                operation.current_epoch.value()
            )));
        }

        // Check group ID matches
        if operation.group_id != self.group_id {
            return Err(AuraError::coordination_failed(format!(
                "Group ID mismatch: expected {}, got {}",
                self.group_id, operation.group_id
            )));
        }

        // Validate roster delta is consistent
        self.validate_roster_delta(&operation.roster_delta)?;

        Ok(())
    }

    /// Validate roster delta can be applied
    fn validate_roster_delta(&self, delta: &RosterDelta) -> Result<()> {
        if delta.previous_size != self.roster.size {
            return Err(AuraError::coordination_failed(format!(
                "Roster size mismatch: expected {}, got {}",
                self.roster.size, delta.previous_size
            )));
        }

        // Check removed members exist
        for member_id in &delta.removed_members {
            if !self.roster.is_member(member_id) {
                return Err(AuraError::coordination_failed(member_id.0.clone()));
            }
        }

        // Check added members don't already exist
        for member_id in &delta.added_members {
            if self.roster.is_member(member_id) {
                return Err(AuraError::coordination_failed(format!(
                    "Member {} already exists in roster",
                    member_id.0
                )));
            }
        }

        Ok(())
    }

    /// Apply roster delta to current roster
    fn apply_roster_delta(&mut self, delta: &RosterDelta) -> Result<()> {
        // Remove members
        for member_id in &delta.removed_members {
            if self.roster.remove_member(member_id).is_none() {
                warn!("Attempted to remove non-existent member: {}", member_id.0);
            }
        }

        // Add members
        for member_id in &delta.added_members {
            self.roster.add_member(member_id.clone());
        }

        Ok(())
    }

    /// Derive application secret from current tree state
    fn derive_application_secret(&self) -> Result<ApplicationSecret> {
        // Get root secret from tree
        let root_secret = self
            .tree
            .get_root_secret()
            .ok_or_else(|| AuraError::encryption_failed("No root secret available".to_string()))?;

        Ok(ApplicationSecret {
            secret: root_secret,
            epoch: self.current_epoch,
            context: self.group_id.clone(),
        })
    }
}

/// BeeKEM binary tree for CGKA operations
#[derive(Debug, Clone)]
pub struct BeeKemTree {
    /// Tree nodes indexed by position
    pub nodes: BTreeMap<TreePosition, TreeNode>,
    /// Tree size (number of leaf positions)
    pub size: u32,
    /// Root secret for current epoch
    pub root_secret: Option<Vec<u8>>,
}

impl BeeKemTree {
    /// Create new tree with specified size
    pub fn new(size: u32) -> Result<Self> {
        if size == 0 {
            return Err(AuraError::coordination_failed(
                "Tree size cannot be zero".to_string(),
            ));
        }

        let mut tree = Self {
            nodes: BTreeMap::new(),
            size,
            root_secret: None,
        };

        // Initialize leaf nodes
        for i in 0..size {
            let position = TreePosition::leaf(i);
            tree.nodes.insert(position, TreeNode::new(position));
        }

        Ok(tree)
    }

    /// Apply tree update operation
    pub fn apply_update(&mut self, update: &TreeUpdate) -> Result<()> {
        match &update.update_type {
            TreeUpdateType::AddLeaf {
                member_id,
                key_package,
            } => {
                self.add_leaf(update.position, member_id.clone(), key_package.clone())?;
            }
            TreeUpdateType::RemoveLeaf { member_id: _ } => {
                self.remove_leaf(update.position)?;
            }
            TreeUpdateType::UpdateNode { new_public_key } => {
                self.update_node(update.position, new_public_key.clone())?;
            }
        }

        // Apply path updates
        for path_update in &update.path_updates {
            self.update_path_node(path_update)?;
        }

        Ok(())
    }

    /// Add new leaf to tree
    fn add_leaf(
        &mut self,
        position: TreePosition,
        _member_id: MemberId,
        key_package: KeyPackage,
    ) -> Result<()> {
        if !position.is_leaf() {
            return Err(AuraError::coordination_failed(
                "Position is not a leaf".to_string(),
            ));
        }

        let mut node = TreeNode::new(position);
        node.public_key = Some(key_package.init_key);

        self.nodes.insert(position, node);
        Ok(())
    }

    /// Remove leaf from tree
    fn remove_leaf(&mut self, position: TreePosition) -> Result<()> {
        if !position.is_leaf() {
            return Err(AuraError::coordination_failed(
                "Position is not a leaf".to_string(),
            ));
        }

        self.nodes.remove(&position);
        Ok(())
    }

    /// Update node public key
    fn update_node(&mut self, position: TreePosition, new_public_key: PublicKey) -> Result<()> {
        let node = self.nodes.get_mut(&position).ok_or_else(|| {
            AuraError::coordination_failed(format!("Node at position {} not found", position.0))
        })?;

        node.public_key = Some(new_public_key);
        Ok(())
    }

    /// Update node in tree path
    fn update_path_node(&mut self, path_update: &PathUpdate) -> Result<()> {
        let node = self.nodes.get_mut(&path_update.position).ok_or_else(|| {
            AuraError::coordination_failed(format!(
                "Node at position {} not found",
                path_update.position.0
            ))
        })?;

        node.public_key = Some(path_update.public_key.clone());
        // Note: encrypted_secret would be processed differently in a full implementation

        Ok(())
    }

    /// Get root secret for application key derivation
    pub fn get_root_secret(&self) -> Option<Vec<u8>> {
        self.root_secret.clone()
    }

    /// Set root secret (typically derived during tree operations)
    pub fn set_root_secret(&mut self, secret: Vec<u8>) {
        self.root_secret = Some(secret);
    }
}
