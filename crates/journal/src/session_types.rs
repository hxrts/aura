//! Journal protocol lifecycle and group session types.

use aura_types::{AccountId, DeviceId, SessionId};
use protocol_core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolRehydration, ProtocolStep,
    },
    metadata::{ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Journal protocol error placeholder.
#[derive(Debug, thiserror::Error)]
pub enum JournalProtocolError {
    #[error("unsupported journal input: {0}")]
    Unsupported(&'static str),
}

/// Ledger empty typestate marker.
#[derive(Debug, Clone)]
pub struct LedgerInitialized;

impl SessionState for LedgerInitialized {
    const NAME: &'static str = "LedgerInitialized";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Journal protocol implementing the unified lifecycle trait.
#[derive(Debug, Clone)]
pub struct JournalProtocol {
    descriptor: ProtocolDescriptor,
    state: LedgerInitialized,
    finished: bool,
}

impl JournalProtocol {
    /// Create a new journal protocol instance.
    pub fn new(device_id: DeviceId, session_id: SessionId) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::Locking,
        )
        .with_priority(ProtocolPriority::Normal)
        .with_mode(ProtocolMode::Asynchronous);

        Self {
            descriptor,
            state: LedgerInitialized,
            finished: false,
        }
    }

    /// Convenience helper with generated session ID.
    #[allow(clippy::disallowed_methods)]
    pub fn new_ephemeral(device_id: DeviceId) -> Self {
        Self::new(device_id, SessionId::new())
    }
}

impl ProtocolLifecycle for JournalProtocol {
    type State = LedgerInitialized;
    type Output = ();
    type Error = JournalProtocolError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match input {
            ProtocolInput::LocalSignal { signal, .. } if signal == "finalize" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    None,
                    Ok(()),
                )
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl ProtocolRehydration for JournalProtocol {
    type Evidence = ();

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        _account_id: AccountId,
        _evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new(device_id, SessionId::new()))
    }
}

/// Legacy-compatible constructor.
pub fn new_journal_protocol(device_id: DeviceId) -> JournalProtocol {
    JournalProtocol::new_ephemeral(device_id)
}

/// Legacy-compatible rehydration helper.
pub fn rehydrate_journal_protocol(
    device_id: DeviceId,
    account_id: AccountId,
) -> Result<JournalProtocol, JournalProtocolError> {
    JournalProtocol::rehydrate(device_id, account_id, ())
}

// ========== Group Session Types ==========

/// Group membership session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembershipState {
    pub group_id: String,
    pub epoch: u64,
    pub members: Vec<String>,
}

impl SessionState for GroupMembershipState {
    const NAME: &'static str = "GroupMembership";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

/// Group messaging session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMessagingState {
    pub group_id: String,
    pub current_epoch: u64,
    pub message_count: u32,
    pub last_ratchet: u64,
}

impl SessionState for GroupMessagingState {
    const NAME: &'static str = "GroupMessaging";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

/// Group administration session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupAdminState {
    pub group_id: String,
    pub pending_operations: Vec<String>,
    pub admin_epoch: u64,
}

impl SessionState for GroupAdminState {
    const NAME: &'static str = "GroupAdmin";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

/// Group capability verification session state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupCapabilityVerification {
    pub group_id: String,
    pub subject_id: String,
    pub operation: String,
    pub verification_complete: bool,
}

impl SessionState for GroupCapabilityVerification {
    const NAME: &'static str = "GroupCapabilityVerification";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = true;
}

/// Group session protocol type
#[derive(Debug, Clone)]
pub struct GroupProtocol<T: SessionState> {
    descriptor: ProtocolDescriptor,
    state: T,
    finished: bool,
}

impl<T: SessionState> GroupProtocol<T> {
    /// Create a new group protocol instance
    pub fn new(device_id: DeviceId, session_id: SessionId, initial_state: T) -> Self {
        let descriptor = ProtocolDescriptor::new(
            Uuid::new_v4(),
            session_id,
            device_id,
            ProtocolType::Group,
        )
        .with_priority(ProtocolPriority::Normal)
        .with_mode(ProtocolMode::Synchronous);

        Self {
            descriptor,
            state: initial_state,
            finished: false,
        }
    }

    /// Get current session state
    pub fn state(&self) -> &T {
        &self.state
    }

    /// Update session state
    pub fn update_state(&mut self, new_state: T) {
        self.state = new_state;
    }
}

impl<T: SessionState> ProtocolLifecycle for GroupProtocol<T> {
    type State = T;
    type Output = ();
    type Error = JournalProtocolError;

    fn descriptor(&self) -> &ProtocolDescriptor {
        &self.descriptor
    }

    fn step(
        &mut self,
        input: ProtocolInput<'_>,
        _caps: &mut ProtocolCapabilities<'_>,
    ) -> ProtocolStep<Self::Output, Self::Error> {
        match input {
            ProtocolInput::LocalSignal { signal, .. } if signal == "finalize" => {
                self.finished = true;
                ProtocolStep::completed(
                    Vec::<ProtocolEffects>::new(),
                    None,
                    Ok(()),
                )
            }
            ProtocolInput::LocalSignal { signal, .. } if signal == "update" => {
                // Group state update logic would go here
                ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None)
            }
            _ => ProtocolStep::progress(Vec::<ProtocolEffects>::new(), None),
        }
    }

    fn is_final(&self) -> bool {
        self.finished
    }
}

impl<T: SessionState> ProtocolRehydration for GroupProtocol<T> 
where 
    T: Clone + Default,
{
    type Evidence = T;

    fn validate_evidence(_evidence: &Self::Evidence) -> bool {
        true
    }

    fn rehydrate(
        device_id: DeviceId,
        _account_id: AccountId,
        evidence: Self::Evidence,
    ) -> Result<Self, Self::Error> {
        Ok(Self::new(device_id, SessionId::new(), evidence))
    }
}

// ========== Group Protocol Constructors ==========

/// Create group membership protocol
pub fn new_group_membership_protocol(
    device_id: DeviceId,
    group_id: String,
) -> GroupProtocol<GroupMembershipState> {
    let initial_state = GroupMembershipState {
        group_id,
        epoch: 0,
        members: Vec::new(),
    };
    GroupProtocol::new(device_id, SessionId::new(), initial_state)
}

/// Create group messaging protocol
pub fn new_group_messaging_protocol(
    device_id: DeviceId,
    group_id: String,
) -> GroupProtocol<GroupMessagingState> {
    let initial_state = GroupMessagingState {
        group_id,
        current_epoch: 0,
        message_count: 0,
        last_ratchet: 0,
    };
    GroupProtocol::new(device_id, SessionId::new(), initial_state)
}

/// Create group admin protocol
pub fn new_group_admin_protocol(
    device_id: DeviceId,
    group_id: String,
) -> GroupProtocol<GroupAdminState> {
    let initial_state = GroupAdminState {
        group_id,
        pending_operations: Vec::new(),
        admin_epoch: 0,
    };
    GroupProtocol::new(device_id, SessionId::new(), initial_state)
}
