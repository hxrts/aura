//! Session Type States for Journal/Ledger Protocol
//!
//! This module provides session type definitions for the CRDT-based authenticated ledger.
//! Note: These session types are currently placeholders for future implementation.

use aura_session_core::{SessionProtocol, SessionState};
use aura_types::DeviceId;
use uuid::Uuid;

/// Journal protocol core (placeholder for future implementation)
#[derive(Debug, Clone)]
pub struct JournalProtocolCore {
    pub session_id: Uuid,
}

/// Journal session error (placeholder for future implementation)
#[derive(Debug, thiserror::Error)]
pub enum JournalSessionError {
    #[error("Journal protocol error: {0}")]
    ProtocolError(String),
}

/// Ledger empty state (placeholder for future implementation)
#[derive(Debug, Clone)]
pub struct LedgerEmpty;

impl SessionState for LedgerEmpty {
    const NAME: &'static str = "LedgerEmpty";
    const IS_FINAL: bool = false;
    const CAN_TERMINATE: bool = false;
}

/// Journal protocol state union (placeholder for future implementation)
#[derive(Debug)]
pub enum JournalSessionState {
    LedgerEmpty(LedgerEmpty),
}

impl SessionProtocol for JournalSessionState {
    type Error = JournalSessionError;

    fn session_id(&self) -> Uuid {
        match self {
            JournalSessionState::LedgerEmpty(_) => Uuid::new_v4(),
        }
    }

    fn device_id(&self) -> DeviceId {
        DeviceId(Uuid::new_v4())
    }

    fn state_name(&self) -> &'static str {
        match self {
            JournalSessionState::LedgerEmpty(_) => LedgerEmpty::NAME,
        }
    }

    fn is_final(&self) -> bool {
        match self {
            JournalSessionState::LedgerEmpty(_) => LedgerEmpty::IS_FINAL,
        }
    }

    fn can_terminate(&self) -> bool {
        match self {
            JournalSessionState::LedgerEmpty(_) => LedgerEmpty::CAN_TERMINATE,
        }
    }
}

/// Create a new journal protocol instance (placeholder for future implementation)
pub fn new_session_typed_journal() -> Result<JournalSessionState, JournalSessionError> {
    Ok(JournalSessionState::LedgerEmpty(LedgerEmpty))
}

/// Rehydrate journal protocol from evidence (placeholder for future implementation)
pub fn rehydrate_journal_session() -> Result<JournalSessionState, JournalSessionError> {
    new_session_typed_journal()
}

// Re-export for compatibility
pub use JournalSessionState as JournalProtocolState;
pub type SessionTypedJournal = JournalSessionState;
