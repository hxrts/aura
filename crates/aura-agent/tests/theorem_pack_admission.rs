//! Theorem-pack admission coverage for Aura runtime launch.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use aura_agent::{
    build_vm_config, AuraChoreoEngine, AuraChoreoEngineError, AuraVmEffectHandler,
    AuraVmHardeningProfile, AuraVmParityProfile,
};
use aura_mpst::{CompositionManifest, CompositionTheoremPack};
use aura_protocol::admission::{
    CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
    CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, CAPABILITY_RECONFIGURATION_SAFETY,
    THEOREM_PACK_AURA_TRANSITION_SAFETY,
};
use telltale_machine::RuntimeContracts;

fn transition_safety_manifest() -> CompositionManifest {
    CompositionManifest {
        protocol_name: "TransitionSafety".to_string(),
        protocol_namespace: Some("test".to_string()),
        protocol_qualified_name: "test.TransitionSafety".to_string(),
        protocol_id: "test.transition_safety".to_string(),
        role_names: vec!["Coordinator".to_string(), "Worker".to_string()],
        required_capabilities: Vec::new(),
        theorem_packs: vec![CompositionTheoremPack {
            name: THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string(),
            capabilities: vec![
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
                CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
                CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
                CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
            ],
            version: Some("1.0.0".to_string()),
            issuer: Some("did:example:aura".to_string()),
            constraints: vec!["fresh_nonce".to_string()],
        }],
        required_theorem_packs: vec![THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string()],
        required_theorem_pack_capabilities: vec![
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
            CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
            CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
        ],
        guard_capabilities: Vec::new(),
        determinism_policy_ref: Some("aura.vm.prod.default".to_string()),
        link_specs: Vec::new(),
        delegation_constraints: Vec::new(),
    }
}

fn build_engine(
    runtime_contracts: Option<RuntimeContracts>,
) -> AuraChoreoEngine<AuraVmEffectHandler> {
    AuraChoreoEngine::new_with_protocol_machine_contracts(
        build_vm_config(
            AuraVmHardeningProfile::Ci,
            AuraVmParityProfile::NativeCooperative,
        ),
        Arc::new(AuraVmEffectHandler::default()),
        runtime_contracts,
    )
    .expect("engine")
}

#[tokio::test]
async fn theorem_pack_admission_rejects_missing_required_pack() {
    let engine = build_engine(None);
    let mut manifest = transition_safety_manifest();
    manifest.theorem_packs.clear();

    let error = engine
        .admit_manifest(&manifest)
        .await
        .expect_err("missing theorem pack must fail closed");
    assert!(matches!(
        error,
        AuraChoreoEngineError::MissingTheoremPack { theorem_pack }
            if theorem_pack == THEOREM_PACK_AURA_TRANSITION_SAFETY
    ));
}

#[tokio::test]
async fn theorem_pack_admission_rejects_missing_required_capability_coverage() {
    let engine = build_engine(None);
    let manifest = transition_safety_manifest();

    let error = engine
        .admit_manifest(&manifest)
        .await
        .expect_err("missing theorem-pack runtime coverage must fail closed");
    match error {
        AuraChoreoEngineError::MissingTheoremPackCapability {
            theorem_pack,
            capability,
        } => {
            assert_eq!(theorem_pack, THEOREM_PACK_AURA_TRANSITION_SAFETY);
            assert_ne!(capability, "theorem_pack_capabilities");
        }
        other => panic!("unexpected error variant: {other:?}"),
    }
}

#[tokio::test]
async fn theorem_pack_admission_succeeds_when_required_capabilities_are_present() {
    let engine = build_engine(Some(RuntimeContracts::full()));
    let manifest = transition_safety_manifest();

    engine
        .admit_manifest(&manifest)
        .await
        .expect("full runtime contracts should admit theorem-pack requirements");
}
