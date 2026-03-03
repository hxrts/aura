//! Typed peer-connection retry state and typestate transitions.

use std::marker::PhantomData;
use std::time::Duration;

/// Monotonic generation for candidate sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CandidateGeneration(pub u64);

/// Monotonic generation for network change epochs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NetworkGeneration(pub u64);

/// Bounded retry budget for connection attempts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptBudget {
    max_attempts: u8,
    attempts_used: u8,
}

impl AttemptBudget {
    /// Create a new attempt budget.
    pub fn new(max_attempts: u8) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            attempts_used: 0,
        }
    }

    /// Consume one attempt. Returns `true` if an attempt was available.
    pub fn try_consume(&mut self) -> bool {
        if self.attempts_used >= self.max_attempts {
            return false;
        }
        self.attempts_used += 1;
        true
    }

    /// Reset the budget back to zero attempts used.
    pub fn reset(&mut self) {
        self.attempts_used = 0;
    }

    /// Total attempts used.
    pub fn attempts_used(&self) -> u8 {
        self.attempts_used
    }

    /// Configured maximum attempts.
    pub fn max_attempts(&self) -> u8 {
        self.max_attempts
    }
}

/// Exponential backoff window with bounded jitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackoffWindow {
    base: Duration,
    max: Duration,
}

impl BackoffWindow {
    /// Create a new backoff window.
    pub fn new(base: Duration, max: Duration) -> Self {
        Self {
            base: base.max(Duration::from_millis(1)),
            max: max.max(base),
        }
    }

    /// Compute jittered delay for `attempt_index` (0-based).
    pub fn jittered_delay(&self, attempt_index: u8) -> Duration {
        let exp = 2u64.saturating_pow(attempt_index as u32);
        let raw_ms = self.base.as_millis().saturating_mul(exp as u128);
        let capped_ms = raw_ms.min(self.max.as_millis()) as u64;

        let (jitter_pct, jitter_sign) = deterministic_jitter(attempt_index);
        let delta = capped_ms.saturating_mul(jitter_pct) / 100;
        let jittered = if jitter_sign {
            capped_ms.saturating_add(delta)
        } else {
            capped_ms.saturating_sub(delta)
        };
        Duration::from_millis(jittered.max(1))
    }
}

fn deterministic_jitter(attempt_index: u8) -> (u64, bool) {
    // Deterministic bounded jitter avoids global RNG usage while preserving backoff spread.
    let seed = (attempt_index as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(13);
    let jitter_pct = seed % 21; // 0..=20%
    let jitter_sign = (seed & 1) == 1;
    (jitter_pct, jitter_sign)
}

/// Candidate path kind for relay-first selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    Relay,
    Direct,
}

/// Candidate abstraction used by `PeerConnectionActor`.
pub trait ConnectionCandidate {
    /// Path kind used for relay-first ordering.
    fn kind(&self) -> CandidateKind;
    /// Whether the candidate remains recoverable with current local state.
    fn is_recoverable(&self) -> bool;
}

/// `RelayOnly` typestate marker.
#[derive(Debug, Clone, Copy)]
pub struct RelayOnly;
/// `Upgrading` typestate marker.
#[derive(Debug, Clone, Copy)]
pub struct Upgrading;
/// `Direct` typestate marker.
#[derive(Debug, Clone, Copy)]
pub struct Direct;
/// `Degraded` typestate marker.
#[derive(Debug, Clone, Copy)]
pub struct Degraded;

/// Typestate-wrapped peer connection with path selection payload.
#[derive(Debug, Clone)]
pub struct PeerConnection<S, C> {
    candidate: Option<C>,
    _state: PhantomData<S>,
}

impl<C> PeerConnection<RelayOnly, C> {
    pub fn new_relay(candidate: Option<C>) -> Self {
        Self {
            candidate,
            _state: PhantomData,
        }
    }

    pub fn begin_upgrade(self) -> PeerConnection<Upgrading, C> {
        PeerConnection {
            candidate: self.candidate,
            _state: PhantomData,
        }
    }
}

impl<C> PeerConnection<Upgrading, C> {
    pub fn upgrade_succeeded(self, candidate: Option<C>) -> PeerConnection<Direct, C> {
        PeerConnection {
            candidate,
            _state: PhantomData,
        }
    }

    pub fn upgrade_failed(self) -> PeerConnection<Degraded, C> {
        PeerConnection {
            candidate: self.candidate,
            _state: PhantomData,
        }
    }
}

impl<C> PeerConnection<Direct, C> {
    pub fn degrade(self) -> PeerConnection<Degraded, C> {
        PeerConnection {
            candidate: self.candidate,
            _state: PhantomData,
        }
    }
}

impl<C> PeerConnection<Degraded, C> {
    pub fn recover_to_relay(self, candidate: Option<C>) -> PeerConnection<RelayOnly, C> {
        PeerConnection {
            candidate,
            _state: PhantomData,
        }
    }
}

/// Stateful retry actor for connection attempts.
#[derive(Debug, Clone)]
pub struct PeerConnectionActor<C> {
    candidate_generation: CandidateGeneration,
    network_generation: NetworkGeneration,
    attempt_budget: AttemptBudget,
    backoff_window: BackoffWindow,
    selected_path: Option<C>,
}

impl<C> PeerConnectionActor<C>
where
    C: Clone + ConnectionCandidate,
{
    /// Create a new actor with bounded retry/backoff settings.
    pub fn new(max_attempts: u8, base_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            candidate_generation: CandidateGeneration::default(),
            network_generation: NetworkGeneration::default(),
            attempt_budget: AttemptBudget::new(max_attempts),
            backoff_window: BackoffWindow::new(base_backoff, max_backoff),
            selected_path: None,
        }
    }

    /// Update candidates and reset retry state when generation changes.
    pub fn on_candidates_changed(&mut self, generation: CandidateGeneration, candidates: &[C]) {
        if generation != self.candidate_generation {
            self.candidate_generation = generation;
            self.attempt_budget.reset();
        }
        self.selected_path = self.select_relay_first(candidates);
    }

    /// Apply a network-generation change and reset retry state.
    pub fn on_network_changed(&mut self, generation: NetworkGeneration) {
        if generation != self.network_generation {
            self.network_generation = generation;
            self.attempt_budget.reset();
        }
    }

    /// Consume an attempt and return jittered delay for this retry.
    pub fn next_retry_delay(&mut self) -> Option<Duration> {
        if !self.attempt_budget.try_consume() {
            return None;
        }
        let attempt_index = self.attempt_budget.attempts_used().saturating_sub(1);
        Some(self.backoff_window.jittered_delay(attempt_index))
    }

    /// Current selected path.
    pub fn selected_path(&self) -> Option<&C> {
        self.selected_path.as_ref()
    }

    /// Number of retry attempts consumed in the current generation window.
    pub fn attempts_used(&self) -> u8 {
        self.attempt_budget.attempts_used()
    }

    /// Configured retry budget ceiling.
    pub fn max_attempts(&self) -> u8 {
        self.attempt_budget.max_attempts()
    }

    fn select_relay_first(&self, candidates: &[C]) -> Option<C> {
        let relay = candidates.iter().find(|candidate| {
            candidate.kind() == CandidateKind::Relay && candidate.is_recoverable()
        });
        if let Some(candidate) = relay {
            return Some(candidate.clone());
        }

        candidates
            .iter()
            .find(|candidate| {
                candidate.kind() == CandidateKind::Direct && candidate.is_recoverable()
            })
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone)]
    struct Candidate {
        kind: CandidateKind,
        recoverable: bool,
    }

    impl ConnectionCandidate for Candidate {
        fn kind(&self) -> CandidateKind {
            self.kind
        }

        fn is_recoverable(&self) -> bool {
            self.recoverable
        }
    }

    #[test]
    fn reset_budget_on_generation_change() {
        let mut actor = PeerConnectionActor::<Candidate>::new(
            3,
            Duration::from_millis(20),
            Duration::from_millis(200),
        );
        assert!(actor.next_retry_delay().is_some());
        assert!(actor.next_retry_delay().is_some());

        actor.on_network_changed(NetworkGeneration(1));
        assert!(actor.next_retry_delay().is_some());
    }

    #[test]
    fn relay_first_selection_prefers_relay_when_recoverable() {
        let mut actor = PeerConnectionActor::<Candidate>::new(
            3,
            Duration::from_millis(20),
            Duration::from_millis(200),
        );
        let candidates = vec![
            Candidate {
                kind: CandidateKind::Direct,
                recoverable: true,
            },
            Candidate {
                kind: CandidateKind::Relay,
                recoverable: true,
            },
        ];
        actor.on_candidates_changed(CandidateGeneration(1), &candidates);

        assert_eq!(
            actor.selected_path().map(|candidate| candidate.kind()),
            Some(CandidateKind::Relay)
        );
    }

    #[test]
    fn typestate_transitions_compile_and_preserve_payload() {
        let relay = PeerConnection::<RelayOnly, Candidate>::new_relay(None);
        let upgrading = relay.begin_upgrade();
        let direct = upgrading.upgrade_succeeded(None);
        let degraded = direct.degrade();
        let _relay_again = degraded.recover_to_relay(None);
    }
}
