//! Capability system types
//!
//! This module provides types for the capability-based access control system
//! used throughout the Aura platform.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::hash::Hash;

/// Capability identifier for access control
///
/// Unique identifier for capabilities within the capability system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilityId(pub [u8; 32]);

impl CapabilityId {
    /// Create a new capability ID
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from a byte slice
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::TypeError> {
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(bytes);
            Ok(Self(array))
        } else {
            Err(crate::TypeError::InvalidIdentifier(format!(
                "CapabilityId must be exactly 32 bytes, got {}",
                bytes.len()
            )))
        }
    }

    /// Create from blake3 hash
    pub fn from_blake3_hash(hash: &blake3::Hash) -> Self {
        Self(*hash.as_bytes())
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(Self(array))
        } else {
            Err(hex::FromHexError::InvalidStringLength)
        }
    }

    /// Generate a random capability ID
    #[allow(clippy::disallowed_methods)]
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(blake3::hash(uuid::Uuid::new_v4().as_bytes()).as_bytes());
        Self(bytes)
    }

    /// Generate a deterministic capability ID from a parent chain
    ///
    /// Creates a capability ID based on a parent capability, subject identifier,
    /// and scope. This allows for deterministic derivation of child capabilities.
    pub fn from_chain(
        parent_id: Option<&CapabilityId>,
        subject_id: &[u8],
        scope_data: &[u8],
    ) -> Self {
        let mut hasher = blake3::Hasher::new();

        if let Some(parent) = parent_id {
            hasher.update(&parent.0);
        }

        hasher.update(subject_id);
        hasher.update(scope_data);

        Self(hasher.finalize().into())
    }

    /// Generate a capability ID from device and timestamp
    /// This creates a deterministic ID for device-specific capabilities
    pub fn from_device_and_timestamp(device_id: crate::DeviceId, timestamp: u64) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(device_id.0.as_bytes());
        hasher.update(&timestamp.to_le_bytes());
        hasher.update(b"capability");
        Self(hasher.finalize().into())
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "capability-{}", self.to_hex())
    }
}

impl From<[u8; 32]> for CapabilityId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<blake3::Hash> for CapabilityId {
    fn from(hash: blake3::Hash) -> Self {
        Self::from_blake3_hash(&hash)
    }
}

impl From<CapabilityId> for [u8; 32] {
    fn from(capability_id: CapabilityId) -> Self {
        capability_id.0
    }
}

/// Capability scope enumeration
///
/// Defines the scope of access that a capability grants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityScope {
    /// Read-only access
    Read,
    /// Write access (includes read)
    Write,
    /// Execute access for operations
    Execute,
    /// Administrative access (full control)
    Admin,
    /// Custom scope with specific permissions
    Custom(String),
}

impl fmt::Display for CapabilityScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityScope::Read => write!(f, "read"),
            CapabilityScope::Write => write!(f, "write"),
            CapabilityScope::Execute => write!(f, "execute"),
            CapabilityScope::Admin => write!(f, "admin"),
            CapabilityScope::Custom(custom) => write!(f, "custom:{}", custom),
        }
    }
}

impl CapabilityScope {
    /// Check if this scope includes read access
    pub fn includes_read(&self) -> bool {
        matches!(
            self,
            CapabilityScope::Read | CapabilityScope::Write | CapabilityScope::Admin
        )
    }

    /// Check if this scope includes write access
    pub fn includes_write(&self) -> bool {
        matches!(self, CapabilityScope::Write | CapabilityScope::Admin)
    }

    /// Check if this scope includes execute access
    pub fn includes_execute(&self) -> bool {
        matches!(self, CapabilityScope::Execute | CapabilityScope::Admin)
    }

    /// Check if this scope includes admin access
    pub fn includes_admin(&self) -> bool {
        matches!(self, CapabilityScope::Admin)
    }
}

/// Capability resource type
///
/// Identifies the type of resource that a capability controls access to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityResource {
    /// Storage chunk access
    Chunk(crate::content::ChunkId),
    /// Session access
    Session(crate::identifiers::SessionId),
    /// Account access
    Account(crate::identifiers::AccountId),
    /// Device access
    Device(crate::identifiers::DeviceId),
    /// Protocol access
    Protocol(crate::protocols::ProtocolType),
    /// Custom resource type
    Custom {
        /// Type of the custom resource
        resource_type: String,
        /// Identifier of the specific resource
        resource_id: String,
    },
}

impl fmt::Display for CapabilityResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityResource::Chunk(chunk_id) => write!(f, "chunk:{}", chunk_id),
            CapabilityResource::Session(session_id) => write!(f, "session:{}", session_id),
            CapabilityResource::Account(account_id) => write!(f, "account:{}", account_id.0),
            CapabilityResource::Device(device_id) => write!(f, "device:{}", device_id.0),
            CapabilityResource::Protocol(protocol_type) => write!(f, "protocol:{}", protocol_type),
            CapabilityResource::Custom {
                resource_type,
                resource_id,
            } => {
                write!(f, "{}:{}", resource_type, resource_id)
            }
        }
    }
}

/// Permission types for capability-based access control
///
/// Defines the types of permissions that can be granted through capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Read access to storage or data
    StorageRead,
    /// Write access to storage or data
    StorageWrite,
    /// Delete access to storage or data
    StorageDelete,
    /// Send messages over communication channels
    CommunicationSend,
    /// Receive messages over communication channels
    CommunicationReceive,
    /// Execute protocol operations
    ProtocolExecute,
    /// Modify protocol state
    ProtocolModify,
    /// Access recovery mechanisms
    RecoveryAccess,
    /// Administrative access (full control)
    Admin,
    /// Custom permission type
    Custom(String),
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Permission::StorageRead => write!(f, "storage:read"),
            Permission::StorageWrite => write!(f, "storage:write"),
            Permission::StorageDelete => write!(f, "storage:delete"),
            Permission::CommunicationSend => write!(f, "communication:send"),
            Permission::CommunicationReceive => write!(f, "communication:receive"),
            Permission::ProtocolExecute => write!(f, "protocol:execute"),
            Permission::ProtocolModify => write!(f, "protocol:modify"),
            Permission::RecoveryAccess => write!(f, "recovery:access"),
            Permission::Admin => write!(f, "admin"),
            Permission::Custom(perm) => write!(f, "custom:{}", perm),
        }
    }
}

/// Capability token for delegated access
///
/// Represents a delegated capability that grants specific permissions
/// for a specific resource with optional constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique identifier for this capability
    pub id: CapabilityId,
    /// Subject (entity) that the capability is granted to
    pub subject: crate::identifiers::DeviceId,
    /// Resource this capability grants access to
    pub resource: CapabilityResource,
    /// Permissions granted by this capability
    pub permissions: Vec<Permission>,
    /// Scope of this capability
    pub scope: CapabilityScope,
    /// When this capability expires
    pub expiration: CapabilityExpiration,
    /// Optional parent capability (for delegation chains)
    pub parent_id: Option<CapabilityId>,
    /// Timestamp when capability was created
    pub created_at: u64,
    /// Signature over the capability data (for verification)
    pub signature: Vec<u8>,
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        subject: crate::identifiers::DeviceId,
        resource: CapabilityResource,
        permissions: Vec<Permission>,
        scope: CapabilityScope,
        expiration: CapabilityExpiration,
        current_timestamp: u64,
    ) -> Self {
        let now = current_timestamp;

        let id = CapabilityId::from_chain(
            None,
            subject.0.as_bytes(),
            format!("{:?}", resource).as_bytes(),
        );

        Self {
            id,
            subject,
            resource,
            permissions,
            scope,
            expiration,
            parent_id: None,
            created_at: now,
            signature: vec![],
        }
    }

    /// Create a derived capability token (delegation)
    pub fn derive(
        &self,
        new_subject: crate::identifiers::DeviceId,
        new_permissions: Vec<Permission>,
        current_timestamp: u64,
    ) -> Self {
        let now = current_timestamp;

        let id = CapabilityId::from_chain(
            Some(&self.id),
            new_subject.0.as_bytes(),
            format!("{:?}", new_permissions).as_bytes(),
        );

        Self {
            id,
            subject: new_subject,
            resource: self.resource.clone(),
            permissions: new_permissions,
            scope: self.scope.clone(),
            expiration: self.expiration.clone(),
            parent_id: Some(self.id),
            created_at: now,
            signature: vec![],
        }
    }

    /// Check if this capability is still valid at the given timestamp
    pub fn is_valid(&self, current_timestamp: u64) -> bool {
        !self.expiration.has_expired(current_timestamp)
    }

    /// Check if this capability grants a specific permission
    pub fn grants_permission(&self, permission: &Permission) -> bool {
        self.permissions.iter().any(|p| p == permission)
            || matches!(
                self.permissions.iter().find(|_| true),
                Some(Permission::Admin)
            )
    }
}

impl PartialEq for CapabilityToken {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CapabilityToken {}

impl Hash for CapabilityToken {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// Capability expiration time
///
/// Defines when a capability expires.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityExpiration {
    /// Never expires
    Never,
    /// Expires at a specific timestamp (Unix timestamp)
    Timestamp(u64),
    /// Expires after a duration (seconds from creation)
    Duration(u64),
    /// Expires at the end of a session
    SessionEnd(crate::identifiers::SessionId),
}

impl fmt::Display for CapabilityExpiration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityExpiration::Never => write!(f, "never"),
            CapabilityExpiration::Timestamp(timestamp) => write!(f, "timestamp:{}", timestamp),
            CapabilityExpiration::Duration(seconds) => write!(f, "duration:{}s", seconds),
            CapabilityExpiration::SessionEnd(session_id) => write!(f, "session-end:{}", session_id),
        }
    }
}

impl CapabilityExpiration {
    /// Check if the capability has expired at the given timestamp
    pub fn has_expired(&self, current_timestamp: u64) -> bool {
        match self {
            CapabilityExpiration::Never => false,
            CapabilityExpiration::Timestamp(expiry) => current_timestamp >= *expiry,
            CapabilityExpiration::Duration(_) => {
                // Duration-based expiration needs creation time to determine expiry
                // This should be handled by the capability management system
                false
            }
            CapabilityExpiration::SessionEnd(_) => {
                // Session-based expiration needs session state to determine expiry
                // This should be handled by the capability management system
                false
            }
        }
    }
}

// =============================================================================
// Conversion Traits for Layered Capability System
// =============================================================================

/// Capability Type Layer Documentation
///
/// The Aura capability system uses a layered approach with clear separation of concerns:
///
/// 1. **aura-types::CapabilityToken** - Lightweight, canonical foundation type
///    - Wire format compatible
///    - Universal reference type
///    - Minimal dependencies
///
/// 2. **aura-authorization::CapabilityToken** - Rich policy enforcement
///    - Cryptographic signatures
///    - Rich conditions and constraints
///    - Delegation depth tracking
///
/// 3. **aura-journal::CapabilityToken** - Ledger event representation
///    - Clean auth/authz separation
///    - Domain-specific permissions
///    - Event-sourced state management
///
/// 4. **aura-store::CapabilityManager** - Storage lifecycle management
///    - Uses aura-authorization types
///    - Capability tracking and revocation
///    - Integration layer
///
/// Trait for converting canonical capability tokens to authorization layer tokens
pub trait IntoAuthorizationToken {
    /// Convert to authorization token with signature and issuer
    fn into_authorization_token(
        self,
        issuer: crate::identifiers::DeviceId,
        signature: Vec<u8>,
    ) -> AuthorizationCapabilityToken;
}

/// Trait for converting canonical capability tokens to journal layer tokens
pub trait IntoJournalToken {
    /// Convert to journal token for event recording
    fn into_journal_token(
        self,
        authenticated_device: crate::identifiers::DeviceId,
        delegation_chain: Vec<CapabilityId>,
        signature: Vec<u8>,
    ) -> JournalCapabilityToken;
}

/// Authorization layer capability token representation
///
/// This mirrors the structure from aura-authorization but without the heavyweight dependencies
/// for conversion purposes. The actual aura-authorization::CapabilityToken should be used
/// for authorization operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorizationCapabilityToken {
    /// Unique identifier for this capability token
    pub id: CapabilityId,
    /// The device/subject this capability is granted to
    pub subject: crate::identifiers::DeviceId,
    /// The resource this capability grants access to
    pub resource: CapabilityResource,
    /// List of permissions granted by this capability
    pub permissions: Vec<Permission>,
    /// Scope limitations for this capability
    pub scope: CapabilityScope,
    /// Unix timestamp when this capability was issued
    pub issued_at: u64,
    /// Optional Unix timestamp when this capability expires
    pub expires_at: Option<u64>,
    /// The device that issued this capability
    pub issuer: crate::identifiers::DeviceId,
    /// Cryptographic signature of this capability
    pub signature: Vec<u8>,
    /// Whether this capability can be delegated to others
    pub delegatable: bool,
    /// Current delegation depth (0 = original capability)
    pub delegation_depth: u8,
}

/// Journal layer capability token representation
///
/// Separates authentication (who) from authorization (what) for ledger operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalCapabilityToken {
    /// Unique identifier for this capability token
    pub id: CapabilityId,
    /// The authenticated device that has been granted this capability
    pub authenticated_device: crate::identifiers::DeviceId,
    /// List of permissions granted by this capability
    pub granted_permissions: Vec<Permission>,
    /// Chain of delegation showing how this capability was derived
    pub delegation_chain: Vec<CapabilityId>,
    /// Cryptographic signature validating this capability
    pub signature: Vec<u8>,
    /// Unix timestamp when this capability was issued
    pub issued_at: u64,
    /// Optional Unix timestamp when this capability expires
    pub expires_at: Option<u64>,
}

impl IntoAuthorizationToken for CapabilityToken {
    fn into_authorization_token(
        self,
        issuer: crate::identifiers::DeviceId,
        signature: Vec<u8>,
    ) -> AuthorizationCapabilityToken {
        let expires_at = match &self.expiration {
            CapabilityExpiration::Never => None,
            CapabilityExpiration::Timestamp(ts) => Some(*ts),
            CapabilityExpiration::Duration(duration) => Some(self.created_at + duration),
            CapabilityExpiration::SessionEnd(_) => None, // Requires session context
        };

        AuthorizationCapabilityToken {
            id: self.id,
            subject: self.subject,
            resource: self.resource,
            permissions: self.permissions,
            scope: self.scope,
            issued_at: self.created_at,
            expires_at,
            issuer,
            signature,
            delegatable: true,   // Default - can be customized
            delegation_depth: 5, // Default - can be customized
        }
    }
}

impl IntoJournalToken for CapabilityToken {
    fn into_journal_token(
        self,
        authenticated_device: crate::identifiers::DeviceId,
        delegation_chain: Vec<CapabilityId>,
        signature: Vec<u8>,
    ) -> JournalCapabilityToken {
        let expires_at = match &self.expiration {
            CapabilityExpiration::Never => None,
            CapabilityExpiration::Timestamp(ts) => Some(*ts),
            CapabilityExpiration::Duration(duration) => Some(self.created_at + duration),
            CapabilityExpiration::SessionEnd(_) => None, // Requires session context
        };

        JournalCapabilityToken {
            id: self.id,
            authenticated_device,
            granted_permissions: self.permissions,
            delegation_chain,
            signature,
            issued_at: self.created_at,
            expires_at,
        }
    }
}

impl CapabilityToken {
    /// Create a signed authorization token from this lightweight token
    ///
    /// # Example
    /// ```ignore
    /// let canonical_token = CapabilityToken::new(device_id, resource, permissions, scope, expiration);
    /// let auth_token = canonical_token.authorize(issuer_device_id, signature);
    /// ```
    pub fn authorize(
        self,
        issuer: crate::identifiers::DeviceId,
        signature: Vec<u8>,
    ) -> AuthorizationCapabilityToken {
        self.into_authorization_token(issuer, signature)
    }

    /// Create a journal event token for ledger operations
    ///
    /// # Example  
    /// ```ignore
    /// let canonical_token = CapabilityToken::new(device_id, resource, permissions, scope, expiration);
    /// let journal_token = canonical_token.for_journal(authenticated_device, delegation_chain, signature);
    /// ```
    pub fn for_journal(
        self,
        authenticated_device: crate::identifiers::DeviceId,
        delegation_chain: Vec<CapabilityId>,
        signature: Vec<u8>,
    ) -> JournalCapabilityToken {
        self.into_journal_token(authenticated_device, delegation_chain, signature)
    }
}

// =============================================================================
// Permission Mapping Support
// =============================================================================

impl Permission {
    /// Map canonical permissions to authorization actions
    ///
    /// This provides a clear mapping between the generic permission model
    /// and the authorization layer's action-based model.
    pub fn to_authorization_actions(&self) -> Vec<String> {
        match self {
            Permission::StorageRead => vec!["Read".to_string()],
            Permission::StorageWrite => vec!["Read".to_string(), "Write".to_string()],
            Permission::StorageDelete => vec![
                "Read".to_string(),
                "Write".to_string(),
                "Delete".to_string(),
            ],
            Permission::CommunicationSend => vec!["Execute".to_string()],
            Permission::CommunicationReceive => vec!["Read".to_string()],
            Permission::ProtocolExecute => vec!["Execute".to_string()],
            Permission::ProtocolModify => vec!["Execute".to_string(), "Write".to_string()],
            Permission::RecoveryAccess => vec!["Admin".to_string()],
            Permission::Admin => vec!["Admin".to_string()],
            Permission::Custom(perm) => vec![format!("Custom:{}", perm)],
        }
    }

    /// Map canonical permissions to journal domain operations
    ///
    /// This provides mapping to the journal layer's domain-specific permission model.
    pub fn to_journal_operations(&self) -> Vec<String> {
        match self {
            Permission::StorageRead => vec!["Storage:Read".to_string()],
            Permission::StorageWrite => vec!["Storage:Write".to_string()],
            Permission::StorageDelete => vec!["Storage:Delete".to_string()],
            Permission::CommunicationSend => vec!["Communication:Send".to_string()],
            Permission::CommunicationReceive => vec!["Communication:Receive".to_string()],
            Permission::ProtocolExecute => vec!["Protocol:Execute".to_string()],
            Permission::ProtocolModify => vec!["Protocol:Modify".to_string()],
            Permission::RecoveryAccess => vec!["Recovery:Access".to_string()],
            Permission::Admin => vec!["Admin".to_string()],
            Permission::Custom(perm) => vec![format!("Custom:{}", perm)],
        }
    }
}
