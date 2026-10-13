//! Runtime-driven relay-first, direct-upgrade path state for one peer.
//!
//! `docs/113_rendezvous.md` §3.2 describes the policy: start on the relay as
//! soon as a relay path is advertised, probe direct candidates in bounded
//! background attempts, promote to direct when a probe succeeds, and fall back
//! to the relay when a direct send fails. The typed retry state lives in
//! `aura_protocol::handlers::PeerConnectionActor`; this module drives it from
//! the runtime's actual sends and exposes the result as a `SelectedPeerPath`
//! observation. It is pure state: the service registry owns one instance per
//! `(context, peer)` and the transport effect asks it what to dial.

use aura_core::service::{EstablishPath, PeerPathKind, SelectedPeerPath};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_protocol::handlers::{
    CandidateGeneration, Degraded, Direct, NetworkGeneration, PeerConnection, PeerConnectionActor,
    RelayOnly, Upgrading,
};
use std::hash::{Hash, Hasher};
use std::time::Duration;

/// Bounded direct-upgrade attempts per generation window.
pub const DIRECT_UPGRADE_MAX_ATTEMPTS: u8 = 3;
/// First backoff between direct-upgrade probes.
pub const DIRECT_UPGRADE_BASE_BACKOFF: Duration = Duration::from_millis(500);
/// Longest backoff between direct-upgrade probes.
pub const DIRECT_UPGRADE_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Typestate phase of the peer connection with its runtime-selected payload.
enum Phase {
    RelayOnly(PeerConnection<RelayOnly, EstablishPath>),
    Upgrading(PeerConnection<Upgrading, EstablishPath>),
    Direct(PeerConnection<Direct, EstablishPath>),
    Degraded(PeerConnection<Degraded, EstablishPath>),
}

impl Phase {
    fn name(&self) -> &'static str {
        match self {
            Self::RelayOnly(_) => "relay_only",
            Self::Upgrading(_) => "upgrading",
            Self::Direct(_) => "direct",
            Self::Degraded(_) => "degraded",
        }
    }
}

/// What the transport should do for one send toward the peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendPlan {
    /// Path to carry this send.
    pub path: EstablishPath,
    /// Classification of `path`.
    pub kind: PeerPathKind,
    /// Direct candidate to probe after the send, when an upgrade attempt is due.
    pub probe: Option<EstablishPath>,
}

/// Relay-first path state for one `(context, peer)`.
pub struct PeerPathState {
    context_id: ContextId,
    peer: AuthorityId,
    actor: PeerConnectionActor<EstablishPath>,
    phase: Phase,
    candidate_fingerprint: u64,
    candidate_generation: u64,
    network_generation: u64,
    relay: Option<EstablishPath>,
    direct: Vec<EstablishPath>,
    selected: EstablishPath,
    selected_at_ms: u64,
    next_upgrade_at_ms: u64,
}

impl PeerPathState {
    /// Build the initial state from the peer's usable establish paths. `None`
    /// when nothing is usable.
    pub fn new(
        context_id: ContextId,
        peer: AuthorityId,
        candidates: &[EstablishPath],
        network_generation: u64,
        now_ms: u64,
    ) -> Option<Self> {
        let (relay, direct) = split_candidates(candidates);
        let selected = relay.clone().or_else(|| direct.first().cloned())?;
        let mut actor = PeerConnectionActor::new(
            DIRECT_UPGRADE_MAX_ATTEMPTS,
            DIRECT_UPGRADE_BASE_BACKOFF,
            DIRECT_UPGRADE_MAX_BACKOFF,
        );
        actor.on_selected_path_changed(CandidateGeneration(1), Some(selected.clone()));
        actor.on_network_changed(NetworkGeneration(network_generation));
        let phase = initial_phase(relay.as_ref(), &selected);
        Some(Self {
            context_id,
            peer,
            actor,
            phase,
            candidate_fingerprint: fingerprint(candidates),
            candidate_generation: 1,
            network_generation,
            relay,
            direct,
            selected,
            selected_at_ms: now_ms,
            next_upgrade_at_ms: now_ms,
        })
    }

    /// Apply the peer's current usable paths and the local network epoch. A
    /// changed candidate set starts a new candidate generation on the relay
    /// (or the only direct path); a changed network epoch keeps the current
    /// path but reopens the upgrade budget.
    pub fn refresh(&mut self, candidates: &[EstablishPath], network_generation: u64, now_ms: u64) {
        let fingerprint = fingerprint(candidates);
        if fingerprint != self.candidate_fingerprint {
            let (relay, direct) = split_candidates(candidates);
            if let Some(initial) = relay.clone().or_else(|| direct.first().cloned()) {
                self.candidate_fingerprint = fingerprint;
                self.candidate_generation += 1;
                self.actor.on_selected_path_changed(
                    CandidateGeneration(self.candidate_generation),
                    Some(initial.clone()),
                );
                self.phase = initial_phase(relay.as_ref(), &initial);
                self.relay = relay;
                self.direct = direct;
                self.selected = initial;
                self.selected_at_ms = now_ms;
                self.next_upgrade_at_ms = now_ms;
            }
        }
        if network_generation != self.network_generation {
            self.network_generation = network_generation;
            self.actor
                .on_network_changed(NetworkGeneration(network_generation));
            self.selected_at_ms = now_ms;
            self.next_upgrade_at_ms = now_ms;
            if let Phase::Upgrading(connection) = take_phase(&mut self.phase) {
                // A probe that straddled the network change proves nothing.
                self.phase = Phase::RelayOnly(
                    connection
                        .upgrade_failed()
                        .recover_to_relay(self.relay.clone()),
                );
            }
        }
    }

    /// Plan one send: the current path, plus a direct probe when the state is
    /// relay-only, a direct candidate exists, and the bounded budget allows
    /// another attempt now.
    pub fn plan_send(&mut self, now_ms: u64) -> SendPlan {
        let mut probe = None;
        if matches!(self.phase, Phase::RelayOnly(_))
            && !self.direct.is_empty()
            && now_ms >= self.next_upgrade_at_ms
        {
            if let Some(delay) = self.actor.next_retry_delay() {
                self.next_upgrade_at_ms = now_ms.saturating_add(delay.as_millis() as u64);
                let index = usize::from(self.actor.attempts_used().saturating_sub(1));
                let candidate = self.direct[index % self.direct.len()].clone();
                if let Phase::RelayOnly(connection) = take_phase(&mut self.phase) {
                    self.phase = Phase::Upgrading(connection.begin_upgrade());
                }
                probe = Some(candidate);
            }
        }
        SendPlan {
            path: self.selected.clone(),
            kind: PeerPathKind::of(&self.selected),
            probe,
        }
    }

    /// Record how dialing `path` went. A successful direct probe promotes the
    /// state to direct; a failed direct dial degrades it back to the relay
    /// when one is advertised.
    pub fn record_outcome(&mut self, path: &EstablishPath, ok: bool, now_ms: u64) {
        let kind = PeerPathKind::of(path);
        match (take_phase(&mut self.phase), kind, ok) {
            (Phase::Upgrading(connection), PeerPathKind::Direct, true) => {
                self.phase = Phase::Direct(connection.upgrade_succeeded(Some(path.clone())));
                self.selected = path.clone();
                self.selected_at_ms = now_ms;
            }
            (Phase::Upgrading(connection), PeerPathKind::Direct, false) => {
                self.phase = Phase::RelayOnly(
                    connection
                        .upgrade_failed()
                        .recover_to_relay(self.relay.clone()),
                );
            }
            (Phase::Direct(connection), PeerPathKind::Direct, false) => {
                let degraded = connection.degrade();
                match self.relay.clone() {
                    Some(relay) => {
                        self.phase =
                            Phase::RelayOnly(degraded.recover_to_relay(Some(relay.clone())));
                        self.selected = relay;
                        self.selected_at_ms = now_ms;
                        // The next upgrade probe waits for the bounded backoff.
                        if let Some(delay) = self.actor.next_retry_delay() {
                            self.next_upgrade_at_ms =
                                now_ms.saturating_add(delay.as_millis() as u64);
                        }
                    }
                    None => self.phase = Phase::Degraded(degraded),
                }
            }
            (Phase::Degraded(connection), PeerPathKind::Direct, true) => {
                // The only path works again.
                self.phase = Phase::Direct(
                    connection
                        .recover_to_relay(None)
                        .begin_upgrade()
                        .upgrade_succeeded(Some(path.clone())),
                );
                self.selected_at_ms = now_ms;
            }
            (phase, _, _) => self.phase = phase,
        }
    }

    /// Current selection as the shared observation type.
    pub fn selected(&self) -> SelectedPeerPath {
        SelectedPeerPath {
            peer: self.peer,
            context_id: self.context_id,
            kind: PeerPathKind::of(&self.selected),
            protocol: self.selected.route.destination.protocol,
            selected_at_ms: self.selected_at_ms,
            candidate_generation: self.candidate_generation,
            network_generation: self.network_generation,
            attempts_used: self.actor.attempts_used(),
        }
    }

    /// Path the next send would use.
    pub fn selected_path(&self) -> &EstablishPath {
        &self.selected
    }

    /// Name of the typestate phase, for diagnostics.
    pub fn phase_name(&self) -> &'static str {
        self.phase.name()
    }
}

fn initial_phase(relay: Option<&EstablishPath>, selected: &EstablishPath) -> Phase {
    let connection = PeerConnection::<RelayOnly, EstablishPath>::new_relay(relay.cloned());
    if relay.is_some() {
        Phase::RelayOnly(connection)
    } else {
        // No relay advertised: the direct path is the only path, so the
        // connection starts (and stays) direct without an upgrade cycle.
        Phase::Direct(
            connection
                .begin_upgrade()
                .upgrade_succeeded(Some(selected.clone())),
        )
    }
}

/// Move the phase out so a typestate transition can consume it; the caller
/// always writes a phase back.
fn take_phase(slot: &mut Phase) -> Phase {
    std::mem::replace(
        slot,
        Phase::RelayOnly(PeerConnection::<RelayOnly, EstablishPath>::new_relay(None)),
    )
}

fn split_candidates(candidates: &[EstablishPath]) -> (Option<EstablishPath>, Vec<EstablishPath>) {
    let relay = candidates
        .iter()
        .find(|path| PeerPathKind::of(path) == PeerPathKind::Relay)
        .cloned();
    let direct = candidates
        .iter()
        .filter(|path| PeerPathKind::of(path) == PeerPathKind::Direct)
        .cloned()
        .collect();
    (relay, direct)
}

fn fingerprint(candidates: &[EstablishPath]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for path in candidates {
        let endpoint = &path.route.destination;
        endpoint.protocol.hash(&mut hasher);
        endpoint.address.hash(&mut hasher);
        endpoint
            .relay_authority
            .map(|authority| authority.to_bytes())
            .hash(&mut hasher);
        path.route.hops.len().hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::service::{LinkEndpoint, LinkProtocol};

    fn ids() -> (ContextId, AuthorityId) {
        (
            ContextId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
        )
    }

    fn relay_path() -> EstablishPath {
        let mut endpoint = LinkEndpoint::relay(AuthorityId::new_from_entropy([3u8; 32]));
        endpoint.address = Some("10.0.0.1:41000".to_string());
        EstablishPath::direct(endpoint)
    }

    fn direct_path(addr: &str) -> EstablishPath {
        EstablishPath::direct(LinkEndpoint::direct(LinkProtocol::Tcp, addr))
    }

    #[test]
    fn relay_is_selected_first_when_advertised() {
        let (context, peer) = ids();
        let candidates = [direct_path("10.0.1.2:41002"), relay_path()];
        let state = PeerPathState::new(context, peer, &candidates, 0, 10).expect("state");
        let selected = state.selected();
        assert_eq!(selected.kind, PeerPathKind::Relay);
        assert_eq!(selected.protocol, LinkProtocol::WebSocketRelay);
        assert_eq!(selected.candidate_generation, 1);
        assert_eq!(selected.network_generation, 0);
        assert_eq!(state.phase_name(), "relay_only");
    }

    #[test]
    fn direct_only_peers_start_direct_without_probes() {
        let (context, peer) = ids();
        let candidates = [direct_path("192.168.1.9:41002")];
        let mut state = PeerPathState::new(context, peer, &candidates, 0, 10).expect("state");
        assert_eq!(state.phase_name(), "direct");
        let plan = state.plan_send(10);
        assert_eq!(plan.kind, PeerPathKind::Direct);
        assert!(plan.probe.is_none());
    }

    #[test]
    fn successful_probe_promotes_to_direct_and_failure_degrades_to_relay() {
        let (context, peer) = ids();
        let direct = direct_path("10.0.1.2:41002");
        let candidates = [relay_path(), direct.clone()];
        let mut state = PeerPathState::new(context, peer, &candidates, 0, 100).expect("state");

        let plan = state.plan_send(100);
        assert_eq!(plan.kind, PeerPathKind::Relay);
        assert_eq!(plan.probe.as_ref(), Some(&direct));
        assert_eq!(state.phase_name(), "upgrading");

        state.record_outcome(&direct, true, 150);
        assert_eq!(state.phase_name(), "direct");
        assert_eq!(state.selected().kind, PeerPathKind::Direct);
        assert_eq!(state.selected().selected_at_ms, 150);

        state.record_outcome(&direct, false, 200);
        assert_eq!(state.phase_name(), "relay_only");
        assert_eq!(state.selected().kind, PeerPathKind::Relay);
    }

    #[test]
    fn probes_are_bounded_until_a_generation_changes() {
        let (context, peer) = ids();
        let direct = direct_path("10.0.1.2:41002");
        let candidates = [relay_path(), direct.clone()];
        let mut state = PeerPathState::new(context, peer, &candidates, 0, 0).expect("state");

        let mut probes = 0;
        let mut now = 0;
        for _ in 0..20 {
            now += 60_000;
            let plan = state.plan_send(now);
            if let Some(probe) = plan.probe {
                probes += 1;
                state.record_outcome(&probe, false, now);
            }
        }
        assert_eq!(probes, usize::from(DIRECT_UPGRADE_MAX_ATTEMPTS));
        assert_eq!(state.selected().attempts_used, DIRECT_UPGRADE_MAX_ATTEMPTS);

        // A network change reopens the budget and is visible in the observation.
        state.refresh(&candidates, 1, now + 1);
        assert_eq!(state.selected().network_generation, 1);
        assert_eq!(state.selected().attempts_used, 0);
        let plan = state.plan_send(now + 2);
        assert!(plan.probe.is_some());

        // A changed candidate set starts a new candidate generation on the relay.
        let changed = [relay_path(), direct_path("10.0.1.2:41099")];
        state.refresh(&changed, 1, now + 3);
        assert_eq!(state.selected().candidate_generation, 2);
        assert_eq!(state.selected().kind, PeerPathKind::Relay);
        assert_eq!(state.phase_name(), "relay_only");
    }

    #[test]
    fn unchanged_candidates_keep_generation_and_selection() {
        let (context, peer) = ids();
        let candidates = [relay_path(), direct_path("10.0.1.2:41002")];
        let mut state = PeerPathState::new(context, peer, &candidates, 0, 0).expect("state");
        let plan = state.plan_send(0);
        state.record_outcome(plan.probe.as_ref().expect("probe"), true, 1);
        state.refresh(&candidates, 0, 2);
        assert_eq!(state.selected().candidate_generation, 1);
        assert_eq!(state.selected().kind, PeerPathKind::Direct);
    }
}
