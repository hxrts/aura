//! Capability soundness tests for guard chain verification

use aura_core::{identifiers::MessageContext, journal::Journal, AuthLevel, Cap};
use aura_protocol::guards::capability::{CapabilityError, EffectRequirement, GuardedContext};

fn context_with_caps(caps: Cap) -> GuardedContext {
    GuardedContext::new(
        Journal::with_caps(caps),
        MessageContext::dkd_context("ctx", [0u8; 32]),
        0,
        AuthLevel::None,
    )
}

#[test]
fn capability_requirement_passes_when_subset() {
    let mut caps = Cap::with_permissions(vec!["caps.send".to_string()]);
    caps.add_resource("journal");
    let ctx = context_with_caps(caps);

    let requirement = EffectRequirement::new()
        .require_permission("caps.send")
        .require_resource("journal");

    assert!(ctx.satisfies_requirement(&requirement).is_ok());
}

#[test]
fn capability_requirement_fails_when_permission_missing() {
    let caps = Cap::with_permissions(vec!["caps.read".to_string()]);
    let ctx = context_with_caps(caps);

    let requirement = EffectRequirement::new().require_permission("caps.write");

    let err = ctx.satisfies_requirement(&requirement).unwrap_err();
    matches!(err, CapabilityError::InsufficientCapabilities { .. });
}

#[test]
fn capability_requirement_fails_when_resource_mismatch() {
    let mut caps = Cap::with_permissions(vec!["caps.send".to_string()]);
    caps.add_resource("journal");
    let ctx = context_with_caps(caps);

    let requirement = EffectRequirement::new()
        .require_permission("caps.send")
        .require_resource("storage");

    let err = ctx.satisfies_requirement(&requirement).unwrap_err();
    matches!(err, CapabilityError::InsufficientCapabilities { .. });
}
