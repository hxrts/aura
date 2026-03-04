//! Telltale VM effect handler adapter for Aura runtime integration.
//!
//! This handler is intentionally deterministic and session-local. It can be used as
//! the VM host boundary while Aura routes protocol semantics through existing
//! effect interpreters.

use parking_lot::Mutex;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use telltale_vm::effect::{AcquireDecision, EffectHandler, TopologyPerturbation};
use telltale_vm::{OutputConditionHint, SessionId, Value};

use aura_core::effects::guard::EffectCommand;
use aura_core::types::scope::ResourceScope;

use super::vm_hardening::{
    AURA_OUTPUT_PREDICATE_CHOICE, AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE,
    AURA_OUTPUT_PREDICATE_GUARD_RELEASE, AURA_OUTPUT_PREDICATE_OBSERVABLE,
    AURA_OUTPUT_PREDICATE_STEP, AURA_OUTPUT_PREDICATE_TRANSPORT_RECV,
    AURA_OUTPUT_PREDICATE_TRANSPORT_SEND,
};

/// Structured event emitted by [`AuraVmEffectHandler`] for debugging/replay hooks.
#[derive(Debug, Clone, PartialEq)]
pub enum AuraVmEffectEvent {
    Send {
        role: String,
        partner: String,
        label: String,
    },
    Recv {
        role: String,
        partner: String,
        label: String,
        payload: Value,
    },
    Choose {
        role: String,
        partner: String,
        labels: Vec<String>,
        selected: String,
    },
    Step {
        role: String,
    },
    Acquire {
        sid: SessionId,
        role: String,
        layer: String,
        granted: bool,
    },
    Release {
        sid: SessionId,
        role: String,
        layer: String,
        evidence: Value,
    },
}

/// Effect-command envelope emitted from VM callbacks.
#[derive(Debug, Clone)]
pub struct AuraVmEffectEnvelope {
    /// Source VM callback event.
    pub event: AuraVmEffectEvent,
    /// Effect commands to route through Aura interpreters.
    pub commands: Vec<EffectCommand>,
}

impl AuraVmEffectEnvelope {
    fn metadata(event: AuraVmEffectEvent, key: &str, value: String) -> Self {
        Self {
            event,
            commands: vec![EffectCommand::StoreMetadata {
                key: key.to_string(),
                value,
            }],
        }
    }
}

type EnvelopeSink = Arc<dyn Fn(&AuraVmEffectEnvelope) -> Result<(), String> + Send + Sync>;

/// Lease material tracked for VM guard-layer acquire/release callbacks.
#[derive(Debug, Clone, PartialEq)]
pub struct AuraVmCapabilityLease {
    /// Session identifier for the lease.
    pub sid: SessionId,
    /// VM role name that acquired the lease.
    pub role: String,
    /// VM layer identifier.
    pub layer: String,
    /// Optional mapped Aura resource scope.
    pub scope: Option<ResourceScope>,
    /// Evidence issued by the acquire callback.
    pub evidence: Value,
}

/// Deterministic telemetry counters for VM guard-layer semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuraVmTelemetry {
    /// Count of acquire decisions that blocked.
    pub acquire_denied: u64,
    /// Count of release attempts without an active lease.
    pub release_faults: u64,
    /// Count of envelope sink failures returned as runtime errors.
    pub envelope_sink_faults: u64,
}

/// Deterministic VM host effect handler for Aura integration work.
///
/// Behavior:
/// - `handle_send`: uses queued payloads when available; otherwise falls back to
///   the sender state tail or `Value::Unit`.
/// - `handle_choose`: uses queued branch labels when available; otherwise selects
///   the first offered label.
/// - `handle_acquire`: grants by default unless the layer is explicitly denied.
pub struct AuraVmEffectHandler {
    identity: String,
    outbound: Mutex<VecDeque<Value>>,
    branch_choices: Mutex<VecDeque<String>>,
    denied_layers: Mutex<HashSet<String>>,
    layer_scope_map: Mutex<HashMap<String, ResourceScope>>,
    active_leases: Mutex<HashMap<(SessionId, String, String), AuraVmCapabilityLease>>,
    output_predicates: Mutex<HashMap<String, String>>,
    telemetry: Mutex<AuraVmTelemetry>,
    events: Mutex<Vec<AuraVmEffectEvent>>,
    envelopes: Mutex<VecDeque<AuraVmEffectEnvelope>>,
    topology_schedule: Mutex<BTreeMap<u64, Vec<TopologyPerturbation>>>,
    envelope_sink: Mutex<Option<EnvelopeSink>>,
}

impl std::fmt::Debug for AuraVmEffectHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuraVmEffectHandler")
            .field("identity", &self.identity)
            .field("outbound_len", &self.outbound.lock().len())
            .field("branch_choices_len", &self.branch_choices.lock().len())
            .field("denied_layers_len", &self.denied_layers.lock().len())
            .field("layer_scope_map_len", &self.layer_scope_map.lock().len())
            .field("active_leases_len", &self.active_leases.lock().len())
            .field(
                "output_predicates_len",
                &self.output_predicates.lock().len(),
            )
            .field("telemetry", &*self.telemetry.lock())
            .field("events_len", &self.events.lock().len())
            .field("envelopes_len", &self.envelopes.lock().len())
            .field("topology_ticks_len", &self.topology_schedule.lock().len())
            .finish()
    }
}

impl Default for AuraVmEffectHandler {
    fn default() -> Self {
        Self::new("aura-vm-effect-handler")
    }
}

impl AuraVmEffectHandler {
    /// Create a new deterministic VM effect handler with a stable identity.
    pub fn new(identity: impl Into<String>) -> Self {
        Self {
            identity: identity.into(),
            outbound: Mutex::new(VecDeque::new()),
            branch_choices: Mutex::new(VecDeque::new()),
            denied_layers: Mutex::new(HashSet::new()),
            layer_scope_map: Mutex::new(HashMap::new()),
            active_leases: Mutex::new(HashMap::new()),
            output_predicates: Mutex::new(HashMap::new()),
            telemetry: Mutex::new(AuraVmTelemetry::default()),
            events: Mutex::new(Vec::new()),
            envelopes: Mutex::new(VecDeque::new()),
            topology_schedule: Mutex::new(BTreeMap::new()),
            envelope_sink: Mutex::new(None),
        }
    }

    /// Queue a payload to be used by the next VM send hook.
    pub fn push_send_value(&self, value: Value) {
        self.outbound.lock().push_back(value);
    }

    /// Queue a label to be selected by the next VM choose hook.
    pub fn push_branch_choice(&self, label: impl Into<String>) {
        self.branch_choices.lock().push_back(label.into());
    }

    /// Mark a guard layer as denied for acquire checks.
    pub fn deny_layer(&self, layer: impl Into<String>) {
        self.denied_layers.lock().insert(layer.into());
    }

    /// Remove a guard-layer deny rule.
    pub fn allow_layer(&self, layer: &str) {
        self.denied_layers.lock().remove(layer);
    }

    /// Map a VM layer id to an Aura resource scope.
    pub fn bind_layer_scope(&self, layer: impl Into<String>, scope: ResourceScope) {
        self.layer_scope_map.lock().insert(layer.into(), scope);
    }

    /// Resolve a mapped Aura resource scope for a VM layer id.
    pub fn layer_scope(&self, layer: &str) -> Option<ResourceScope> {
        self.layer_scope_map.lock().get(layer).cloned()
    }

    /// Snapshot currently active VM capability leases.
    pub fn active_leases(&self) -> Vec<AuraVmCapabilityLease> {
        self.active_leases.lock().values().cloned().collect()
    }

    /// Snapshot deterministic acquire/release telemetry.
    pub fn telemetry(&self) -> AuraVmTelemetry {
        self.telemetry.lock().clone()
    }

    /// Guard contention/failure counters suitable for structured telemetry export.
    pub fn guard_contention_snapshot(&self) -> BTreeMap<String, u64> {
        let telemetry = self.telemetry();
        BTreeMap::from([
            ("acquire_denied".to_string(), telemetry.acquire_denied),
            ("release_faults".to_string(), telemetry.release_faults),
            (
                "envelope_sink_faults".to_string(),
                telemetry.envelope_sink_faults,
            ),
        ])
    }

    /// Snapshot the recorded VM effect events.
    pub fn events(&self) -> Vec<AuraVmEffectEvent> {
        self.events.lock().clone()
    }

    /// Drain queued effect envelopes emitted by VM callbacks.
    pub fn drain_envelopes(&self) -> Vec<AuraVmEffectEnvelope> {
        let mut guard = self.envelopes.lock();
        guard.drain(..).collect()
    }

    /// Register a sink that receives each envelope as it is emitted.
    pub fn set_envelope_sink(
        &self,
        sink: impl Fn(&AuraVmEffectEnvelope) -> Result<(), String> + Send + Sync + 'static,
    ) {
        *self.envelope_sink.lock() = Some(Arc::new(sink));
    }

    /// Schedule one topology perturbation to be emitted at an exact scheduler tick.
    pub fn schedule_topology_event(&self, tick: u64, event: TopologyPerturbation) {
        self.topology_schedule
            .lock()
            .entry(tick)
            .or_default()
            .push(event);
    }

    /// Schedule multiple topology perturbations for one scheduler tick.
    pub fn schedule_topology_events(
        &self,
        tick: u64,
        events: impl IntoIterator<Item = TopologyPerturbation>,
    ) {
        self.topology_schedule
            .lock()
            .entry(tick)
            .or_default()
            .extend(events);
    }

    /// Remove all queued topology perturbations.
    pub fn clear_topology_schedule(&self) {
        self.topology_schedule.lock().clear();
    }

    fn record(&self, event: AuraVmEffectEvent) {
        self.events.lock().push(event);
    }

    fn record_output_predicate(&self, role: &str, predicate_ref: &str) {
        self.output_predicates
            .lock()
            .insert(role.to_string(), predicate_ref.to_string());
    }

    fn emit_envelope(&self, envelope: AuraVmEffectEnvelope) -> Result<(), String> {
        if let Some(sink) = self.envelope_sink.lock().clone() {
            if let Err(error) = sink(&envelope) {
                let mut telemetry = self.telemetry.lock();
                telemetry.envelope_sink_faults = telemetry.envelope_sink_faults.saturating_add(1);
                return Err(format!("envelope sink failed: {error}"));
            }
        }
        self.envelopes.lock().push_back(envelope);
        Ok(())
    }
}

impl EffectHandler for AuraVmEffectHandler {
    fn handler_identity(&self) -> String {
        self.identity.clone()
    }

    fn handle_send(
        &self,
        role: &str,
        partner: &str,
        label: &str,
        state: &[Value],
    ) -> Result<Value, String> {
        let event = AuraVmEffectEvent::Send {
            role: role.to_string(),
            partner: partner.to_string(),
            label: label.to_string(),
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_TRANSPORT_SEND);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.send",
            format!("{role}->{partner}:{label}"),
        ))?;

        if let Some(value) = self.outbound.lock().pop_front() {
            return Ok(value);
        }

        Ok(state.last().cloned().unwrap_or(Value::Unit))
    }

    fn handle_recv(
        &self,
        role: &str,
        partner: &str,
        label: &str,
        state: &mut Vec<Value>,
        payload: &Value,
    ) -> Result<(), String> {
        if let Some(last) = state.last_mut() {
            *last = payload.clone();
        } else {
            state.push(payload.clone());
        }
        let event = AuraVmEffectEvent::Recv {
            role: role.to_string(),
            partner: partner.to_string(),
            label: label.to_string(),
            payload: payload.clone(),
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_TRANSPORT_RECV);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.recv",
            format!("{role}<-{partner}:{label}"),
        ))?;
        Ok(())
    }

    fn handle_choose(
        &self,
        role: &str,
        partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> Result<String, String> {
        if labels.is_empty() {
            return Err("no labels available".to_string());
        }

        let selected = if let Some(candidate) = self.branch_choices.lock().pop_front() {
            if labels.iter().any(|label| label == &candidate) {
                candidate
            } else {
                return Err(format!("queued branch choice '{candidate}' is not valid"));
            }
        } else {
            labels[0].clone()
        };

        let event = AuraVmEffectEvent::Choose {
            role: role.to_string(),
            partner: partner.to_string(),
            labels: labels.to_vec(),
            selected: selected.clone(),
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_CHOICE);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.choose",
            format!("{role}:{selected}"),
        ))?;

        Ok(selected)
    }

    fn step(&self, role: &str, _state: &mut Vec<Value>) -> Result<(), String> {
        let event = AuraVmEffectEvent::Step {
            role: role.to_string(),
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_STEP);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.step",
            role.to_string(),
        ))?;
        Ok(())
    }

    fn handle_acquire(
        &self,
        sid: SessionId,
        role: &str,
        layer: &str,
        _state: &[Value],
    ) -> Result<AcquireDecision, String> {
        let granted = !self.denied_layers.lock().contains(layer);
        let mapped_scope = self.layer_scope(layer);
        let evidence = Value::Str(layer.to_string());
        let event = AuraVmEffectEvent::Acquire {
            sid,
            role: role.to_string(),
            layer: layer.to_string(),
            granted,
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.acquire",
            format!(
                "sid={sid}:{role}:{layer}:{granted}:scope={}",
                mapped_scope
                    .as_ref()
                    .map(ResourceScope::resource_pattern)
                    .unwrap_or_else(|| "unmapped".to_string())
            ),
        ))?;

        if granted {
            self.active_leases.lock().insert(
                (sid, role.to_string(), layer.to_string()),
                AuraVmCapabilityLease {
                    sid,
                    role: role.to_string(),
                    layer: layer.to_string(),
                    scope: mapped_scope,
                    evidence: evidence.clone(),
                },
            );
            Ok(AcquireDecision::Grant(evidence))
        } else {
            let mut telemetry = self.telemetry.lock();
            telemetry.acquire_denied = telemetry.acquire_denied.saturating_add(1);
            Ok(AcquireDecision::Block)
        }
    }

    fn handle_release(
        &self,
        sid: SessionId,
        role: &str,
        layer: &str,
        evidence: &Value,
        _state: &[Value],
    ) -> Result<(), String> {
        let event = AuraVmEffectEvent::Release {
            sid,
            role: role.to_string(),
            layer: layer.to_string(),
            evidence: evidence.clone(),
        };
        self.record(event.clone());
        self.record_output_predicate(role, AURA_OUTPUT_PREDICATE_GUARD_RELEASE);
        self.emit_envelope(AuraVmEffectEnvelope::metadata(
            event,
            "vm.release",
            format!("sid={sid}:{role}:{layer}"),
        ))?;
        let removed = self
            .active_leases
            .lock()
            .remove(&(sid, role.to_string(), layer.to_string()));
        if removed.is_none() {
            let mut telemetry = self.telemetry.lock();
            telemetry.release_faults = telemetry.release_faults.saturating_add(1);
            return Err(format!(
                "release without active lease sid={sid} role={role} layer={layer}"
            ));
        }
        Ok(())
    }

    fn output_condition_hint(
        &self,
        _sid: SessionId,
        role: &str,
        _state: &[Value],
    ) -> Option<OutputConditionHint> {
        let predicate_ref = self
            .output_predicates
            .lock()
            .get(role)
            .cloned()
            .unwrap_or_else(|| AURA_OUTPUT_PREDICATE_OBSERVABLE.to_string());
        Some(OutputConditionHint {
            predicate_ref,
            witness_ref: Some(format!("role:{role}")),
        })
    }

    fn topology_events(&self, tick: u64) -> Result<Vec<TopologyPerturbation>, String> {
        let mut events = self
            .topology_schedule
            .lock()
            .remove(&tick)
            .unwrap_or_default();
        events.sort_by_key(TopologyPerturbation::ordering_key);
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::scope::{AuthorityOp, ResourceScope};
    use aura_core::AuthorityId;
    use std::time::Duration;
    use telltale_vm::effect::{CorruptionType, TopologyPerturbation};
    use uuid::Uuid;

    #[test]
    fn queued_payload_and_branch_are_used() {
        let handler = AuraVmEffectHandler::default();
        handler.push_send_value(Value::Nat(7));
        handler.push_branch_choice("Commit");

        let send = handler
            .handle_send("A", "B", "Msg", &[])
            .expect("send should succeed");
        assert_eq!(send, Value::Nat(7));

        let choice = handler
            .handle_choose("A", "B", &["Commit".to_string(), "Abort".to_string()], &[])
            .expect("choose should succeed");
        assert_eq!(choice, "Commit");
        assert_eq!(handler.drain_envelopes().len(), 2);
    }

    #[test]
    fn denied_layer_blocks_acquire() {
        let handler = AuraVmEffectHandler::default();
        handler.deny_layer("cap.guard");

        let decision = handler
            .handle_acquire(1, "A", "cap.guard", &[])
            .expect("acquire should return decision");
        assert!(matches!(decision, AcquireDecision::Block));
        assert_eq!(handler.telemetry().acquire_denied, 1);
    }

    #[test]
    fn acquire_release_tracks_scope_mapped_leases() {
        let handler = AuraVmEffectHandler::default();
        let authority = AuthorityId::from_uuid(Uuid::from_bytes([7u8; 16]));
        let scope = ResourceScope::Authority {
            authority_id: authority,
            operation: AuthorityOp::Rotate,
        };
        handler.bind_layer_scope("cap.guard", scope.clone());

        let acquire = handler
            .handle_acquire(42, "Coordinator", "cap.guard", &[])
            .expect("acquire should succeed");
        assert!(matches!(acquire, AcquireDecision::Grant(_)));

        let leases = handler.active_leases();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].scope, Some(scope));

        handler
            .handle_release(42, "Coordinator", "cap.guard", &Value::Unit, &[])
            .expect("release should succeed");
        assert!(handler.active_leases().is_empty());
    }

    #[test]
    fn release_without_lease_is_deterministic_fault() {
        let handler = AuraVmEffectHandler::default();
        let err = handler
            .handle_release(99, "Coordinator", "cap.guard", &Value::Unit, &[])
            .expect_err("release without prior acquire should fault");
        assert!(err.contains("release without active lease"));
        assert_eq!(handler.telemetry().release_faults, 1);
    }

    #[test]
    fn guard_contention_snapshot_surfaces_failure_counters() {
        let handler = AuraVmEffectHandler::default();
        handler.deny_layer("cap.guard");
        let _ = handler
            .handle_acquire(7, "A", "cap.guard", &[])
            .expect("acquire decision");
        let _ = handler.handle_release(7, "A", "cap.guard", &Value::Unit, &[]);

        let snapshot = handler.guard_contention_snapshot();
        assert_eq!(snapshot.get("acquire_denied"), Some(&1));
        assert_eq!(snapshot.get("release_faults"), Some(&1));
    }

    #[test]
    fn topology_schedule_is_sorted_and_drained_per_tick() {
        let handler = AuraVmEffectHandler::default();
        handler.schedule_topology_events(
            3,
            vec![
                TopologyPerturbation::Timeout {
                    site: "node-c".to_string(),
                    duration: Duration::from_millis(50),
                },
                TopologyPerturbation::Partition {
                    from: "node-b".to_string(),
                    to: "node-c".to_string(),
                },
                TopologyPerturbation::Crash {
                    site: "node-a".to_string(),
                },
                TopologyPerturbation::Corrupt {
                    from: "node-d".to_string(),
                    to: "node-e".to_string(),
                    corruption: CorruptionType::PayloadErase,
                },
            ],
        );

        let first = handler.topology_events(3).expect("scheduled events");
        assert_eq!(first.len(), 4);
        assert_eq!(
            first
                .iter()
                .map(TopologyPerturbation::ordering_key)
                .collect::<Vec<_>>(),
            {
                let mut keys = first
                    .iter()
                    .map(TopologyPerturbation::ordering_key)
                    .collect::<Vec<_>>();
                keys.sort();
                keys
            }
        );

        let drained = handler.topology_events(3).expect("drained tick");
        assert!(drained.is_empty());
    }

    #[test]
    fn topology_schedule_is_isolated_by_tick() {
        let handler = AuraVmEffectHandler::default();
        handler.schedule_topology_event(
            1,
            TopologyPerturbation::Crash {
                site: "tick-one".to_string(),
            },
        );
        handler.schedule_topology_event(
            2,
            TopologyPerturbation::Crash {
                site: "tick-two".to_string(),
            },
        );

        let tick_one = handler.topology_events(1).expect("tick one");
        let tick_two = handler.topology_events(2).expect("tick two");
        assert_eq!(tick_one.len(), 1);
        assert_eq!(tick_two.len(), 1);
        assert_eq!(
            tick_one[0],
            TopologyPerturbation::Crash {
                site: "tick-one".to_string()
            }
        );
        assert_eq!(
            tick_two[0],
            TopologyPerturbation::Crash {
                site: "tick-two".to_string()
            }
        );
    }
}
