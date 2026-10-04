//! Actual custom builder dispatch, source retention and provider selection contracts.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::expect_used)]
use aura_agent::{AgentBuilder, AgentConfig, AuraAgent};
use aura_core::effects::crypto::SigningMode;
use aura_core::effects::transport::{TransportEnvelope, TransportError};
use aura_core::effects::*;
use aura_core::{AuthorityId, ContextId};
use aura_effects::crypto::RealCryptoHandler;
use aura_effects::time::PhysicalTimeHandler;
use aura_testkit::stateful_effects::custom_provider::{
    CustomCryptoProbe, CustomProviderOutage, CustomProviderProbe,
};
use aura_testkit::stateful_effects::verification_failure_fixture::VerificationFailureFixture;
use std::error::Error;
use std::sync::Arc;

fn config(root: &std::path::Path) -> AgentConfig {
    AgentConfig {
        storage: aura_agent::core::config::StorageConfig {
            base_path: root.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
type CompleteCustomBuilder = aura_agent::builder::CustomPresetBuilder<
    aura_agent::builder::Provided<Arc<dyn CryptoEffects>>,
    aura_agent::builder::Provided<Arc<dyn StorageEffects>>,
    aura_agent::builder::Provided<Arc<dyn PhysicalTimeEffects>>,
    aura_agent::builder::Provided<Arc<dyn RandomEffects>>,
    aura_agent::builder::Provided<Arc<dyn ConsoleEffects>>,
>;

fn builder(
    probe: Arc<CustomProviderProbe>,
    root: &std::path::Path,
    crypto: Arc<dyn CryptoEffects>,
) -> CompleteCustomBuilder {
    AgentBuilder::custom()
        .with_crypto(crypto)
        .with_storage(probe.clone())
        .with_time(Arc::new(PhysicalTimeHandler::new()))
        .with_random(probe.clone())
        .with_console(probe)
        .authority(AuthorityId::new_from_entropy([197; 32]))
        .testing_mode()
        .with_config(config(root))
}
fn envelope() -> TransportEnvelope {
    TransportEnvelope {
        destination: AuthorityId::new_from_entropy([198; 32]),
        source: AuthorityId::new_from_entropy([197; 32]),
        context: ContextId::new_from_entropy([199; 32]),
        payload: vec![4],
        metadata: Default::default(),
        receipt: None,
    }
}
fn has_source<T: Error + 'static>(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.is::<T>() {
            return true;
        }
        current = error.source();
    }
    false
}
async fn verify_selected_dispatch(agent: &AuraAgent, probe: &CustomProviderProbe) {
    let effects = agent.runtime().effects();
    assert!(
        probe.random_draws() >= 1,
        "receipt signing key must use the configured entropy owner before service startup"
    );
    assert_eq!(effects.random_bytes_32().await, [0x93; 32]);
    assert_eq!(effects.random_u64().await, 0x9393_9393_9393_9393);
    for result in [
        effects.log_info("info").await,
        effects.log_warn("warn").await,
        effects.log_error("error").await,
        effects.log_debug("debug").await,
    ] {
        result.expect("configured console call");
    }
    assert_eq!(probe.console_calls(), 4);
    effects
        .store("provider-fidelity", vec![11, 12, 13])
        .await
        .expect("selected encrypted storage");
    assert_eq!(
        effects
            .retrieve("provider-fidelity")
            .await
            .expect("selected read"),
        Some(vec![11, 12, 13])
    );
    let physical = probe.stored_bytes().await;
    assert!(
        !physical.is_empty(),
        "selected underlying provider must contain actual persisted bytes"
    );
    assert!(
        physical.values().all(|bytes| bytes != &vec![11, 12, 13]),
        "runtime must keep encryption around configured storage"
    );
    probe.set_fault(true);
    let storage = effects
        .retrieve("provider-fidelity")
        .await
        .expect_err("configured storage outage must not fall back");
    assert!(has_source::<CustomProviderOutage>(&storage));
    let nested = effects
        .get_flow_budget(
            &ContextId::new_from_entropy([196; 32]),
            &AuthorityId::new_from_entropy([195; 32]),
        )
        .await
        .expect_err("journal owner must retain selected storage failure");
    assert!(has_source::<CustomProviderOutage>(&nested));
    let console = effects
        .log_info("required outage")
        .await
        .expect_err("configured console outage must not become tracing success");
    assert!(has_source::<CustomProviderOutage>(&console));
}
#[tokio::test]
async fn custom_async_builder_retains_selected_handlers_and_native_outages() {
    let root = tempfile::tempdir().expect("fixture root");
    let probe = Arc::new(CustomProviderProbe::default());
    let agent = builder(
        probe.clone(),
        root.path(),
        Arc::new(RealCryptoHandler::new()),
    )
    .build()
    .await
    .expect("actual custom build");
    verify_selected_dispatch(&agent, &probe).await;
}
#[test]
fn custom_sync_builder_retains_selected_handlers_and_native_outages() {
    let root = tempfile::tempdir().expect("fixture root");
    let probe = Arc::new(CustomProviderProbe::default());
    let agent = builder(
        probe.clone(),
        root.path(),
        Arc::new(RealCryptoHandler::new()),
    )
    .build_sync()
    .expect("actual custom synchronous build");
    tokio::runtime::Runtime::new()
        .expect("assertion runtime")
        .block_on(verify_selected_dispatch(&agent, &probe));
}
#[tokio::test]
async fn custom_crypto_failure_and_selected_transport_failure_cannot_fall_back() {
    let root = tempfile::tempdir().expect("fixture root");
    let probe = Arc::new(CustomProviderProbe::default());
    let first = Arc::new(CustomProviderProbe::default());
    let selected = Arc::new(CustomProviderProbe::default());
    selected.set_ready(true);
    selected.set_fault(true);
    let crypto = Arc::new(VerificationFailureFixture::new(
        RealCryptoHandler::new(),
        Arc::new(CustomProviderOutage),
    ));
    let agent = builder(probe, root.path(), crypto)
        .with_transport(first.clone())
        .with_transport(selected.clone())
        .build()
        .await
        .expect("actual custom build");
    let effects = agent.runtime().effects();
    let failure = effects
        .verify_signature(&[], &[], &[], SigningMode::SingleSigner)
        .await
        .expect_err("configured crypto outage must remain native");
    assert!(has_source::<CustomProviderOutage>(&failure));
    let message = envelope();
    let destination = message.destination;
    let repeated = message.clone();
    let movement = effects.move_manager().expect("actual runtime move owner");
    assert!(movement.projection().await.last_flush_ms.is_none());
    assert!(
        matches!(effects.send_envelope(message).await, Err(TransportError::DestinationUnreachable { destination: actual }) if actual == destination)
    );
    assert_eq!(selected.sends(), 1);
    let after_failure = movement.projection().await;
    assert!(
        after_failure.last_flush_ms.is_some(),
        "configured physical dispatch must run under actual move enqueue ownership"
    );
    assert_eq!(
        after_failure.replay_window_entries, 0,
        "delivery failure must release the original replay marker"
    );
    assert_eq!(after_failure.queued_envelopes, 0);
    selected.set_fault(false);
    effects
        .send_envelope(repeated)
        .await
        .expect("retry after actual failed-delivery acknowledgement");
    assert_eq!(selected.sends(), 2);
    assert_eq!(movement.projection().await.replay_window_entries, 0);
    assert_eq!(
        first.sends(),
        0,
        "selected provider error must not trigger another provider or native route"
    );
}

#[tokio::test]
async fn custom_persistent_crypto_uses_same_selected_provider_and_source() {
    let root = tempfile::tempdir().expect("fixture root");
    let storage = Arc::new(CustomProviderProbe::default());
    let crypto = Arc::new(CustomCryptoProbe::default());
    let transport = Arc::new(CustomProviderProbe::default());
    let agent = builder(storage.clone(), root.path(), crypto.clone())
        .with_transport(transport.clone())
        .build()
        .await
        .expect("actual custom build");
    let effects = agent.runtime().effects();
    assert!(effects
        .crypto_capabilities()
        .iter()
        .any(|name| name == "configured-custom-probe"));
    effects
        .store("same-crypto-owner", vec![1, 2, 3])
        .await
        .expect("actual selected encrypted persistence");
    effects
        .send_to_peer(AuthorityId::new_from_entropy([194; 32]).uuid(), vec![7])
        .await
        .expect("network adapter must use configured transport even in testing mode");
    assert_eq!(
        transport.sends(),
        1,
        "custom network adapter cannot return compatibility no-op success"
    );
    transport.set_fault(true);
    let network = effects
        .send_to_peer(AuthorityId::new_from_entropy([194; 32]).uuid(), vec![8])
        .await
        .expect_err("selected network transport failure must remain failed");
    assert!(
        has_source::<TransportError>(&network),
        "nested adapter must retain the actual typed transport cause"
    );
    assert_eq!(transport.sends(), 2);
    crypto.set_fault(true);
    let original = storage.stored_bytes().await;
    let error = effects
        .store("same-crypto-owner", vec![4, 5, 6])
        .await
        .expect_err("configured KDF failure must not select default crypto");
    assert!(has_source::<CustomProviderOutage>(&error));
    assert_eq!(
        storage.stored_bytes().await,
        original,
        "crypto failure must not emit replacement ciphertext"
    );
}

#[tokio::test]
async fn interleaved_custom_ingress_retains_other_content_and_source_context_owner() {
    let root = tempfile::tempdir().expect("fixture root");
    let probe = Arc::new(CustomProviderProbe::default());
    let transport = Arc::new(CustomProviderProbe::default());
    let agent = builder(probe, root.path(), Arc::new(RealCryptoHandler::new()))
        .with_transport(transport.clone())
        .build()
        .await
        .expect("actual configured ingress runtime");
    let effects = agent.runtime().effects();
    let receiver = AuthorityId::new_from_entropy([197; 32]);
    let requested_peer = AuthorityId::new_from_entropy([192; 32]);
    let other_peer = AuthorityId::new_from_entropy([191; 32]);
    let [retained_other, retained_generic, requested] =
        interleaved_frames(receiver, requested_peer, other_peer);
    let other_context = retained_other.context;
    let mut invalid_receipt = requested.clone();
    invalid_receipt.receipt = Some(aura_core::effects::transport::TransportReceipt {
        context: ContextId::new_from_entropy([187; 32]),
        src: requested_peer,
        dst: receiver,
        epoch: 0,
        cost: 0,
        nonce: 0,
        prev: [0; 32],
        sig: Vec::new(),
    });
    let invalid_context = invalid_receipt.context;
    transport.push_inbound(retained_other.clone()).await;
    transport.push_inbound(retained_generic.clone()).await;
    transport.push_inbound(requested).await;
    assert_eq!(
        NetworkExtendedEffects::receive_from(effects.as_ref(), requested_peer.uuid())
            .await
            .expect("matching network owner"),
        vec![33]
    );
    assert_eq!(
        transport.receives(),
        3,
        "unmatched retained frames cannot cycle and starve physical ingress"
    );
    transport.push_inbound(invalid_receipt).await;
    assert!(
        matches!(
            TransportEffects::receive_envelope_from(
                effects.as_ref(),
                requested_peer,
                invalid_context
            )
            .await,
            Err(TransportError::ReceiptValidationFailed { .. })
        ),
        "actual configured receipt mismatch must remain a typed failure before delivery"
    );
    transport.set_fault(true);
    let before = transport.receives();
    let matched =
        TransportEffects::receive_envelope_from(effects.as_ref(), requested_peer, other_context)
            .await
            .expect("retained source/context owner");
    assert_eq!(matched.payload, retained_other.payload);
    assert_eq!(matched.context, other_context);
    assert_eq!(
        transport.receives(),
        before,
        "retained matching owner must not consult failed physical provider"
    );
    let generic = TransportEffects::receive_envelope(effects.as_ref())
        .await
        .expect("retained generic owner");
    assert_eq!(generic.payload, retained_generic.payload);
    assert_eq!(generic.context, retained_generic.context);
    assert_eq!(transport.receives(), before);
    assert!(
        matches!(
            TransportEffects::receive_envelope(effects.as_ref()).await,
            Err(TransportError::ProtocolError { .. })
        ),
        "only actual retained absence permits physical ingress and its original fault"
    );
}

fn interleaved_frames(
    receiver: AuthorityId,
    requested_peer: AuthorityId,
    other_peer: AuthorityId,
) -> [TransportEnvelope; 3] {
    let other_context = ContextId::new_from_entropy([190; 32]);

    let retained_other = TransportEnvelope {
        destination: receiver,
        source: requested_peer,
        context: other_context,
        payload: vec![31],
        metadata: std::collections::HashMap::from([(
            "content-type".into(),
            "application/other-session".into(),
        )]),
        receipt: None,
    };
    let retained_generic = TransportEnvelope {
        destination: receiver,
        source: other_peer,
        context: ContextId::new_from_entropy([189; 32]),
        payload: vec![32],
        metadata: std::collections::HashMap::from([(
            "content-type".into(),
            "application/aura-network".into(),
        )]),
        receipt: None,
    };
    let requested = TransportEnvelope {
        destination: receiver,
        source: requested_peer,
        context: ContextId::new_from_entropy([188; 32]),
        payload: vec![33],
        metadata: std::collections::HashMap::from([(
            "content-type".into(),
            "application/aura-network".into(),
        )]),
        receipt: None,
    };

    [retained_other, retained_generic, requested]
}
