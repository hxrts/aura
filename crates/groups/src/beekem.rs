// BeeKEM CGKA protocol implementation

use crate::events::*;
use crate::roster::RosterBuilder;
use crate::state::CgkaState;
use crate::types::*;
use crate::{CgkaError, Result};
use aura_journal::capability::authority_graph::AuthorityGraph;
// Remove unused import - timestamps now use effects system
use std::collections::BTreeMap;
use tracing::{debug, info};

/// BeeKEM protocol manager for capability-driven CGKA
pub struct BeeKemManager {
    /// Active group states
    pub groups: BTreeMap<String, CgkaState>,
    /// Roster builders for each group
    pub roster_builders: BTreeMap<String, RosterBuilder>,
    /// Pending operations awaiting capability validation
    pub pending_operations: BTreeMap<OperationId, KeyhiveCgkaOperation>,
    /// Injectable effects for deterministic testing
    pub effects: aura_crypto::Effects,
}

impl BeeKemManager {
    /// Create new BeeKEM manager
    pub fn new(effects: aura_crypto::Effects) -> Self {
        Self {
            groups: BTreeMap::new(),
            roster_builders: BTreeMap::new(),
            pending_operations: BTreeMap::new(),
            effects,
        }
    }
    
    /// Initialize new group from capability graph
    pub fn initialize_group(
        &mut self,
        group_id: String,
        authority_graph: &AuthorityGraph,
    ) -> Result<()> {
        info!("Initializing BeeKEM group: {}", group_id);
        
        // Create roster builder for this group
        let roster_builder = RosterBuilder::for_mls_group(&group_id);
        
        // Extract initial members from capability graph
        let initial_members = roster_builder.extract_mls_members(authority_graph, &group_id, &self.effects)?;
        
        if initial_members.is_empty() {
            return Err(CgkaError::InvalidOperation(
                "Cannot initialize group with no members".to_string()
            ));
        }
        
        // Create initial group state
        let group_state = CgkaState::new(group_id.clone(), initial_members.clone(), &self.effects)?;
        
        debug!("Initialized group {} with {} members", group_id, initial_members.len());
        
        self.groups.insert(group_id.clone(), group_state);
        self.roster_builders.insert(group_id, roster_builder);
        
        Ok(())
    }
    
    /// Process capability changes and generate CGKA operations
    pub fn process_capability_changes(
        &mut self,
        group_id: &str,
        authority_graph: &AuthorityGraph,
    ) -> Result<Vec<KeyhiveCgkaOperation>> {
        let group_state = self.groups.get(group_id)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("Group {} not found", group_id)))?;
        
        let roster_builder = self.roster_builders.get_mut(group_id)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("Roster builder for group {} not found", group_id)))?;
        
        // Check if roster needs update
        if !roster_builder.needs_update(authority_graph, &group_state.roster, &self.effects) {
            debug!("No roster update needed for group {}", group_id);
            return Ok(Vec::new());
        }
        
        info!("Processing capability changes for group {}", group_id);
        
        // Build updated roster
        let (_new_roster, roster_delta) = roster_builder
            .build_update(authority_graph, &group_state.roster, &self.effects)?
            .ok_or_else(|| CgkaError::InvalidOperation("Expected roster update but none available".to_string()))?;
        
        // Generate CGKA operation based on roster changes
        let operation = self.generate_cgka_operation(
            group_id.to_string(),
            group_state.current_epoch,
            &roster_delta,
        )?;
        
        debug!("Generated CGKA operation {:?} for group {}", operation.operation_id, group_id);
        
        Ok(vec![operation])
    }
    
    /// Generate CGKA operation from roster delta
    fn generate_cgka_operation(
        &self,
        group_id: String,
        current_epoch: Epoch,
        roster_delta: &RosterDelta,
    ) -> Result<KeyhiveCgkaOperation> {
        // Determine operation type
        let operation_type = if !roster_delta.added_members.is_empty() && !roster_delta.removed_members.is_empty() {
            // Both adds and removes - this is complex but we'll handle as update
            CgkaOperationType::Update
        } else if !roster_delta.added_members.is_empty() {
            CgkaOperationType::Add {
                members: roster_delta.added_members.clone(),
            }
        } else if !roster_delta.removed_members.is_empty() {
            CgkaOperationType::Remove {
                members: roster_delta.removed_members.clone(),
            }
        } else {
            CgkaOperationType::Update
        };
        
        // Generate tree updates based on roster changes
        let tree_updates = self.generate_tree_updates(&operation_type, roster_delta, &group_id)?;
        
        // Create operation (signature will be added later)
        let operation = KeyhiveCgkaOperation::new(
            group_id,
            current_epoch,
            operation_type,
            roster_delta.clone(),
            tree_updates,
            aura_journal::DeviceId::from_string_with_effects("placeholder", &self.effects), // TODO: Get actual device ID
            &self.effects,
        );
        
        Ok(operation)
    }
    
    /// Generate tree updates for roster changes
    fn generate_tree_updates(
        &self,
        operation_type: &CgkaOperationType,
        roster_delta: &RosterDelta,
        group_id: &str,
    ) -> Result<Vec<TreeUpdate>> {
        let mut updates = Vec::new();
        
        match operation_type {
            CgkaOperationType::Add { members } => {
                for (index, member_id) in members.iter().enumerate() {
                    let position = TreePosition::leaf(roster_delta.previous_size + index as u32);
                    
                    // Generate dummy key package (would be real in production)
                    let key_package = KeyPackage {
                        member_id: member_id.clone(),
                        init_key: PublicKey::new(vec![0u8; 32]), // Dummy key
                        signature: vec![0u8; 64], // Dummy signature
                        created_at: self.effects.now().unwrap_or(0),
                    };
                    
                    let update = TreeUpdate {
                        position,
                        update_type: TreeUpdateType::AddLeaf {
                            member_id: member_id.clone(),
                            key_package,
                        },
                        path_updates: self.generate_path_updates_for_add(position, &member_id)?,
                    };
                    
                    updates.push(update);
                }
            }
            CgkaOperationType::Remove { members: _ } => {
                for member_id in &roster_delta.removed_members {
                    // TODO: Look up actual position from current roster
                    let position = TreePosition::leaf(0); // Placeholder
                    
                    let update = TreeUpdate {
                        position,
                        update_type: TreeUpdateType::RemoveLeaf {
                            member_id: member_id.clone(),
                        },
                        path_updates: self.generate_path_updates_for_remove(position)?,
                    };
                    
                    updates.push(update);
                }
            }
            CgkaOperationType::Update => {
                // Generate tree refresh updates to rebalance the tree
                let refresh_updates = self.generate_tree_refresh_updates(group_id)?;
                updates.extend(refresh_updates);
            }
            CgkaOperationType::Init { initial_members } => {
                // Generate initialization tree setup for all initial members
                let init_updates = self.generate_initialization_updates(initial_members)?;
                updates.extend(init_updates);
            }
        }
        
        Ok(updates)
    }
    
    /// Apply CGKA operation to group state
    pub fn apply_operation(
        &mut self,
        group_id: &str,
        operation: KeyhiveCgkaOperation,
    ) -> Result<()> {
        let group_state = self.groups.get_mut(group_id)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("Group {} not found", group_id)))?;
        
        info!("Applying CGKA operation {:?} to group {}", operation.operation_id, group_id);
        
        group_state.apply_operation(operation, &self.effects)?;
        
        Ok(())
    }
    
    /// Get current application secret for a group
    pub fn get_application_secret(&self, group_id: &str) -> Option<&ApplicationSecret> {
        self.groups.get(group_id)?.current_application_secret()
    }
    
    /// Derive encryption key for specific purpose
    pub fn derive_encryption_key(&self, group_id: &str, purpose: &str) -> Option<Vec<u8>> {
        let app_secret = self.get_application_secret(group_id)?;
        Some(app_secret.derive_key(purpose))
    }
    
    /// Check if member is in group
    pub fn is_member(&self, group_id: &str, member_id: &MemberId) -> bool {
        self.groups.get(group_id)
            .map(|state| state.is_member(member_id))
            .unwrap_or(false)
    }
    
    /// Get group roster
    pub fn get_roster(&self, group_id: &str) -> Option<&Roster> {
        self.groups.get(group_id).map(|state| &state.roster)
    }
    
    /// Get group epoch
    pub fn get_epoch(&self, group_id: &str) -> Option<Epoch> {
        self.groups.get(group_id).map(|state| state.current_epoch)
    }
    
    /// Validate operation against current group state
    pub fn validate_operation(
        &self,
        group_id: &str,
        operation: &KeyhiveCgkaOperation,
    ) -> Result<()> {
        let group_state = self.groups.get(group_id)
            .ok_or_else(|| CgkaError::InvalidOperation(format!("Group {} not found", group_id)))?;
        
        // Basic validation - would be more comprehensive in production
        if operation.current_epoch != group_state.current_epoch {
            return Err(CgkaError::EpochMismatch {
                expected: group_state.current_epoch.value(),
                actual: operation.current_epoch.value(),
            });
        }
        
        Ok(())
    }
    
    /// Generate path updates for adding a new member
    fn generate_path_updates_for_add(&self, position: TreePosition, member_id: &MemberId) -> Result<Vec<PathUpdate>> {
        let mut path_updates = Vec::new();
        
        // Generate key material for the new leaf and its path to root
        let mut current_pos = position;
        
        // Walk up the tree from leaf to root, generating new keys for each node
        while current_pos.0 > 1 {
            let parent_pos = current_pos.parent();
            
            // Generate new key pair for this position in the path
            let (public_key, _private_key) = self.generate_keypair_for_position(parent_pos, member_id)?;
            
            // Create encrypted secret for path update (simplified)
            let encrypted_secret = self.encrypt_path_secret(&public_key, member_id)?;
            
            path_updates.push(PathUpdate {
                position: parent_pos,
                public_key,
                encrypted_secret,
            });
            
            current_pos = parent_pos;
        }
        
        debug!("Generated {} path updates for adding member {} at position {:?}", 
               path_updates.len(), member_id.0, position);
        
        Ok(path_updates)
    }
    
    /// Generate path updates for removing a member
    fn generate_path_updates_for_remove(&self, position: TreePosition) -> Result<Vec<PathUpdate>> {
        let mut path_updates = Vec::new();
        
        // For removal, we need to refresh the entire path from the removed leaf to root
        let mut current_pos = position;
        
        // Walk up the tree, generating fresh keys for each position to maintain forward secrecy
        while current_pos.0 > 1 {
            let parent_pos = current_pos.parent();
            
            // Generate new key pair for this position (no specific member for removal)
            let (public_key, _private_key) = self.generate_keypair_for_position(parent_pos, &MemberId::new("removal"))?;
            
            // Create encrypted secret for remaining members
            let encrypted_secret = self.encrypt_removal_secret(&public_key)?;
            
            path_updates.push(PathUpdate {
                position: parent_pos,
                public_key,
                encrypted_secret,
            });
            
            current_pos = parent_pos;
        }
        
        debug!("Generated {} path updates for member removal at position {:?}", 
               path_updates.len(), position);
        
        Ok(path_updates)
    }
    
    /// Generate tree refresh updates to rebalance the tree
    fn generate_tree_refresh_updates(&self, group_id: &str) -> Result<Vec<TreeUpdate>> {
        let mut updates = Vec::new();
        
        // Get current group state to determine tree structure
        if let Some(group_state) = self.groups.get(group_id) {
            let member_count = group_state.roster.member_count();
            
            // For simplicity, generate updates for all internal nodes to refresh keys
            for i in 1..(member_count * 2) {
                let position = TreePosition(i as u32);
                
                if !position.is_leaf() {
                    // Generate refresh update for internal node
                    let (public_key, _private_key) = self.generate_keypair_for_position(position, &MemberId::new("refresh"))?;
                    let encrypted_secret = self.encrypt_refresh_secret(&public_key)?;
                    
                    updates.push(TreeUpdate {
                        position,
                        update_type: TreeUpdateType::UpdateNode {
                            new_public_key: public_key.clone(),
                        },
                        path_updates: vec![PathUpdate {
                            position,
                            public_key,
                            encrypted_secret,
                        }],
                    });
                }
            }
        }
        
        debug!("Generated {} tree refresh updates for group {}", updates.len(), group_id);
        
        Ok(updates)
    }
    
    /// Generate initialization updates for setting up the tree with initial members
    fn generate_initialization_updates(&self, initial_members: &[MemberId]) -> Result<Vec<TreeUpdate>> {
        let mut updates = Vec::new();
        
        // Create tree structure for initial members
        for (index, member_id) in initial_members.iter().enumerate() {
            let position = TreePosition::leaf(index as u32);
            
            // Create key package for each initial member
            let (public_key, _private_key) = self.generate_keypair_for_position(position, member_id)?;
            
            let key_package = KeyPackage {
                member_id: member_id.clone(),
                init_key: public_key,
                signature: vec![0u8; 64], // Placeholder signature
                created_at: self.effects.now().unwrap_or(0),
            };
            
            // Generate path updates for this member
            let path_updates = self.generate_path_updates_for_add(position, member_id)?;
            
            updates.push(TreeUpdate {
                position,
                update_type: TreeUpdateType::AddLeaf {
                    member_id: member_id.clone(),
                    key_package,
                },
                path_updates,
            });
        }
        
        debug!("Generated {} initialization updates for {} members", 
               updates.len(), initial_members.len());
        
        Ok(updates)
    }
    
    /// Generate a keypair for a specific tree position
    fn generate_keypair_for_position(&self, position: TreePosition, context: &MemberId) -> Result<(PublicKey, PrivateKey)> {
        // Use deterministic key generation based on position and context
        let mut key_material = Vec::new();
        key_material.extend_from_slice(&position.0.to_le_bytes());
        key_material.extend_from_slice(context.0.as_bytes());
        key_material.extend_from_slice(&self.effects.random_bytes::<32>());
        
        // Generate key from material (simplified - in production would use proper KDF)
        let private_key_bytes = blake3::hash(&key_material).as_bytes().to_vec();
        let public_key_bytes = blake3::hash(&[&private_key_bytes[..], b"public"].concat()).as_bytes().to_vec();
        
        Ok((
            PublicKey::new(public_key_bytes),
            PrivateKey::new(private_key_bytes),
        ))
    }
    
    /// Encrypt secret for path update during member addition
    fn encrypt_path_secret(&self, public_key: &PublicKey, _member_id: &MemberId) -> Result<Vec<u8>> {
        // Simplified encryption - in production would use proper HPKE
        let secret = self.effects.random_bytes::<32>();
        let mut encrypted = public_key.as_bytes().to_vec();
        encrypted.extend_from_slice(&secret);
        Ok(blake3::hash(&encrypted).as_bytes().to_vec())
    }
    
    /// Encrypt secret for path update during member removal
    fn encrypt_removal_secret(&self, public_key: &PublicKey) -> Result<Vec<u8>> {
        // Generate fresh secret for forward secrecy
        let secret = self.effects.random_bytes::<32>();
        let mut encrypted = public_key.as_bytes().to_vec();
        encrypted.extend_from_slice(&secret);
        Ok(blake3::hash(&encrypted).as_bytes().to_vec())
    }
    
    /// Encrypt secret for tree refresh
    fn encrypt_refresh_secret(&self, public_key: &PublicKey) -> Result<Vec<u8>> {
        // Generate fresh secret for tree refresh
        let secret = self.effects.random_bytes::<32>();
        let mut encrypted = public_key.as_bytes().to_vec();
        encrypted.extend_from_slice(&secret);
        Ok(blake3::hash(&encrypted).as_bytes().to_vec())
    }
}

impl Default for BeeKemManager {
    fn default() -> Self {
        Self::new(aura_crypto::Effects::test())
    }
}

