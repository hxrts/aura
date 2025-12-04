//! # Block View State

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Resident {
    /// Resident identifier (authority ID)
    pub id: String,
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
}

/// Storage budget info
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct StorageBudget {
    /// Total storage budget in bytes
    pub total_bytes: u64,
    /// Used storage in bytes
    pub used_bytes: u64,
    /// Reserved storage in bytes
    pub reserved_bytes: u64,
}

impl StorageBudget {
    /// Get available storage in bytes
    pub fn available_bytes(&self) -> u64 {
        self.total_bytes
            .saturating_sub(self.used_bytes)
            .saturating_sub(self.reserved_bytes)
    }

    /// Get usage percentage (0-100)
    pub fn usage_percentage(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }
}

/// Block state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct BlockState {
    /// Block identifier
    pub id: String,
    /// Block name
    pub name: String,
    /// All residents
    pub residents: Vec<Resident>,
    /// Current user's role
    pub my_role: ResidentRole,
    /// Storage budget
    pub storage: StorageBudget,
    /// Number of online residents
    pub online_count: u32,
    /// Total resident count
    pub resident_count: u32,
}

impl BlockState {
    /// Get resident by ID
    pub fn resident(&self, id: &str) -> Option<&Resident> {
        self.residents.iter().find(|r| r.id == id)
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
}
