//! Authentication state manager.

use super::state::with_state_mut_validated;
use crate::handlers::AuthChallenge;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct AuthState {
    pending_challenges: HashMap<String, AuthChallenge>,
}

impl AuthState {
    fn validate(&self) -> Result<(), super::invariant::InvariantViolation> {
        for (id, challenge) in &self.pending_challenges {
            if id != &challenge.challenge_id {
                return Err(super::invariant::InvariantViolation::new(
                    "AuthManager",
                    format!(
                        "challenge id mismatch: key {} vs value {}",
                        id, challenge.challenge_id
                    ),
                ));
            }
        }
        Ok(())
    }
}

/// Manages authentication challenges for the auth handler.
#[derive(Default)]
pub struct AuthManager {
    state: RwLock<AuthState>,
}

impl AuthManager {
    /// Create a new auth manager.
    pub fn new() -> Self {
        Self {
            state: RwLock::new(AuthState::default()),
        }
    }

    /// Cache a pending challenge.
    pub async fn cache_challenge(&self, challenge: AuthChallenge) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .pending_challenges
                    .insert(challenge.challenge_id.clone(), challenge);
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Get a cached challenge.
    pub async fn get_challenge(&self, challenge_id: &str) -> Option<AuthChallenge> {
        self.state
            .read()
            .await
            .pending_challenges
            .get(challenge_id)
            .cloned()
    }

    /// Remove a cached challenge.
    pub async fn remove_challenge(&self, challenge_id: &str) -> Option<AuthChallenge> {
        with_state_mut_validated(
            &self.state,
            |state| state.pending_challenges.remove(challenge_id),
            |state| state.validate(),
        )
        .await
    }
}
