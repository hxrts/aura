//! Annotation extraction contracts — choreography annotations must be
//! extracted completely and in document order. If annotations are lost
//! or reordered, guard checks may be skipped or run in the wrong sequence.

use aura_mpst::{extract_aura_annotations, AuraEffect, RoleId};

/// guard_capability annotation produces GuardCapability effect with the
/// correct capability string and role — if lost, the guard chain skips
/// capability verification.
#[test]
fn guard_capability_annotation_emits_effect() {
    let choreography = r#"
        choreography Guarded {
            roles: Alice, Bob;
            Alice[guard_capability = "send_message"] -> Bob: Message;
        }
    "#;

    let effects = extract_aura_annotations(choreography).expect("extract annotations");
    let has_guard = effects.iter().any(|effect| {
        matches!(
            effect,
            AuraEffect::GuardCapability { capability, role }
                if capability == "send_message" && role == &RoleId::new("Alice")
        )
    });

    assert!(has_guard, "Expected guard capability effect for Alice");
}

/// leak annotation produces Leakage effect with correct observer classes
/// and role — if lost, leakage tracking is bypassed.
#[test]
fn leak_annotation_emits_effect() {
    let choreography = r#"
        choreography Leaky {
            roles: Alice, Bob;
            Alice[leak: (External, Neighbor)] -> Bob: Message;
        }
    "#;

    let effects = extract_aura_annotations(choreography).expect("extract annotations");
    let has_leak = effects.iter().any(|effect| {
        matches!(
            effect,
            AuraEffect::Leakage { observers, role }
                if role == &RoleId::new("Alice")
                    && observers.contains(&"External".to_string())
                    && observers.contains(&"Neighbor".to_string())
        )
    });

    assert!(has_leak, "Expected leakage effect for Alice");
}

/// Multiple annotations on the same send must preserve document order.
/// The guard chain requires capability → flow_cost → journal_facts ordering.
/// If reordered, capability checks happen after budget charges or journal commits.
#[test]
fn multiple_annotations_preserve_document_order() {
    let choreography = r#"
        choreography MultiAnnotated {
            roles: Alice, Bob;
            Alice[guard_capability = "send", flow_cost = 10, leak: (External)] -> Bob: Msg;
        }
    "#;

    let effects = extract_aura_annotations(choreography).expect("extract annotations");

    // Find the indices of each effect type for Alice
    let guard_idx = effects.iter().position(|e| matches!(e, AuraEffect::GuardCapability { .. }));
    let cost_idx = effects.iter().position(|e| matches!(e, AuraEffect::FlowCost { .. }));
    let leak_idx = effects.iter().position(|e| matches!(e, AuraEffect::Leakage { .. }));

    assert!(guard_idx.is_some(), "guard_capability must be extracted");
    assert!(cost_idx.is_some(), "flow_cost must be extracted");
    assert!(leak_idx.is_some(), "leak must be extracted");

    // Document order: guard_capability appears before flow_cost appears before leak
    assert!(
        guard_idx.unwrap() < cost_idx.unwrap(),
        "guard_capability must appear before flow_cost (guard chain ordering)"
    );
    assert!(
        cost_idx.unwrap() < leak_idx.unwrap(),
        "flow_cost must appear before leak (guard chain ordering)"
    );
}
