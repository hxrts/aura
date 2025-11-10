//! Tree Coordination Effects Interface
//!
//! This module defines the pure effect interface for tree operation coordination,
//! including validation, synchronization, and multi-device session management.

use async_trait::async_trait;
use aura_core::{AttestedOp, AuraError, DeviceId, Hash32, LeafNode, NodeIndex, TreeOpKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use uuid::Uuid;

/// Session identifier for tree coordination sessions
pub type SessionId = Uuid;

/// Role of a device in a tree coordination session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionRole {
    /// Device that can approve tree operations
    Approver,
    /// Device that observes but cannot approve
    Observer,
    /// Device that initiates the session
    Initiator,
}

/// Tree coordination operation errors
#[derive(Debug, thiserror::Error)]
pub enum CoordinationError {
    /// Session not found
    #[error("Session not found: {session_id}")]
    SessionNotFound { session_id: SessionId },

    /// Invalid session state for operation
    #[error("Invalid session state: {reason}")]
    InvalidSessionState { reason: String },

    /// Validation failed
    #[error("Operation validation failed: {reason}")]
    ValidationFailed { reason: String },

    /// Synchronization failed
    #[error("Tree synchronization failed: {reason}")]
    SyncFailed { reason: String },

    /// Session timeout
    #[error("Session timed out: {session_id}")]
    SessionTimeout { session_id: SessionId },

    /// Insufficient approvals
    #[error("Insufficient approvals: got {got}, needed {needed}")]
    InsufficientApprovals { got: usize, needed: usize },

    /// Participant unavailable
    #[error("Participant unavailable: {device_id}")]
    ParticipantUnavailable { device_id: DeviceId },

    /// Session failed
    #[error("Session failed: {reason}")]
    SessionFailed { reason: String },

    /// Protocol error
    #[error("Protocol error: {message}")]
    ProtocolError { message: String },
}

// Note: SessionError removed - using AuraError instead
// TODO: If session_manager is implemented later, add appropriate error conversion

/// Result of operation validation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationResult {
    /// Operation is valid and can proceed
    Valid,
    /// Operation has issues but can be fixed
    Warning { issues: Vec<String> },
    /// Operation is invalid and must be rejected
    Invalid { reason: String },
}

/// Status of approval collection
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    /// Waiting for more approvals
    Pending {
        received: usize,
        required: usize,
        missing_from: BTreeSet<DeviceId>,
    },
    /// Sufficient approvals received
    Approved,
    /// Operation was rejected
    Rejected {
        reason: String,
        rejected_by: BTreeSet<DeviceId>,
    },
    /// Approval process timed out
    TimedOut,
}

/// Progress of tree synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgress {
    /// Current synchronization phase
    pub phase: SyncPhase,
    /// Number of peers contacted
    pub peers_contacted: usize,
    /// Number of operations synchronized
    pub operations_synced: usize,
    /// Estimated completion time (milliseconds)
    pub estimated_completion_ms: u64,
}

/// Phases of tree synchronization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncPhase {
    /// Discovering available peers
    Discovery,
    /// Comparing tree states
    Comparison,
    /// Transferring operations
    Transfer,
    /// Validating received operations
    Validation,
    /// Applying operations to local tree
    Application,
    /// Synchronization complete
    Complete,
}

/// Information about a tree coordination session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier
    pub session_id: SessionId,
    /// Participants in the session
    pub participants: Vec<DeviceId>,
    /// The tree operation being coordinated
    pub operation: TreeOpKind,
    /// Current session status
    pub status: SessionStatus,
    /// When the session was created
    pub created_at: u64,
    /// Session timeout (if any)
    pub timeout_at: Option<u64>,
}

/// Status of a coordination session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionStatus {
    /// Session is active and collecting approvals
    Active,
    /// Waiting for operation validation
    Validating,
    /// Waiting for participant responses
    WaitingForApprovals,
    /// Session completed successfully
    Completed,
    /// Session was cancelled
    Cancelled,
    /// Session timed out
    TimedOut,
    /// Session failed due to validation issues
    Failed { reason: String },
}

/// Reason for closing a session
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseReason {
    /// Operation completed successfully
    Completed,
    /// Session was cancelled by user
    UserCancelled,
    /// Session timed out
    Timeout,
    /// Validation failed
    ValidationFailed,
    /// Insufficient approvals
    InsufficientApprovals,
    /// Network or system error
    SystemError { reason: String },
}

/// Validation context for tree operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationContext {
    /// Current tree state
    pub current_epoch: u64,
    /// Device requesting the operation
    pub requesting_device: DeviceId,
    /// Session context (if applicable)
    pub session_id: Option<SessionId>,
    /// Additional validation metadata
    pub metadata: BTreeMap<String, String>,
}

/// Tree digest for efficient synchronization
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDigest {
    /// Range of epochs covered
    pub epoch_range: Range<u64>,
    /// Hash of operations in this range
    pub operations_hash: Hash32,
    /// Number of operations in this range
    pub operation_count: u64,
    /// Tree state hash at end of range
    pub state_hash: Hash32,
}

/// Result of tree reconciliation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconcileResult {
    /// Operations that were successfully applied
    pub applied: Vec<Hash32>,
    /// Operations that were rejected
    pub rejected: Vec<(Hash32, String)>,
    /// Operations that need additional validation
    pub pending: Vec<Hash32>,
    /// Final tree state after reconciliation
    pub final_state_hash: Hash32,
}

/// Vote on an operation approval
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalVote {
    /// Device that cast the vote
    pub device_id: DeviceId,
    /// Vote decision
    pub decision: VoteDecision,
    /// Reasoning for the vote
    pub reason: Option<String>,
    /// When the vote was cast
    pub timestamp: u64,
}

/// Decision in an approval vote
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoteDecision {
    /// Approve the operation
    Approve,
    /// Reject the operation
    Reject,
    /// Abstain from voting
    Abstain,
}

/// Tree coordination effects interface
///
/// This trait provides operations for coordinating tree modifications across
/// multiple devices, including validation, approval workflows, and synchronization.
#[async_trait]
pub trait TreeCoordinationEffects: Send + Sync {
    // === Session Lifecycle ===

    /// Initiate a new tree update session with specified participants
    async fn initiate_tree_update_session(
        &self,
        participants: Vec<DeviceId>,
        operation: TreeOpKind,
    ) -> Result<SessionId, CoordinationError>;

    /// Join an existing tree update session
    async fn join_tree_update_session(
        &self,
        session_id: SessionId,
    ) -> Result<SessionRole, CoordinationError>;

    /// Get information about an active session
    async fn get_session_info(
        &self,
        session_id: SessionId,
    ) -> Result<SessionInfo, CoordinationError>;

    /// Get all active tree coordination sessions
    async fn get_active_tree_sessions(&self) -> Result<Vec<SessionInfo>, CoordinationError>;

    /// Close a tree coordination session
    async fn close_tree_session(
        &self,
        session_id: SessionId,
        reason: CloseReason,
    ) -> Result<(), CoordinationError>;

    // === Operation Validation ===

    /// Validate a tree operation for correctness and policy compliance
    async fn validate_tree_operation(
        &self,
        operation: &TreeOpKind,
        context: &ValidationContext,
    ) -> Result<ValidationResult, CoordinationError>;

    /// Validate an attested operation's cryptographic integrity
    async fn validate_attested_operation(
        &self,
        attested_op: &AttestedOp,
    ) -> Result<ValidationResult, CoordinationError>;

    /// Check if a device has permission to perform an operation
    async fn check_operation_permission(
        &self,
        device_id: DeviceId,
        operation: &TreeOpKind,
    ) -> Result<bool, CoordinationError>;

    // === Approval Coordination ===

    /// Coordinate approval collection for a tree operation
    async fn coordinate_operation_approval(
        &self,
        session_id: SessionId,
        operation: &TreeOpKind,
    ) -> Result<ApprovalStatus, CoordinationError>;

    /// Submit an approval vote for a tree operation
    async fn submit_approval_vote(
        &self,
        session_id: SessionId,
        vote: ApprovalVote,
    ) -> Result<(), CoordinationError>;

    /// Get current approval status for a session
    async fn get_approval_status(
        &self,
        session_id: SessionId,
    ) -> Result<ApprovalStatus, CoordinationError>;

    // === Synchronization ===

    /// Synchronize tree state with specified peers
    async fn sync_tree_with_peers(
        &self,
        peer_ids: Vec<DeviceId>,
        target_epoch: Option<u64>,
    ) -> Result<SyncProgress, CoordinationError>;

    /// Request tree operations delta from a peer
    async fn request_tree_delta(
        &self,
        peer_id: DeviceId,
        from_epoch: u64,
        to_epoch: u64,
    ) -> Result<Vec<AttestedOp>, CoordinationError>;

    /// Compute a digest of local tree state for synchronization
    async fn compute_tree_digest(
        &self,
        epoch_range: Range<u64>,
    ) -> Result<TreeDigest, CoordinationError>;

    /// Compare local tree digest with a remote digest
    async fn compare_tree_digests(
        &self,
        local_digest: &TreeDigest,
        remote_digest: &TreeDigest,
    ) -> Result<Vec<Range<u64>>, CoordinationError>;

    /// Reconcile received operations with local tree state
    async fn reconcile_tree_operations(
        &self,
        operations: Vec<AttestedOp>,
    ) -> Result<ReconcileResult, CoordinationError>;

    // === Coordination Helpers ===

    /// Get the required approval threshold for an operation
    async fn get_approval_threshold(
        &self,
        operation: &TreeOpKind,
    ) -> Result<usize, CoordinationError>;

    /// Get devices that should participate in approving an operation
    async fn get_approval_participants(
        &self,
        operation: &TreeOpKind,
    ) -> Result<Vec<DeviceId>, CoordinationError>;

    /// Check if all required participants are available
    async fn check_participant_availability(
        &self,
        participants: &[DeviceId],
    ) -> Result<Vec<DeviceId>, CoordinationError>;

    /// Estimate the time required to complete a coordination session
    async fn estimate_completion_time(
        &self,
        operation: &TreeOpKind,
        participants: &[DeviceId],
    ) -> Result<u64, CoordinationError>;
}

/// Events that can occur during tree coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinationEvent {
    /// A new session was created
    SessionCreated {
        session_id: SessionId,
        operation: TreeOpKind,
        participants: Vec<DeviceId>,
    },
    /// A participant joined a session
    ParticipantJoined {
        session_id: SessionId,
        device_id: DeviceId,
        role: SessionRole,
    },
    /// An approval vote was submitted
    VoteSubmitted {
        session_id: SessionId,
        vote: ApprovalVote,
    },
    /// Operation validation completed
    ValidationCompleted {
        session_id: SessionId,
        result: ValidationResult,
    },
    /// Session status changed
    StatusChanged {
        session_id: SessionId,
        old_status: SessionStatus,
        new_status: SessionStatus,
    },
    /// Session was closed
    SessionClosed {
        session_id: SessionId,
        reason: CloseReason,
    },
    /// Synchronization progress update
    SyncProgress {
        session_id: Option<SessionId>,
        progress: SyncProgress,
    },
}

/// Configuration for tree coordination behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationConfig {
    /// Default session timeout in milliseconds
    pub session_timeout_ms: u64,
    /// Maximum number of concurrent sessions
    pub max_concurrent_sessions: usize,
    /// Default approval threshold for operations
    pub default_approval_threshold: usize,
    /// Enable automatic synchronization after operations
    pub auto_sync_enabled: bool,
    /// Synchronization timeout in milliseconds
    pub sync_timeout_ms: u64,
    /// Maximum number of operations in a single sync batch
    pub max_sync_batch_size: usize,
}

impl Default for CoordinationConfig {
    fn default() -> Self {
        Self {
            session_timeout_ms: 300_000, // 5 minutes
            max_concurrent_sessions: 10,
            default_approval_threshold: 2, // Majority for 2-of-3
            auto_sync_enabled: true,
            sync_timeout_ms: 60_000, // 1 minute
            max_sync_batch_size: 100,
        }
    }
}
