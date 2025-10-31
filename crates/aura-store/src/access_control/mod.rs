//! Access Control Domain
//!
//! This domain implements capability-based access control (CapBAC) for storage:
//! - **Capabilities**: Cryptographic tokens granting specific permissions
//! - **Verification**: Checking capabilities and enforcing permission scopes
//! - **Delegation**: Passing capabilities to other devices with restrictions
//!
//! # Capability-Based Security Model
//!
//! Rather than role-based or ACL-based access control, Aura uses capabilities:
//! - **Capability Token**: Cryptographic proof of a specific permission
//! - **Scopes**: Capabilities scoped to resources (objects, directories, account)
//! - **Operations**: Specific actions granted (read, write, append, etc)
//! - **Constraints**: Time limits, device restrictions, delegation chains
//!
//! # Key Components
//!
//! - **CapabilityToken**: Represents a single permission credential with:
//!   - `authenticated_device`: Device that holds the capability
//!   - `granted_permissions`: Set of operations allowed (read, write, delete)
//!   - `delegation_chain`: History of how capability was delegated
//!   - `signature`: Cryptographic proof from threshold key
//!   - Expiration and validity timestamps
//!
//! - **CapabilityManager**: Maintains capability state:
//!   - `grant_capability()`: Issue new capability to device
//!   - `revoke_capability()`: Invalidate a capability
//!   - `record_delegation()`: Track capability delegation chains
//!   - `has_operation()`: Check if operation is permitted
//!
//! - **CapabilityChecker**: Verifies capabilities before operations:
//!   - `verify_access()`: Check device can perform action on resource
//!   - `can_perform_operation()`: Validate specific operation is allowed
//!   - `validate_signature()`: Verify capability cryptographic proof
//!   - `resource_matches()`: Check scope constraints
//!
//! # Access Control Flow
//!
//! ```text
//! Device Request (read/write object)
//!   ↓
//! CapabilityChecker.verify_access()
//!   ↓
//! Check device has valid capability token
//!   ↓
//! Validate capability signature (threshold key)
//!   ↓
//! Check resource scope matches request
//!   ↓
//! Verify operation is in granted permissions
//!   ↓
//! If all valid: proceed to storage domain
//! If invalid: return CapabilityError
//! ```
//!
//! # Security Properties
//!
//! - **No Ambient Authority**: Every operation requires explicit capability
//! - **Principle of Least Privilege**: Capabilities can be narrowly scoped
//! - **Revocability**: Capabilities can be revoked immediately
//! - **Delegation**: Capabilities can be delegated with additional constraints
//! - **Threshold Signed**: Capabilities require M-of-N device agreement
//!
//! # Integration Points
//!
//! - **Manifest Domain**: Defines capabilities and permissions per object
//! - **Storage Domain**: Only processes operations with valid capabilities
//! - **Journal Domain**: Capability grants/revokes are ledger events
//! - **Agent Domain**: Device-specific capability storage and lifecycle

pub mod capability;
pub mod capability_checker;

pub use aura_types::CapabilityError;
pub use capability::CapabilityManager;
pub use capability_checker::CapabilityChecker;
// Re-export CapabilityToken from authorization crate
pub use aura_authorization::CapabilityToken;
