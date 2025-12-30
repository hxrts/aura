//! Authorization helpers for frontends.

use crate::{views::home::ResidentRole, StateSnapshot};
use aura_core::AuraError;

/// Require admin or owner role in the current home.
pub fn require_admin(snapshot: Option<&StateSnapshot>, operation: &str) -> Result<(), AuraError> {
    let role = snapshot.and_then(|s| s.homes.current_home().map(|h| h.my_role));
    match role {
        Some(ResidentRole::Admin | ResidentRole::Owner) => Ok(()),
        Some(ResidentRole::Resident) => Err(AuraError::agent(format!(
            "Permission denied: {operation} requires administrator privileges",
        ))),
        None => Err(AuraError::agent(format!(
            "Permission denied: {operation} requires a home context",
        ))),
    }
}
