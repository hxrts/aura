//! Session Type States for CGKA (Continuous Group Key Agreement) Protocol
//!
//! This module provides session type definitions for the BeeKEM CGKA protocol.
//! Note: These session types are currently placeholders for future implementation.

use session_types::{SessionProtocol, SessionState};
use uuid::Uuid;

/// CGKA session error (placeholder for future implementation)
#[derive(Debug, thiserror::Error)]
pub enum CgkaSessionError {
    #[error("CGKA protocol error: {0}")]
    ProtocolError(String),
}

/// Group initialized state (placeholder for future implementation)
#[derive(Debug, Clone)]
pub struct CgkaGroupInitialized;

impl SessionState for CgkaGroupInitialized {
    const NAME: &'static str = "CgkaGroupInitialized";
    const IS_FINAL: bool = false;
}

/// CGKA protocol state union (placeholder for future implementation)
#[derive(Debug, Clone)]
pub enum CgkaSessionState {
    CgkaGroupInitialized(CgkaGroupInitialized),
}

impl SessionProtocol for CgkaSessionState {
    type State = CgkaGroupInitialized;
    type Output = ();
    type Error = CgkaSessionError;

    #[allow(clippy::disallowed_methods)]
    fn session_id(&self) -> Uuid {
        match self {
            CgkaSessionState::CgkaGroupInitialized(_) => Uuid::new_v4(),
        }
    }

    #[allow(clippy::disallowed_methods)]
    fn protocol_id(&self) -> Uuid {
        match self {
            CgkaSessionState::CgkaGroupInitialized(_) => Uuid::new_v4(),
        }
    }

    #[allow(clippy::disallowed_methods)]
    fn device_id(&self) -> Uuid {
        Uuid::new_v4()
    }

    fn state_name(&self) -> &'static str {
        match self {
            CgkaSessionState::CgkaGroupInitialized(_) => CgkaGroupInitialized::NAME,
        }
    }

    fn can_terminate(&self) -> bool {
        match self {
            CgkaSessionState::CgkaGroupInitialized(_) => false,
        }
    }
}

/// Create a new CGKA protocol instance (placeholder for future implementation)
pub fn new_session_typed_cgka() -> Result<CgkaSessionState, CgkaSessionError> {
    Ok(CgkaSessionState::CgkaGroupInitialized(CgkaGroupInitialized))
}

/// Rehydrate CGKA protocol from evidence (placeholder for future implementation)
pub fn rehydrate_cgka_session() -> Result<CgkaSessionState, CgkaSessionError> {
    new_session_typed_cgka()
}

// Re-export for compatibility
pub type SessionTypedCgka = CgkaSessionState;
