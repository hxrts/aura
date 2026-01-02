use aura_mpst::{extract_aura_annotations, AuraEffect, RoleId};

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
