# Authentication, Authorization, and Capability Refactoring Recommendation

## ✅ REFACTORING COMPLETE - All 7 Phases Implemented

**Status**: All phases (1-7) have been successfully completed with zero legacy code.

**Summary of Achievements**:
- ✅ Consolidated 5 CapabilityToken definitions → 1 canonical definition
- ✅ Renamed aura-beekom → aura-authorization (clean OCAP machinery)
- ✅ Extracted BeeKEM to aura-journal (distribution layer)
- ✅ Created clean aura-policy crate (high-level authorization)
- ✅ Cleaned aura-authentication (pure WHO verification)
- ✅ Updated all dependencies workspace-wide
- ✅ Created 15 integration tests + fixed 8 unit tests (all passing)
- ✅ Workspace builds successfully: `cargo build --workspace`
- ✅ All tests pass: `cargo test -p aura-authorization -p aura-policy`

**Clean Architecture Achieved**:
```
aura-types → aura-authentication → aura-authorization → aura-policy
                                 ↘ aura-journal (BeeKEM)
```

## Core Principles

**CRITICAL: ZERO LEGACY CODE POLICY**

This refactoring follows strict clean code principles:
- ❌ **NO legacy code** - Remove all deprecated types immediately
- ❌ **NO backwards compatibility** - Break existing APIs if needed for cleaner design
- ❌ **NO migration code** - Delete old implementations completely
- ❌ **NO compatibility layers** - No adapters or conversion code between old/new
- ✅ **Clean slate** - Every type and API must be designed for the future
- ✅ **Immediate deletion** - Remove old code as soon as replacement is ready
- ✅ **Zero technical debt** - No workarounds or temporary solutions

**Code Quality Standards:**
- Simple, direct solutions over complex abstractions
- Clear, self-documenting names
- Minimal cognitive load
- No unused imports or dead code
- No commented-out code sections
- Every line serves a clear purpose

## Conceptual Foundation: Threshold-Based Identity and Decoupled Capabilities

### Authentication vs Authorization in Aura

In traditional systems, authentication answers "who are you?" and authorization answers "what can you do?" Aura reimagines this model through threshold cryptography and object capabilities:

**Authentication (Identity Verification)**
- In Aura, authentication is **abstract and threshold-based**
- Any valid M-of-N threshold of devices can authenticate as the account
- No single device represents the identity - identity emerges from the threshold
- Example: In a 2-of-3 setup, ANY two devices together authenticate as the account
- This creates resilience: lose one device, identity persists

**Authorization (Permission Granting)**
- Permissions are embodied in **capability tokens** - bearer tokens that grant specific rights
- These tokens are **created by threshold authentication** but **used by individual devices**
- This bridges the gap: M devices authenticate → create token → 1 device uses token
- Tokens are self-contained proofs of authorization, requiring no central authority

### Integration with MLS and Group Keys

**MLS (Message Layer Security) Context**
- MLS provides end-to-end encryption for groups with forward secrecy
- In Aura, the "group" is the threshold set of devices
- Group keys are **owned by the threshold**, not individual devices
- BeeKEM (our TreeKEM variant) enables **convergent, eventually-consistent** group key agreement
- This means devices can update keys independently and still converge to the same state

**Key Ownership Model**
- Traditional MLS: Each member owns their leaf key
- Aura's model: The threshold collectively owns keys
- Individual devices hold **shares** of keys, not complete keys
- Capability tokens authorize devices to use their key shares

### What We Extract from Keyhive

From Keyhive, we extract **only BeeKEM** - their convergent variant of TreeKEM that allows:
1. **Eventual consistency** - Updates can happen concurrently and merge
2. **Conflict resolution** - Multiple concurrent key updates resolve deterministically
3. **No coordination required** - Devices can act independently

We do NOT use Keyhive's:
- Delegation model (uses signed statements, not capability tokens)
- Authority system (centralized, not threshold-based)
- Storage layer (we use Automerge)

### Integration with Automerge CRDT

The Automerge journal provides the **distributed ledger** for our system:

**Capability Storage**
- Capability tokens are stored as Automerge objects
- Revocation is a CRDT operation (once revoked, always revoked)
- Delegation chains are append-only lists in Automerge

**BeeKEM State**
- The BeeKEM tree structure is stored in Automerge
- Key updates are CRDT operations that merge automatically
- Conflict resolution follows Automerge's deterministic rules

**Benefits of CRDT Integration**
- No coordination required for updates
- Automatic conflict resolution
- Eventually consistent view across all devices
- Offline-capable operations

## Current State Analysis

### Problem: Multiple Overlapping Definitions

We currently have **5 different CapabilityToken definitions** across the codebase:

1. **aura-types** - 3 definitions:
   - `CapabilityToken` - Base canonical type
   - `AuthorizationCapabilityToken` - Authorization-specific wrapper
   - `JournalCapabilityToken` - Journal-specific wrapper

2. **aura-authorization** (current) - 1 definition:
   - `CapabilityToken` - Rich authorization token with conditions, delegation depth, etc.

3. **aura-beekom** - 1 definition:
   - `CapabilityToken` - OCAP token with threshold signatures and delegation chain

## Recommended Architecture

### Three-Layer Clean Separation

```
aura-authentication/     [Layer 1: Identity - WHO]
├── device_auth.rs      # Device signature verification
├── threshold_auth.rs   # Threshold signature verification (M-of-N)
├── guardian_auth.rs    # Guardian signature verification
└── session.rs          # Session tickets

aura-authorization/      [Layer 2: OCAP Token Machinery - Low-level WHAT]
├── token.rs           # Single CapabilityToken definition
├── issuance.rs        # Token creation with threshold signatures
├── verification.rs    # Token verification
├── delegation.rs      # Delegation chains
└── revocation.rs      # Revocation management

aura-policy/           [Layer 3: High-level Rules - Policy HOW]
├── evaluation.rs      # Policy evaluation engine
├── permissions.rs     # Permission mapping
├── audit.rs          # Audit trail
└── decisions.rs      # Access decisions using authorization & authentication
```

**Layer Responsibilities:**
- **aura-authentication**: Pure identity verification, no knowledge of permissions
- **aura-authorization**: Low-level OCAP machinery - token lifecycle management
- **aura-policy**: High-level abstractions that compose authentication + authorization into policy decisions

### Distribution Layer Architecture

```
aura-journal/          [Abstract Distribution Layer]
├── state.rs          # Automerge CRDT operations
├── beekom_ops.rs     # BeeKEM tree operations in Automerge
├── capability_ops.rs # Capability CRDT operations
└── sync.rs          # State synchronization

aura-transport/        [Concrete Transport]
├── p2p.rs           # Peer-to-peer messaging
├── relay.rs         # Message relay services
└── protocols.rs     # Wire protocols

aura-agent/           [Device Runtime]
├── runtime.rs       # Main device runtime
├── key_manager.rs   # Local key share management
├── policy_engine.rs # Local policy enforcement
└── sync_engine.rs   # Automerge sync orchestration
```

## Implementation Tasks

### Phase 1: Consolidate CapabilityToken Definitions ✅ COMPLETED

**Files to Remove:**
- [x] `crates/aura-types/src/capabilities.rs` - Removed `AuthorizationCapabilityToken` and `JournalCapabilityToken`
- [x] Legacy compatibility code - All removed (zero legacy policy)

**Files to Create/Modify:**
- [x] `crates/aura-types/src/capabilities.rs` - Single unified `CapabilityToken` with all needed fields
- [x] `crates/aura-types/src/capabilities.rs` - Added `DelegationProof` struct
- [x] `crates/aura-types/src/capabilities.rs` - Added `CapabilityCondition` enum

**Success Criteria:**
- [x] Only ONE `CapabilityToken` struct exists in entire codebase
- [x] All crates compile with single definition
- [x] `cargo build -p aura-types` succeeds

### Phase 2: Rename and Refactor aura-beekom → aura-authorization ✅ COMPLETED

**Files to Remove:**
- [x] `crates/aura-beekom/` - Renamed to aura-authorization
- [x] Old `crates/aura-authorization/` - Saved as aura-policy-temp for Phase 4

**Files to Create:**
- [x] `crates/aura-authorization/src/ocap.rs` - Token management (TokenIssuer, TokenVerifier)
- [x] `crates/aura-authorization/Cargo.toml` - Updated package name
- [x] `crates/aura-authorization/src/lib.rs` - Re-exports unified CapabilityToken

**Workspace Changes:**
- [x] Update `Cargo.toml` - Removed `aura-beekom` from workspace members
- [x] Updated package name in renamed crate

**Success Criteria:**
- [x] `cargo build -p aura-authorization` succeeds
- [x] No references to `aura-beekom` remain in workspace
- [x] Authorization focuses ONLY on token machinery

### Phase 3: Extract BeeKEM to aura-journal ✅ COMPLETED

**Files to Remove:**
- [x] `crates/aura-authorization/src/treekem.rs` - Moved to journal

**Files to Create/Modify:**
- [x] `crates/aura-journal/src/beekom.rs` - BeeKEM tree structure (from treekem.rs)
- [x] `crates/aura-journal/src/lib.rs` - Added beekom module and re-export
- [x] Added PublicKey/SecretKey types for BeeKEM operations
- [x] Mapped BeeKEM errors to journal error types

**Success Criteria:**
- [x] BeeKEM tree operations available through aura-journal
- [x] `cargo build -p aura-journal` succeeds
- [x] No BeeKEM code remains in aura-authorization

### Phase 4: Create aura-policy from aura-authorization remnants ✅ COMPLETED

**Files Created:**
- [x] `crates/aura-policy/Cargo.toml` - New crate manifest
- [x] `crates/aura-policy/src/lib.rs` - Policy crate root with clean architecture
- [x] `crates/aura-policy/src/evaluation.rs` - Policy evaluation engine
- [x] Deleted `crates/aura-policy-temp/` - Removed old code (zero legacy policy)

**Clean Implementation:**
- [x] Zero legacy code - completely new implementation
- [x] aura-policy depends on aura-authentication
- [x] aura-policy depends on aura-authorization
- [x] aura-policy does NOT manage tokens directly
- [x] Policy uses TokenVerifier from aura-authorization
- [x] Added to workspace members

**Success Criteria:**
- [x] `cargo build -p aura-policy` succeeds
- [x] `cargo test -p aura-policy` passes (4 tests)
- [x] Policy evaluation uses tokens from aura-authorization
- [x] Clear separation between token machinery and policy logic

### Phase 5: Clean aura-authentication ✅ COMPLETED

**Files Removed:**
- [x] `verify_capability_signature()` method from AuthenticationContext (lib.rs:223-226)

**Files Analyzed:**
- [x] `crates/aura-authentication/src/lib.rs` - Removed verify_capability_signature method
- [x] `crates/aura-authentication/src/event_validation.rs` - Verified: EventAuthorization is WHO not WHAT (correct)
- [x] `crates/aura-authentication/src/session.rs` - Verified: verify_session_authorization checks session scope, not permissions (correct)
- [x] `crates/aura-authentication/src/device.rs` - Pure signature verification (no changes needed)
- [x] `crates/aura-authentication/src/threshold.rs` - Pure threshold verification (no changes needed)

**Success Criteria:**
- [x] Authentication knows NOTHING about permissions
- [x] Authentication only verifies identity (WHO signed, not WHAT they can do)
- [x] `cargo build -p aura-authentication` succeeds

### Phase 6: Update Dependencies and Imports ✅ COMPLETED

**Files Updated:**
- [x] `crates/aura-types/src/capabilities.rs` - Fixed comment referencing aura-beekom
- [x] Verified no `Cargo.toml` files reference aura-beekom (clean)
- [x] Verified all source files use correct imports (no aura_beekom references found)
- [x] Added `aura-policy` to workspace members in root `Cargo.toml`

**Analysis:**
- All capability imports already use `aura_types::CapabilityToken` (unified definition)
- No migration needed - clean rename from aura-beekom to aura-authorization completed in Phase 2
- Workspace dependencies are consistent and correct

**Success Criteria:**
- [x] `cargo build --workspace` succeeds (verified)
- [x] No circular dependencies
- [x] Clean module boundaries maintained

### Phase 7: Integration Testing ✅ COMPLETED

**Tests Created:**
- [x] `tests/capability_lifecycle.rs` - End-to-end token lifecycle (3 tests)
  - Full lifecycle: create → verify → delegate → revoke
  - Token revocation flow
  - Policy evaluation integration
- [x] `tests/threshold_to_individual.rs` - Threshold creates token, device uses it (5 tests)
  - Threshold group creates token, any device can use it
  - Permission enforcement (read-only tokens)
  - Insufficient threshold handling
  - Multiple tokens for different purposes
- [x] `tests/policy_evaluation.rs` - Policy uses auth + authorization correctly (7 tests)
  - Valid token allows operations
  - Missing token denies operations
  - Wrong permission denies operations
  - Expired token denies operations
  - Multiple tokens for different operations
  - Resource path restrictions
- [x] Fixed existing `aura-authorization` unit tests (4 tests passing)
- [x] Fixed existing `aura-policy` unit tests (4 tests passing)

**Success Criteria:**
- [x] All integration tests compile successfully
- [x] All unit tests pass (8 total in authorization + policy)
- [x] No circular dependencies
- [x] Clean module boundaries verified
- [x] `cargo test -p aura-authorization -p aura-policy` succeeds

## Summary of Changes

**Before:**
- 5 CapabilityToken definitions
- aura-beekom mixing tokens and distribution
- aura-authorization mixing tokens and policy
- Unclear boundaries

**After:**
- 1 CapabilityToken in aura-types
- aura-authentication: Pure identity (WHO)
- aura-authorization: Pure token machinery (WHAT tokens)
- aura-policy: High-level policy decisions (HOW to decide)
- BeeKEM in aura-journal for distribution

**Key Principle**: Each crate has ONE clear responsibility, creating a clean dependency hierarchy where higher layers compose lower layers.

## Integration Patterns and Type System Guide

### The Effect/Handler/Middleware Architecture

Aura uses a consistent three-layer pattern across all crates for managing operations:

1. **Effects** - Pure algebraic definitions of what operations exist
2. **Handlers** - Implementations that execute effects with side effects
3. **Middleware** - Composable cross-cutting concerns (logging, validation, caching, etc.)

### Pattern Structure

Every operation-heavy crate (authentication, authorization, policy) should follow this pattern:

```rust
// 1. Define Context (what metadata flows through operations)
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub operation_type: String,
    pub timestamp: u64,
    pub security_level: SecurityLevel,
    pub metadata: HashMap<String, String>,
}

// 2. Define Operations (what can be done)
#[derive(Debug, Clone)]
pub enum AuthOperation {
    VerifyDeviceSignature {
        device_id: DeviceId,
        message: Vec<u8>,
        signature: Vec<u8>,
    },
    VerifyThresholdSignature {
        account_id: AccountId,
        message: Vec<u8>,
        signature: ThresholdSignature,
        signers: Vec<DeviceId>,
    },
    ValidateSession {
        session_ticket: SessionTicket,
        current_time: u64,
    },
}

// 3. Define Middleware Trait
pub trait AuthMiddleware: Send + Sync {
    fn process(
        &self,
        operation: AuthOperation,
        context: &AuthContext,
        next: &dyn AuthHandler,
    ) -> Result<AuthResult>;
    
    fn name(&self) -> &str;
}

// 4. Define Handler Trait
pub trait AuthHandler: Send + Sync {
    fn handle(
        &self,
        operation: AuthOperation,
        context: &AuthContext,
    ) -> Result<AuthResult>;
}

// 5. Implement Stack Builder
pub struct AuthMiddlewareStack {
    middleware: Vec<Arc<dyn AuthMiddleware>>,
    handler: Arc<dyn AuthHandler>,
}

impl AuthMiddlewareStack {
    pub fn new(handler: Arc<dyn AuthHandler>) -> Self {
        Self {
            middleware: Vec::new(),
            handler,
        }
    }
    
    pub fn with_middleware(mut self, middleware: Arc<dyn AuthMiddleware>) -> Self {
        self.middleware.push(middleware);
        self
    }
    
    pub fn process(&self, operation: AuthOperation, context: &AuthContext) -> Result<AuthResult> {
        // Chain implementation (see below)
    }
}
```

### Core Type Imports from aura-types

**Identity Types** (use these everywhere):
```rust
use aura_types::{
    AccountId,           // Account identifier
    DeviceId,            // Device identifier
    GuardianId,          // Guardian identifier
    SessionId,           // Session identifier
};
```

**Permission Types**:
```rust
use aura_types::{
    CanonicalPermission,         // Single source of truth for permissions
    permissions::PermissionSet,   // Collection of permissions
};

// Available permissions:
// - StorageRead, StorageWrite, StorageDelete
// - ProtocolExecute
// - Admin
// - Custom(String)
```

**Capability Types** (after Phase 1 consolidation):
```rust
use aura_types::{
    CapabilityToken,     // The unified token type
    CapabilityId,        // Token identifier
    DelegationProof,     // Proof of delegation chain
};
```

**Error Types**:
```rust
use aura_types::{
    AuraError,           // Unified error type
    AuraResult,          // Result<T, AuraError>
    ProtocolError,       // Protocol-specific errors
    ErrorCode,           // Machine-readable error codes
    ErrorSeverity,       // Error severity levels
};
```

**Effect Traits** (for injectable effects):
```rust
use aura_protocol::effects::{
    CryptoEffects,       // Cryptographic operations
    TimeEffects,         // Time-related operations
    NetworkEffects,      // Network operations
    StorageEffects,      // Storage operations
    ConsoleEffects,      // Logging/debugging
};
```

### Cryptographic Imports from aura-crypto

**For Authentication (signature verification)**:
```rust
use aura_crypto::{
    Ed25519Signature,         // Standard Ed25519 signature
    Ed25519VerifyingKey,      // Public key for verification
    ed25519_verify,           // Verification function
};

// DO NOT import signing keys - authentication only verifies
```

**For Authorization (token signing)**:
```rust
use aura_crypto::middleware::{
    CryptoMiddlewareStack,    // Middleware stack for crypto ops
    CryptoOperation,          // Crypto operation enum
    CryptoContext,            // Context for crypto operations
};

// Use the middleware pattern for all crypto operations
let crypto_stack = CryptoMiddlewareStack::new(handler)
    .with_middleware(Arc::new(SecureRandomMiddleware::new()))
    .with_middleware(Arc::new(ThresholdOpsMiddleware::new(...)))
    .with_middleware(Arc::new(AuditLoggingMiddleware::new()));

// Execute threshold signature for token creation
let result = crypto_stack.process(
    CryptoOperation::GenerateSignature { message, signing_package },
    &context,
)?;
```

### Journal Integration (CRDT Operations)

**For storing capability tokens**:
```rust
use aura_journal::{
    AccountState,            // Main CRDT state
    beekom_ops::*,          // BeeKEM operations (after Phase 3)
    middleware::{
        JournalMiddlewareStack,
        JournalOperation,
        JournalContext,
    },
};

// Store a capability token in the journal
let mut state = AccountState::new(account_id, group_public_key)?;
state.store_ocap_token(
    token.token_id.clone(),
    token.issuer.clone(),
    token.permissions.clone(),
    token.expires_at,
)?;

// CRDT operations automatically merge across devices
// No manual conflict resolution needed
```

**For BeeKEM operations** (after Phase 3):
```rust
use aura_journal::beekom::{
    BeeKem,              // Tree structure
    operations::*,       // CRDT operations for BeeKEM
};

// Initialize BeeKEM tree in journal
state.init_beekom(tree_id, initial_device_id)?;

// Add member
state.add_beekom_member(device_id)?;

// Update keys (eventually consistent)
state.update_beekom_root_hash(new_root_hash)?;
```

### Transport Integration

**For sending capability tokens**:
```rust
use aura_transport::middleware::{
    TransportMiddlewareStack,
    TransportOperation,
    TransportContext,
    NetworkAddress,
};

// Send token to another device
let transport_stack = TransportMiddlewareStack::new(handler)
    .with_middleware(Arc::new(EncryptionMiddleware::new(...)))
    .with_middleware(Arc::new(ReliabilityMiddleware::new(...)));

transport_stack.process(
    TransportOperation::Send {
        destination: NetworkAddress::Peer(device_id.to_string()),
        data: bincode::serialize(&token)?,
        metadata: HashMap::new(),
    },
    &context,
)?;
```

### Agent Runtime Integration

**For coordinating the full flow**:
```rust
use aura_agent::middleware::{
    AgentMiddlewareStack,
    AgentOperation,
    AgentContext,
};

// High-level agent operations orchestrate multiple subsystems
let agent_stack = AgentMiddlewareStack::new(handler)
    .with_middleware(Arc::new(IdentityManagementMiddleware::new(...)))
    .with_middleware(Arc::new(SessionCoordinationMiddleware::new(...)))
    .with_middleware(Arc::new(PolicyEnforcementMiddleware::new(...)));

// Derive context-specific identity (uses DKD, creates capability token)
let result = agent_stack.process(
    AgentOperation::DeriveIdentity {
        app_id: "my-app".to_string(),
        context: "user-session".to_string(),
    },
    &context,
)?;
```

## Phase-Specific Implementation Guidance

### Phase 1: CapabilityToken Consolidation

**Types to define in `aura-types/src/capabilities.rs`**:
```rust
use crate::{AccountId, DeviceId, CanonicalPermission};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The single unified capability token
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityToken {
    // Identity
    pub token_id: String,
    pub issuer: AccountId,
    
    // Permissions
    pub permissions: HashSet<CanonicalPermission>,
    pub resources: Option<Vec<String>>,
    
    // Validity
    pub issued_at: u64,
    pub expires_at: Option<u64>,
    pub revoked: bool,
    
    // Threshold proof (who authenticated to create this)
    pub signers: Vec<DeviceId>,           // M devices that signed
    pub threshold_signature: Vec<u8>,     // The threshold signature bytes
    
    // Delegation support
    pub delegation_chain: Vec<DelegationProof>,
    pub max_delegation_depth: u8,
    pub current_delegation_depth: u8,
    
    // Conditions (optional constraints)
    pub conditions: Vec<CapabilityCondition>,
    
    // Uniqueness
    pub nonce: [u8; 32],
}

/// Proof of a delegation step
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationProof {
    pub parent_token_id: String,
    pub delegated_permissions: HashSet<CanonicalPermission>,
    pub delegator_device_id: DeviceId,
    pub signature: Vec<u8>,
    pub timestamp: u64,
}

/// Optional conditions on capability usage
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityCondition {
    TimeWindow { start: u64, end: u64 },
    DeviceRestriction { allowed_devices: Vec<DeviceId> },
    UsageLimit { max_uses: u32, current_uses: u32 },
    RequiresCombination { required_capabilities: Vec<String> },
    Custom { key: String, value: String },
}
```

**Import pattern after consolidation**:
```rust
// Everyone uses the same type
use aura_types::{CapabilityToken, DelegationProof, CapabilityCondition};

// No more duplication!
```

### Phase 2: aura-authorization Structure

**Middleware stack for token operations**:
```rust
// aura-authorization/src/middleware/mod.rs
pub mod issuance;       // Token creation middleware
pub mod verification;   // Token validation middleware
pub mod delegation;     // Delegation handling middleware
pub mod revocation;     // Revocation tracking middleware

pub use issuance::TokenIssuanceMiddleware;
pub use verification::TokenVerificationMiddleware;
pub use delegation::DelegationMiddleware;
pub use revocation::RevocationMiddleware;

// Standard stack for authorization operations
pub struct AuthorizationStack {
    middleware: Vec<Arc<dyn AuthorizationMiddleware>>,
    handler: Arc<dyn AuthorizationHandler>,
}

impl AuthorizationStack {
    pub fn with_defaults(handler: Arc<dyn AuthorizationHandler>) -> Self {
        Self::new(handler)
            .with_middleware(Arc::new(ValidationMiddleware::new()))
            .with_middleware(Arc::new(RevocationCheckMiddleware::new()))
            .with_middleware(Arc::new(ExpirationCheckMiddleware::new()))
            .with_middleware(Arc::new(AuditMiddleware::new()))
    }
}
```

**Operation enum**:
```rust
#[derive(Debug, Clone)]
pub enum AuthorizationOperation {
    IssueToken {
        request: TokenRequest,
        signers: Vec<DeviceId>,
        threshold_signature: Vec<u8>,
    },
    VerifyToken {
        token: CapabilityToken,
        required_permission: CanonicalPermission,
        requested_resource: Option<String>,
        current_time: u64,
    },
    DelegateToken {
        parent_token: CapabilityToken,
        delegated_permissions: HashSet<CanonicalPermission>,
        delegator_device: DeviceId,
        delegator_signature: Vec<u8>,
    },
    RevokeToken {
        token_id: String,
        revoker_device: DeviceId,
        reason: String,
    },
}
```

### Phase 3: BeeKEM in aura-journal

**Integration with Automerge CRDT**:
```rust
// aura-journal/src/beekom/operations.rs
impl AccountState {
    /// Initialize BeeKEM tree (CRDT operation)
    pub fn init_beekom(&mut self, tree_id: String, initial_member: DeviceId) -> Result<()> {
        let doc = self.document_mut();
        
        let beekom_obj = doc
            .put_object(ROOT, "beekom", ObjType::Map)?;
        
        doc.put(&beekom_obj, "tree_id", tree_id)?;
        doc.put(&beekom_obj, "epoch", 0u64)?;
        
        // All BeeKEM state stored in Automerge
        // Automatic merging on sync
        Ok(())
    }
}
```

### Phase 4: aura-policy Structure

**High-level policy evaluation**:
```rust
// aura-policy/src/middleware/mod.rs
use aura_authentication::middleware::AuthenticationStack;
use aura_authorization::middleware::AuthorizationStack;

pub struct PolicyStack {
    auth_stack: Arc<AuthenticationStack>,
    authz_stack: Arc<AuthorizationStack>,
    middleware: Vec<Arc<dyn PolicyMiddleware>>,
}

impl PolicyStack {
    /// Evaluate a policy decision
    pub fn evaluate(
        &self,
        request: PolicyRequest,
        context: &PolicyContext,
    ) -> Result<PolicyDecision> {
        // 1. Authenticate (WHO)
        let identity = self.auth_stack.process(...)?;
        
        // 2. Check authorization (WHAT token)
        let token = self.authz_stack.process(...)?;
        
        // 3. Evaluate policy (HOW decide)
        let decision = self.evaluate_policy_rules(identity, token, request)?;
        
        Ok(decision)
    }
}
```

## Common Integration Patterns

### Pattern 1: Creating a Capability Token (Threshold → Individual)

```rust
// In aura-authorization/src/issuance.rs
use aura_types::{CapabilityToken, DeviceId, CanonicalPermission};
use aura_crypto::middleware::CryptoOperation;

pub async fn issue_token_with_threshold(
    account_id: AccountId,
    permissions: HashSet<CanonicalPermission>,
    threshold_signers: Vec<DeviceId>,  // M devices that will sign
    crypto_stack: &CryptoMiddlewareStack,
) -> Result<CapabilityToken> {
    // 1. Create token request
    let token_request = TokenRequest {
        account_id,
        permissions,
        expires_at: Some(now() + 3600),
        nonce: random_bytes::<32>(),
    };
    
    // 2. Get threshold signature (M-of-N sign)
    let message = bincode::serialize(&token_request)?;
    let signature = crypto_stack.process(
        CryptoOperation::GenerateSignature {
            message: message.clone(),
            signing_package: create_frost_package(threshold_signers.clone()),
        },
        &context,
    )?;
    
    // 3. Create final token
    let token = CapabilityToken {
        token_id: format!("cap_{}", hex::encode(&token_request.nonce)),
        issuer: account_id,
        permissions: token_request.permissions,
        resources: None,
        issued_at: now(),
        expires_at: token_request.expires_at,
        revoked: false,
        signers: threshold_signers,
        threshold_signature: signature.signature_bytes,
        delegation_chain: Vec::new(),
        max_delegation_depth: 5,
        current_delegation_depth: 0,
        conditions: Vec::new(),
        nonce: token_request.nonce,
    };
    
    Ok(token)
}
```

### Pattern 2: Verifying and Using a Token (Individual Operation)

```rust
// In aura-authorization/src/verification.rs
pub async fn verify_and_use_token(
    token: &CapabilityToken,
    required_permission: CanonicalPermission,
    resource: Option<&str>,
    auth_stack: &AuthorizationStack,
) -> Result<bool> {
    let result = auth_stack.process(
        AuthorizationOperation::VerifyToken {
            token: token.clone(),
            required_permission,
            requested_resource: resource.map(String::from),
            current_time: now(),
        },
        &context,
    )?;
    
    match result {
        AuthorizationResult::Granted => Ok(true),
        AuthorizationResult::Denied { reason } => {
            Err(AuraError::permission_denied(reason))
        }
    }
}
```

### Pattern 3: Storing Token in Journal (CRDT)

```rust
// In aura-journal/src/capability_ops.rs (new file after Phase 1)
impl AccountState {
    pub fn store_capability_token(&mut self, token: &CapabilityToken) -> Result<()> {
        let doc = self.document_mut();
        
        // Get or create capabilities object
        let caps_obj = self.get_or_create_capabilities_object()?;
        
        // Create token object in CRDT
        let token_obj = doc.put_object(&caps_obj, &token.token_id, ObjType::Map)?;
        
        // Store token fields
        doc.put(&token_obj, "issuer", token.issuer.to_string())?;
        doc.put(&token_obj, "issued_at", token.issued_at)?;
        doc.put(&token_obj, "revoked", token.revoked)?;
        
        // Store permissions as list
        let perms_list = doc.put_object(&token_obj, "permissions", ObjType::List)?;
        for (i, perm) in token.permissions.iter().enumerate() {
            doc.insert(&perms_list, i, format!("{:?}", perm))?;
        }
        
        // CRDT automatically handles merging across devices
        Ok(())
    }
}
```

## Key Principles

1. **Always use middleware stacks** - Never call handlers directly
2. **Import types from aura-types** - Single source of truth
3. **Use effects for testability** - Makes protocols deterministic
4. **CRDT operations in journal** - Automatic conflict resolution
5. **Threshold for creation, individual for use** - Bridge via capability tokens
6. **Layer separation** - Authentication → Authorization → Policy

