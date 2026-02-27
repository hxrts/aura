//! Telltale VM hardening profiles and parity-lane configuration for Aura.

use telltale_vm::vm::{FlowPolicy, FlowPredicate, GuardLayerConfig};
use telltale_vm::{
    CommunicationReplayMode, DeterminismMode, EffectDeterminismTier, EffectTraceCaptureMode,
    MonitorMode, OutputConditionPolicy, PayloadValidationMode, SchedPolicy, VMConfig,
};

/// Default output predicate when no operation-specific hint is provided.
pub const AURA_OUTPUT_PREDICATE_OBSERVABLE: &str = "aura.observable_output";
/// Legacy VM default output predicate retained for compatibility.
pub const AURA_OUTPUT_PREDICATE_VM_OBSERVABLE: &str = "vm.observable_output";
/// Output predicate used for transport send visibility.
pub const AURA_OUTPUT_PREDICATE_TRANSPORT_SEND: &str = "aura.transport.send";
/// Output predicate used for transport receive visibility.
pub const AURA_OUTPUT_PREDICATE_TRANSPORT_RECV: &str = "aura.transport.recv";
/// Output predicate used for choice/branch visibility.
pub const AURA_OUTPUT_PREDICATE_CHOICE: &str = "aura.protocol.choice";
/// Output predicate used for invoke/step visibility.
pub const AURA_OUTPUT_PREDICATE_STEP: &str = "aura.protocol.step";
/// Output predicate used for guard acquire visibility.
pub const AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE: &str = "aura.guard.acquire";
/// Output predicate used for guard release visibility.
pub const AURA_OUTPUT_PREDICATE_GUARD_RELEASE: &str = "aura.guard.release";

/// Typed guard-layer identifiers reserved by Aura runtime profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuraVmGuardLayer {
    /// Serializes migration/reconfiguration critical sections.
    MigrationLock,
    /// Reserves scarce slots for high-value protocol operations.
    HighValueProtocolSlot,
}

impl AuraVmGuardLayer {
    /// Stable VM layer identifier.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::MigrationLock => "aura.guard.migration_lock",
            Self::HighValueProtocolSlot => "aura.guard.high_value_protocol_slot",
        }
    }

    /// All reserved guard layers configured by Aura VM profiles.
    #[must_use]
    pub fn all() -> [Self; 2] {
        [Self::MigrationLock, Self::HighValueProtocolSlot]
    }
}

/// Profile-level VM hardening in Aura runtime contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraVmHardeningProfile {
    /// Local developer profile: strict assertions with verbose diagnostics.
    Dev,
    /// CI profile: strictest commit/flow gates and replay-consumption checks.
    Ci,
    /// Production profile: safety-preserving defaults with bounded overhead.
    #[default]
    Prod,
}

impl AuraVmHardeningProfile {
    /// Parse profile from a stable textual identifier.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" => Some(Self::Dev),
            "ci" => Some(Self::Ci),
            "prod" | "production" => Some(Self::Prod),
            _ => None,
        }
    }
}

/// Cross-target parity lane profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuraVmParityProfile {
    /// Native cooperative scheduler lane.
    NativeCooperative,
    /// Native threaded lane.
    NativeThreaded,
    /// WASM cooperative lane.
    WasmCooperative,
    /// Runtime default lane for non-parity production execution.
    #[default]
    RuntimeDefault,
}

/// Determinism profile parse/validation error.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuraVmDeterminismProfileError {
    /// Unknown determinism mode string.
    #[error("unknown determinism mode: {raw}")]
    UnknownDeterminismMode {
        /// Invalid input value.
        raw: String,
    },
    /// Unknown effect determinism tier string.
    #[error("unknown effect determinism tier: {raw}")]
    UnknownEffectDeterminismTier {
        /// Invalid input value.
        raw: String,
    },
    /// Unknown communication replay mode string.
    #[error("unknown communication replay mode: {raw}")]
    UnknownCommunicationReplayMode {
        /// Invalid input value.
        raw: String,
    },
    /// Invalid profile combination in VM config.
    #[error(
        "invalid determinism profile combination: mode={mode:?}, tier={tier:?}, replay={replay:?} ({reason})"
    )]
    InvalidCombination {
        /// Determinism mode.
        mode: DeterminismMode,
        /// Effect determinism tier.
        tier: EffectDeterminismTier,
        /// Communication replay mode.
        replay: CommunicationReplayMode,
        /// Human-readable reason.
        reason: &'static str,
    },
}

/// Parse `DeterminismMode` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownDeterminismMode`] for unknown values.
pub fn parse_determinism_mode(raw: &str) -> Result<DeterminismMode, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "full" => Ok(DeterminismMode::Full),
        "modulo_effects" | "modulo-effects" => Ok(DeterminismMode::ModuloEffects),
        "modulo_commutativity" | "modulo-commutativity" => Ok(DeterminismMode::ModuloCommutativity),
        "replay" => Ok(DeterminismMode::Replay),
        _ => Err(AuraVmDeterminismProfileError::UnknownDeterminismMode {
            raw: raw.to_string(),
        }),
    }
}

/// Parse `EffectDeterminismTier` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownEffectDeterminismTier`] for unknown values.
pub fn parse_effect_determinism_tier(
    raw: &str,
) -> Result<EffectDeterminismTier, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "strict_deterministic" | "strict-deterministic" => {
            Ok(EffectDeterminismTier::StrictDeterministic)
        }
        "replay_deterministic" | "replay-deterministic" => {
            Ok(EffectDeterminismTier::ReplayDeterministic)
        }
        "envelope_bounded_nondeterministic" | "envelope-bounded-nondeterministic" => {
            Ok(EffectDeterminismTier::EnvelopeBoundedNondeterministic)
        }
        _ => Err(
            AuraVmDeterminismProfileError::UnknownEffectDeterminismTier {
                raw: raw.to_string(),
            },
        ),
    }
}

/// Parse `CommunicationReplayMode` from a stable textual identifier.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::UnknownCommunicationReplayMode`] for unknown values.
pub fn parse_communication_replay_mode(
    raw: &str,
) -> Result<CommunicationReplayMode, AuraVmDeterminismProfileError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Ok(CommunicationReplayMode::Off),
        "sequence" => Ok(CommunicationReplayMode::Sequence),
        "nullifier" => Ok(CommunicationReplayMode::Nullifier),
        _ => Err(
            AuraVmDeterminismProfileError::UnknownCommunicationReplayMode {
                raw: raw.to_string(),
            },
        ),
    }
}

/// Validate determinism/profile requirements encoded in VM config.
///
/// # Errors
///
/// Returns [`AuraVmDeterminismProfileError::InvalidCombination`] when the configured profile is
/// inconsistent with Aura admission constraints.
pub fn validate_determinism_profile(
    config: &VMConfig,
) -> Result<(), AuraVmDeterminismProfileError> {
    let mode = config.determinism_mode;
    let tier = config.effect_determinism_tier;
    let replay = config.communication_replay_mode;

    if mode == DeterminismMode::Full && tier != EffectDeterminismTier::StrictDeterministic {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "full determinism requires strict effect determinism",
        });
    }

    if tier == EffectDeterminismTier::EnvelopeBoundedNondeterministic
        && !matches!(
            mode,
            DeterminismMode::ModuloEffects | DeterminismMode::ModuloCommutativity
        )
    {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "envelope-bounded effect tier requires a modulo determinism mode",
        });
    }

    if replay == CommunicationReplayMode::Nullifier && mode == DeterminismMode::Full {
        return Err(AuraVmDeterminismProfileError::InvalidCombination {
            mode,
            tier,
            replay,
            reason: "nullifier replay mode is incompatible with full determinism mode",
        });
    }

    Ok(())
}

/// Output predicates accepted by Aura hardening policies.
#[must_use]
pub fn aura_output_predicate_allow_list() -> Vec<String> {
    [
        AURA_OUTPUT_PREDICATE_OBSERVABLE,
        AURA_OUTPUT_PREDICATE_VM_OBSERVABLE,
        AURA_OUTPUT_PREDICATE_TRANSPORT_SEND,
        AURA_OUTPUT_PREDICATE_TRANSPORT_RECV,
        AURA_OUTPUT_PREDICATE_CHOICE,
        AURA_OUTPUT_PREDICATE_STEP,
        AURA_OUTPUT_PREDICATE_GUARD_ACQUIRE,
        AURA_OUTPUT_PREDICATE_GUARD_RELEASE,
    ]
    .iter()
    .map(ToString::to_string)
    .collect()
}

fn aura_guard_layers() -> Vec<GuardLayerConfig> {
    AuraVmGuardLayer::all()
        .into_iter()
        .map(|layer| GuardLayerConfig {
            id: layer.id().to_string(),
            active: true,
        })
        .collect()
}

/// Serializable flow-policy predicate for Aura role/category constraints.
#[must_use]
pub fn aura_flow_policy_predicate() -> FlowPredicate {
    FlowPredicate::Any(vec![
        FlowPredicate::TargetRolePrefix("Proposer".to_string()),
        FlowPredicate::TargetRolePrefix("Witness".to_string()),
        FlowPredicate::TargetRolePrefix("Observer".to_string()),
        FlowPredicate::TargetRolePrefix("Committer".to_string()),
        FlowPredicate::TargetRolePrefix("Issuer".to_string()),
        FlowPredicate::TargetRolePrefix("Invitee".to_string()),
        FlowPredicate::TargetRolePrefix("Inviter".to_string()),
        FlowPredicate::TargetRolePrefix("Requester".to_string()),
        FlowPredicate::TargetRolePrefix("Guardian".to_string()),
        FlowPredicate::TargetRolePrefix("Primary".to_string()),
        FlowPredicate::TargetRolePrefix("Replica".to_string()),
        FlowPredicate::TargetRolePrefix("Sync".to_string()),
        FlowPredicate::TargetRolePrefix("Relay".to_string()),
        FlowPredicate::TargetRolePrefix("Context".to_string()),
    ])
}

fn apply_hardening_profile(config: &mut VMConfig, profile: AuraVmHardeningProfile) {
    config.monitor_mode = MonitorMode::SessionTypePrecheck;
    config.host_contract_assertions = true;
    config.output_condition_policy =
        OutputConditionPolicy::PredicateAllowList(aura_output_predicate_allow_list());
    config.flow_policy = FlowPolicy::PredicateExpr(aura_flow_policy_predicate());
    config.guard_layers = aura_guard_layers();
    config.payload_validation_mode = PayloadValidationMode::Structural;
    config.effect_trace_capture_mode = EffectTraceCaptureMode::Full;
    config.max_payload_bytes = 256 * 1024;

    match profile {
        AuraVmHardeningProfile::Dev => {
            config.communication_replay_mode = CommunicationReplayMode::Off;
            config.sched_policy = SchedPolicy::Cooperative;
        }
        AuraVmHardeningProfile::Ci => {
            config.communication_replay_mode = CommunicationReplayMode::Sequence;
            // Keep CI strict without requiring schema annotations on every test choreography.
            config.payload_validation_mode = PayloadValidationMode::Structural;
        }
        AuraVmHardeningProfile::Prod => {
            config.communication_replay_mode = CommunicationReplayMode::Off;
            config.effect_trace_capture_mode = EffectTraceCaptureMode::TopologyOnly;
        }
    }
}

fn apply_parity_profile(config: &mut VMConfig, parity: AuraVmParityProfile) {
    match parity {
        AuraVmParityProfile::NativeCooperative | AuraVmParityProfile::WasmCooperative => {
            config.sched_policy = SchedPolicy::Cooperative;
            config.determinism_mode = DeterminismMode::Full;
            config.effect_determinism_tier = EffectDeterminismTier::StrictDeterministic;
        }
        AuraVmParityProfile::NativeThreaded => {
            config.sched_policy = SchedPolicy::RoundRobin;
            config.determinism_mode = DeterminismMode::ModuloCommutativity;
            config.effect_determinism_tier = EffectDeterminismTier::ReplayDeterministic;
        }
        AuraVmParityProfile::RuntimeDefault => {}
    }
}

/// Build a VM config with explicit hardening and parity profiles.
#[must_use]
pub fn build_vm_config(hardening: AuraVmHardeningProfile, parity: AuraVmParityProfile) -> VMConfig {
    let mut config = VMConfig::default();
    apply_hardening_profile(&mut config, hardening);
    apply_parity_profile(&mut config, parity);
    config
}

/// Build a VM config from hardening profile only.
#[must_use]
pub fn vm_config_for_profile(hardening: AuraVmHardeningProfile) -> VMConfig {
    build_vm_config(hardening, AuraVmParityProfile::RuntimeDefault)
}

#[cfg(test)]
mod tests {
    use super::*;
    use telltale_types::{GlobalType, Label};
    use telltale_vm::coroutine::KnowledgeFact;
    use telltale_vm::effect::EffectHandler;
    use telltale_vm::instr::Endpoint;
    use telltale_vm::loader::CodeImage;
    use telltale_vm::vm::{RunStatus, VM};
    use telltale_vm::{verify_output_condition, OutputConditionMeta, Value};

    #[derive(Default)]
    struct UnitHandler;

    impl EffectHandler for UnitHandler {
        fn handle_send(
            &self,
            _role: &str,
            _partner: &str,
            _label: &str,
            _state: &[Value],
        ) -> Result<Value, String> {
            Ok(Value::Unit)
        }

        fn handle_recv(
            &self,
            _role: &str,
            _partner: &str,
            _label: &str,
            _state: &mut Vec<Value>,
            _payload: &Value,
        ) -> Result<(), String> {
            Ok(())
        }

        fn handle_choose(
            &self,
            _role: &str,
            _partner: &str,
            labels: &[String],
            _state: &[Value],
        ) -> Result<String, String> {
            labels
                .first()
                .cloned()
                .ok_or_else(|| "no labels available".to_string())
        }

        fn step(&self, _role: &str, _state: &mut Vec<Value>) -> Result<(), String> {
            Ok(())
        }
    }

    fn flow_compatibility_image() -> CodeImage {
        let global = GlobalType::send(
            "Primary",
            "Replica",
            Label::new("delta"),
            GlobalType::send("Replica", "Primary", Label::new("receipt"), GlobalType::End),
        );
        let locals = telltale_theory::projection::project_all(&global)
            .expect("projection must succeed")
            .into_iter()
            .collect::<std::collections::BTreeMap<_, _>>();
        CodeImage::from_local_types(&locals, &global)
    }

    #[test]
    fn profile_parser_handles_known_values() {
        assert_eq!(
            AuraVmHardeningProfile::parse("dev"),
            Some(AuraVmHardeningProfile::Dev)
        );
        assert_eq!(
            AuraVmHardeningProfile::parse("CI"),
            Some(AuraVmHardeningProfile::Ci)
        );
        assert_eq!(
            AuraVmHardeningProfile::parse("production"),
            Some(AuraVmHardeningProfile::Prod)
        );
        assert_eq!(AuraVmHardeningProfile::parse("unknown"), None);
    }

    #[test]
    fn ci_profile_denies_unknown_output_predicates() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let unknown = OutputConditionMeta {
            predicate_ref: "aura.unknown".to_string(),
            witness_ref: Some("witness-x".to_string()),
            output_digest: "digest".to_string(),
        };
        assert!(
            !verify_output_condition(&config.output_condition_policy, &unknown),
            "ci profile must reject unknown output predicates"
        );

        let known = OutputConditionMeta {
            predicate_ref: AURA_OUTPUT_PREDICATE_TRANSPORT_SEND.to_string(),
            witness_ref: None,
            output_digest: "digest".to_string(),
        };
        assert!(
            verify_output_condition(&config.output_condition_policy, &known),
            "ci profile must admit known output predicates"
        );
    }

    #[test]
    fn flow_policy_predicate_blocks_unclassified_targets() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let fact = KnowledgeFact {
            endpoint: Endpoint {
                sid: 1,
                role: "Proposer".to_string(),
            },
            fact: "commit-ready".to_string(),
        };
        assert!(config.flow_policy.allows_knowledge(&fact, "GuardianAlpha"));
        assert!(!config.flow_policy.allows_knowledge(&fact, "UnknownRole"));
    }

    #[test]
    fn prod_profile_keeps_bounded_overhead_defaults() {
        let config = build_vm_config(
            AuraVmHardeningProfile::Prod,
            AuraVmParityProfile::RuntimeDefault,
        );
        assert_eq!(
            config.effect_trace_capture_mode,
            EffectTraceCaptureMode::TopologyOnly
        );
        assert_eq!(
            config.communication_replay_mode,
            CommunicationReplayMode::Off
        );
        assert_eq!(config.monitor_mode, MonitorMode::SessionTypePrecheck);
        let guard_layer_ids = config
            .guard_layers
            .iter()
            .map(|layer| layer.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert!(guard_layer_ids.contains(AuraVmGuardLayer::MigrationLock.id()));
        assert!(guard_layer_ids.contains(AuraVmGuardLayer::HighValueProtocolSlot.id()));
    }

    #[test]
    fn flow_policy_preserves_send_recv_progression_for_allowed_roles() {
        let image = flow_compatibility_image();
        let handler = UnitHandler;

        let ci_config = build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        );
        let mut allow_all_config = ci_config.clone();
        allow_all_config.flow_policy = FlowPolicy::AllowAll;

        let mut ci_vm = VM::new(ci_config);
        ci_vm.load_choreography(&image).expect("load choreography");
        let ci_status = ci_vm.run(&handler, 64).expect("ci run");
        assert_eq!(ci_status, RunStatus::AllDone);

        let mut allow_all_vm = VM::new(allow_all_config);
        allow_all_vm
            .load_choreography(&image)
            .expect("load choreography");
        let allow_status = allow_all_vm.run(&handler, 64).expect("allow-all run");
        assert_eq!(allow_status, RunStatus::AllDone);

        let ci_effect_kinds = ci_vm
            .effect_trace()
            .iter()
            .map(|entry| entry.effect_kind.clone())
            .collect::<Vec<_>>();
        let allow_effect_kinds = allow_all_vm
            .effect_trace()
            .iter()
            .map(|entry| entry.effect_kind.clone())
            .collect::<Vec<_>>();

        assert_eq!(
            ci_effect_kinds, allow_effect_kinds,
            "Aura flow policy must preserve send/recv progression for allowed roles"
        );
    }

    #[test]
    fn determinism_profile_parsers_handle_known_values() {
        assert_eq!(
            parse_determinism_mode("modulo-commutativity"),
            Ok(DeterminismMode::ModuloCommutativity)
        );
        assert_eq!(
            parse_effect_determinism_tier("replay_deterministic"),
            Ok(EffectDeterminismTier::ReplayDeterministic)
        );
        assert_eq!(
            parse_communication_replay_mode("sequence"),
            Ok(CommunicationReplayMode::Sequence)
        );
        assert!(matches!(
            parse_determinism_mode("unknown"),
            Err(AuraVmDeterminismProfileError::UnknownDeterminismMode { .. })
        ));
    }

    #[test]
    fn determinism_profile_validation_rejects_incompatible_combinations() {
        let mut config = VMConfig {
            determinism_mode: DeterminismMode::Full,
            effect_determinism_tier: EffectDeterminismTier::ReplayDeterministic,
            communication_replay_mode: CommunicationReplayMode::Off,
            ..VMConfig::default()
        };
        assert!(matches!(
            validate_determinism_profile(&config),
            Err(AuraVmDeterminismProfileError::InvalidCombination { .. })
        ));

        config.effect_determinism_tier = EffectDeterminismTier::StrictDeterministic;
        assert!(validate_determinism_profile(&config).is_ok());
    }
}
