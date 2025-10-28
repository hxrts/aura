//! Journal protocol lifecycle placeholder.

use aura_types::{AccountId, DeviceId, SessionId};
use protocol_core::{
    capabilities::{ProtocolCapabilities, ProtocolEffects},
    lifecycle::{
        ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolRehydration, ProtocolStep,
    },
    metadata::{ProtocolMode, ProtocolPriority, ProtocolType},
    typestate::SessionState,
};
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
