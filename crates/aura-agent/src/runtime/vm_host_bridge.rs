use crate::runtime::{
    build_vm_config, AuraChoreoEngine, AuraEffectSystem, AuraVmHardeningProfile,
    AuraVmParityProfile,
};
use crate::runtime::vm_hardening::{
    apply_protocol_execution_policy, apply_scheduler_execution_policy,
    configured_guard_capacity, policy_for_protocol, scheduler_control_input_for_image,
    scheduler_policy_for_input, AuraVmSchedulerSignals, AuraVmSchedulerSignalsProvider,
};
use aura_mpst::CompositionManifest;
use aura_mpst::telltale_types::{GlobalType, LocalTypeR};
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole, ChoreographyError};
use parking_lot::Mutex;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use telltale_vm::coroutine::{BlockReason, CoroStatus, Value};
use telltale_vm::effect::EffectHandler;
use telltale_vm::loader::CodeImage;
use telltale_vm::runtime_contracts::RuntimeContracts;
use telltale_vm::vm::VM;
use telltale_vm::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingVmSend {
    pub from_role: String,
    pub to_role: String,
    pub label: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedVmReceive {
    pub from_role: String,
    pub to_role: String,
    pub peer_role: ChoreographicRole,
    pub payload: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct AuraQueuedVmBridgeHandler {
    outbound_values: Mutex<VecDeque<Value>>,
    choice_labels: Mutex<VecDeque<String>>,
    pending_sends: Mutex<VecDeque<PendingVmSend>>,
    scheduler_signals: Mutex<AuraVmSchedulerSignals>,
}

impl AuraQueuedVmBridgeHandler {
    pub fn push_send_bytes(&self, payload: Vec<u8>) {
        self.outbound_values
            .lock()
            .push_back(Value::Str(hex::encode(payload)));
    }

    pub fn drain_pending_sends(&self) -> Vec<PendingVmSend> {
        let mut guard = self.pending_sends.lock();
        guard.drain(..).collect()
    }

    #[allow(dead_code)]
    pub fn push_choice_label(&self, label: impl Into<String>) {
        self.choice_labels.lock().push_back(label.into());
    }

    pub fn value_to_bytes(value: &Value) -> Result<Vec<u8>, String> {
        match value {
            Value::Unit => Ok(Vec::new()),
            Value::Str(encoded) => {
                hex::decode(encoded).map_err(|error| format!("invalid bridged payload: {error}"))
            }
            other => Err(format!(
                "unsupported VM bridge payload type: {other:?}; expected hex string"
            )),
        }
    }

    pub fn bytes_to_value(payload: &[u8]) -> Value {
        Value::Str(hex::encode(payload))
    }

    #[allow(dead_code)]
    pub fn set_scheduler_signals(&self, signals: AuraVmSchedulerSignals) {
        *self.scheduler_signals.lock() = signals.normalized();
    }
}

impl AuraVmSchedulerSignalsProvider for AuraQueuedVmBridgeHandler {
    fn scheduler_signals(&self) -> AuraVmSchedulerSignals {
        *self.scheduler_signals.lock()
    }
}

impl EffectHandler for AuraQueuedVmBridgeHandler {
    fn handler_identity(&self) -> String {
        "aura-vm-host-bridge".to_string()
    }

    fn handle_send(
        &self,
        role: &str,
        partner: &str,
        label: &str,
        _state: &[Value],
    ) -> Result<Value, String> {
        let payload = self.outbound_values.lock().pop_front().ok_or_else(|| {
            format!("missing queued outbound payload for VM send {role}->{partner}:{label}")
        })?;
        let payload_bytes = Self::value_to_bytes(&payload)?;
        self.pending_sends.lock().push_back(PendingVmSend {
            from_role: role.to_string(),
            to_role: partner.to_string(),
            label: label.to_string(),
            payload: payload_bytes,
        });
        Ok(payload)
    }

    fn handle_recv(
        &self,
        _role: &str,
        _partner: &str,
        _label: &str,
        state: &mut Vec<Value>,
        payload: &Value,
    ) -> Result<(), String> {
        if let Some(last) = state.last_mut() {
            *last = payload.clone();
        } else {
            state.push(payload.clone());
        }
        Ok(())
    }

    fn handle_choose(
        &self,
        _role: &str,
        _partner: &str,
        labels: &[String],
        _state: &[Value],
    ) -> Result<String, String> {
        if let Some(choice) = self.choice_labels.lock().pop_front() {
            if labels.iter().any(|label| label == &choice) {
                return Ok(choice);
            }
            return Err(format!(
                "queued VM choice {choice} not present in offered labels {labels:?}"
            ));
        }
        labels
            .first()
            .cloned()
            .ok_or_else(|| "no labels available for VM bridge choice".to_string())
    }

    fn step(&self, _role: &str, _state: &mut Vec<Value>) -> Result<(), String> {
        Ok(())
    }
}

pub fn build_role_scoped_code_image(
    roles: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
) -> Result<CodeImage, String> {
    let mut scoped = BTreeMap::new();
    for role in roles {
        if *role == active_role {
            let local_type = local_types
                .get(*role)
                .cloned()
                .ok_or_else(|| format!("missing VM local type for active role {active_role}"))?;
            scoped.insert((*role).to_string(), local_type);
        } else {
            scoped.insert((*role).to_string(), LocalTypeR::End);
        }
    }
    Ok(CodeImage::from_local_types(&scoped, global_type))
}

#[cfg(test)]
fn open_role_scoped_vm_session(
    role_names: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    String,
> {
    let image = build_role_scoped_code_image(role_names, active_role, global_type, local_types)?;
    let handler = Arc::new(AuraQueuedVmBridgeHandler::default());
    let config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    let mut engine = AuraChoreoEngine::new_with_contracts(
        config,
        Arc::clone(&handler),
        Some(RuntimeContracts::full()),
    )
    .map_err(|error| format!("failed to create VM engine: {error}"))?;
    let sid = engine
        .open_session(&image)
        .map_err(|error| format!("failed to open VM session: {error}"))?;
    Ok((engine, handler, sid))
}

pub async fn open_role_scoped_vm_session_admitted(
    role_names: &[&str],
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
    protocol_id: &str,
    determinism_policy_ref: Option<&str>,
    scheduler_signals: AuraVmSchedulerSignals,
    required_capabilities: &[&str],
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    String,
> {
    let image = build_role_scoped_code_image(role_names, active_role, global_type, local_types)?;
    let handler = Arc::new(AuraQueuedVmBridgeHandler::default());
    handler.set_scheduler_signals(scheduler_signals);
    let mut config = build_vm_config(
        AuraVmHardeningProfile::Prod,
        AuraVmParityProfile::RuntimeDefault,
    );
    let policy = policy_for_protocol(protocol_id, determinism_policy_ref)
        .map_err(|error| format!("failed to resolve VM protocol policy: {error}"))?;
    apply_protocol_execution_policy(&mut config, policy);
    let scheduler_input = scheduler_control_input_for_image(
        &image,
        policy.protocol_class,
        configured_guard_capacity(&config),
        handler.scheduler_signals(),
    );
    let scheduler_policy = scheduler_policy_for_input(scheduler_input);
    apply_scheduler_execution_policy(&mut config, &scheduler_policy);
    let mut engine = AuraChoreoEngine::new_with_contracts(
        config,
        Arc::clone(&handler),
        Some(RuntimeContracts::full()),
    )
    .map_err(|error| format!("failed to create VM engine: {error}"))?;
    let sid = engine
        .open_session_admitted(&image, protocol_id, determinism_policy_ref, required_capabilities)
        .await
        .map_err(|error| format!("failed to open VM session: {error}"))?;
    Ok((engine, handler, sid))
}

pub async fn open_manifest_vm_session_admitted(
    manifest: &CompositionManifest,
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
    scheduler_signals: AuraVmSchedulerSignals,
) -> Result<
    (
        AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
        Arc<AuraQueuedVmBridgeHandler>,
        SessionId,
    ),
    String,
> {
    let role_names = manifest
        .role_names
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let required_capabilities = manifest
        .required_capabilities
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    open_role_scoped_vm_session_admitted(
        role_names.as_slice(),
        active_role,
        global_type,
        local_types,
        manifest.protocol_id.as_str(),
        manifest.determinism_policy_ref.as_deref(),
        scheduler_signals,
        required_capabilities.as_slice(),
    )
    .await
}

pub async fn flush_pending_vm_sends(
    effects: &AuraEffectSystem,
    handler: &AuraQueuedVmBridgeHandler,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<(), String> {
    for pending in handler.drain_pending_sends() {
        let peer_role = peer_roles.get(&pending.to_role).copied().ok_or_else(|| {
            format!(
                "missing peer mapping for VM send target role {}",
                pending.to_role
            )
        })?;
        effects
            .send_to_role_bytes(peer_role, pending.payload)
            .await
            .map_err(|error| {
                format!(
                    "failed to bridge VM send {}->{}:{}: {error}",
                    pending.from_role, pending.to_role, pending.label
                )
            })?;
    }
    Ok(())
}

pub fn blocked_recv_edge(vm: &VM, sid: SessionId, role: &str) -> Option<(String, String)> {
    vm.coroutines().iter().find_map(|coro| {
        if coro.session_id != sid || coro.role != role {
            return None;
        }
        match &coro.status {
            CoroStatus::Blocked(BlockReason::Recv { edge, .. }) => {
                Some((edge.sender.clone(), edge.receiver.clone()))
            }
            _ => None,
        }
    })
}

pub async fn receive_blocked_vm_message(
    effects: &AuraEffectSystem,
    vm: &VM,
    sid: SessionId,
    active_role: &str,
    peer_roles: &BTreeMap<String, ChoreographicRole>,
) -> Result<Option<BlockedVmReceive>, ChoreographyError> {
    let Some((from_role, to_role)) = blocked_recv_edge(vm, sid, active_role) else {
        return Ok(None);
    };
    let peer_role = peer_roles
        .get(&from_role)
        .copied()
        .or_else(|| peer_roles.get(&to_role).copied())
        .ok_or_else(|| ChoreographyError::InternalError {
            message: format!("missing peer mapping for blocked VM edge {from_role}->{to_role}"),
        })?;
    let payload = effects.receive_from_role_bytes(peer_role).await?;
    Ok(Some(BlockedVmReceive {
        from_role,
        to_role,
        peer_role,
        payload,
    }))
}

pub fn inject_vm_receive(
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
    receive: &BlockedVmReceive,
) -> Result<(), String> {
    engine
        .vm_mut()
        .inject_message(
            sid,
            &receive.from_role,
            &receive.to_role,
            AuraQueuedVmBridgeHandler::bytes_to_value(&receive.payload),
        )
        .map(|_| ())
        .map_err(|error| format!("failed to inject VM message: {error}"))
}

pub fn close_and_reap_vm_session(
    engine: &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    sid: SessionId,
) -> Result<(), String> {
    engine
        .close_session(sid)
        .map_err(|error| format!("failed to close VM session: {error}"))?;
    let _ = engine.vm_mut().reap_closed_sessions();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_mpst::telltale_types::Label;
    use telltale_vm::session::SessionStatus;

    #[test]
    fn role_scoped_image_keeps_stubbed_peer_edges() {
        let global = GlobalType::send("Sender", "Receiver", Label::new("msg"), GlobalType::End);
        let locals = BTreeMap::from([
            (
                "Sender".to_string(),
                LocalTypeR::send("Receiver", Label::new("msg"), LocalTypeR::End),
            ),
            (
                "Receiver".to_string(),
                LocalTypeR::recv("Sender", Label::new("msg"), LocalTypeR::End),
            ),
        ]);

        let image =
            build_role_scoped_code_image(&["Sender", "Receiver"], "Sender", &global, &locals)
                .expect("image");

        assert_eq!(
            image.roles(),
            vec!["Receiver".to_string(), "Sender".to_string()]
        );
        assert_eq!(image.local_types["Receiver"], LocalTypeR::End);
        assert!(matches!(
            image.local_types["Sender"],
            LocalTypeR::Send { .. }
        ));
    }

    #[test]
    fn queued_bridge_handler_surfaces_pending_send_payloads() {
        let handler = AuraQueuedVmBridgeHandler::default();
        handler.push_send_bytes(vec![0xaa, 0xbb]);

        let payload = handler
            .handle_send("Sender", "Receiver", "InvitationOffer", &[])
            .expect("queued send");
        assert_eq!(
            payload,
            AuraQueuedVmBridgeHandler::bytes_to_value(&[0xaa, 0xbb])
        );

        let sends = handler.drain_pending_sends();
        assert_eq!(sends.len(), 1);
        assert_eq!(sends[0].label, "InvitationOffer");
        assert_eq!(sends[0].payload, vec![0xaa, 0xbb]);
    }

    #[test]
    fn queued_bridge_handler_respects_queued_choice() {
        let handler = AuraQueuedVmBridgeHandler::default();
        handler.push_choice_label("cancel");

        let choice = handler
            .handle_choose(
                "Initiator",
                "Guardian1",
                &["finalize".to_string(), "cancel".to_string()],
                &[],
            )
            .expect("choice");

        assert_eq!(choice, "cancel");
    }

    #[test]
    fn close_and_reap_vm_session_removes_closed_session() {
        let global = GlobalType::End;
        let locals = BTreeMap::from([("Sender".to_string(), LocalTypeR::End)]);
        let (mut engine, _handler, sid) =
            open_role_scoped_vm_session(&["Sender"], "Sender", &global, &locals)
                .expect("session opens");

        assert!(engine.active_sessions().contains(&sid));
        close_and_reap_vm_session(&mut engine, sid).expect("session closes");
        assert!(!engine.active_sessions().contains(&sid));
        assert!(matches!(
            engine
                .vm()
                .sessions()
                .get(sid)
                .map(|session| &session.status),
            Some(SessionStatus::Closed)
        ));
    }
}
