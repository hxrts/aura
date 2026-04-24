//! Authentication state manager.

use super::state::with_state_mut_validated;
use crate::handlers::AuthChallenge;
use std::collections::HashMap;
use tokio::sync::RwLock;

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthManagerCommand {
    CacheChallenge,
    RemoveChallenge,
}

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
#[aura_macros::actor_owned(
    owner = "auth_manager",
    domain = "authentication",
    gate = "auth_command_ingress",
    command = AuthManagerCommand,
    capacity = 64,
    category = "actor_owned"
)]
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
    pub async fn cache_challenge(&self, challenge: AuthChallenge) -> Result<(), String> {
        with_state_mut_validated(
            &self.state,
            move |state| {
                if state
                    .pending_challenges
                    .contains_key(&challenge.challenge_id)
                {
                    return Err(format!(
                        "pending auth challenge collision for id {}",
                        challenge.challenge_id
                    ));
                }
                state
                    .pending_challenges
                    .insert(challenge.challenge_id.clone(), challenge);
                Ok(())
            },
            |state| state.validate(),
        )
        .await
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
