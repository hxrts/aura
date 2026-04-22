use aura_agent::core::AuthorityContext;
use aura_agent::handlers::{AuthHandler, AuthMethod, AuthResponse};
use aura_agent::AuraEffectSystem;
use aura_core::{ContextId, DeviceId, Hash32};
use aura_guards::{DecodedIngress, IngressSource, VerifiedIngressMetadata};

fn effects() -> &'static AuraEffectSystem {
    unimplemented!()
}

fn main() {
    let authority = aura_core::AuthorityId::new_from_entropy([1; 32]);
    let handler = AuthHandler::new(AuthorityContext::new_with_device(
        authority,
        DeviceId::new_from_entropy([3; 32]),
    ))
    .unwrap();
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Authority(authority),
        ContextId::new_from_entropy([2; 32]),
        None,
        Hash32::zero(),
        1,
    );
    let decoded = DecodedIngress::new(
        AuthResponse {
            challenge_id: "challenge".to_string(),
            signature: vec![0; 64],
            public_key: vec![0; 32],
            auth_method: AuthMethod::DeviceKey,
        },
        metadata,
    );

    let _ = handler.verify_response(effects(), &decoded);
}
