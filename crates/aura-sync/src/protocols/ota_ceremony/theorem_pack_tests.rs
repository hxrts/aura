use super::*;
use aura_protocol::admission::{
    CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE, CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE,
    CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION, CAPABILITY_RECONFIGURATION_SAFETY,
    THEOREM_PACK_AURA_TRANSITION_SAFETY,
};

#[test]
fn proof_status_exposes_required_transition_pack() {
    assert_eq!(
        telltale_session_types_ota_activation::proof_status::REQUIRED_THEOREM_PACKS,
        &[THEOREM_PACK_AURA_TRANSITION_SAFETY]
    );
}

#[test]
fn manifest_emits_transition_safety_pack_metadata() {
    let manifest = telltale_session_types_ota_activation::vm_artifacts::composition_manifest();
    assert_eq!(
        manifest.required_theorem_packs,
        vec![THEOREM_PACK_AURA_TRANSITION_SAFETY.to_string()]
    );
    assert_eq!(
        manifest.required_theorem_pack_capabilities,
        vec![
            CAPABILITY_PROTOCOL_ENVELOPE_BRIDGE.to_string(),
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADHERENCE.to_string(),
            CAPABILITY_PROTOCOL_MACHINE_ENVELOPE_ADMISSION.to_string(),
            CAPABILITY_RECONFIGURATION_SAFETY.to_string(),
        ]
    );
}
