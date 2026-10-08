//! Regression test: a multifactor ceremony with a device that is not enrolled
//! in the account fails at once with a clear reason (work/8.md Task 200).
//!
//! The TUI demo once let a user add a contact's "mobile device" (a separate
//! authority) and start a multifactor ceremony with it, which failed later with
//! a truncated internal error. A multifactor ceremony re-keys the account's own
//! enrolled devices, whose tree leaves authenticate the rotation; another
//! authority's device joins through device enrollment first. Starting the
//! ceremony with an unenrolled device is now refused before any key material is
//! prepared. Rotation among enrolled devices is covered by the agent's
//! `two_runtime_two_of_two_rotation_signs_with_both_device_shares`.

#![cfg(feature = "development")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args,
    missing_docs
)]

use async_lock::RwLock;
use std::sync::Arc;

use aura_agent::core::{AgentBuilder, AgentConfig};
use aura_agent::EffectContext;
use aura_app::{AppConfig, AppCore};
use aura_core::effects::ExecutionMode;
use aura_core::hash::{self};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::types::FrostThreshold;
use aura_journal::DomainFact;
use aura_relational::ContactFact;
use aura_terminal::ids;
use aura_terminal::tui::context::InitializedAppCore;

#[allow(clippy::duplicate_mod)]
#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn multifactor_ceremony_refuses_a_contacts_unenrolled_device() {
    let seed = 3024u64;
    let test_dir = support::unique_test_dir("aura-multifactor-unenrolled-device");

    let bob_device_id_str = "demo:bob";
    let bob_authority = AuthorityId::new_from_entropy(hash::hash(
        format!("authority:{bob_device_id_str}").as_bytes(),
    ));
    let bob_context = ContextId::new_from_entropy(hash::hash(
        format!("context:{bob_device_id_str}").as_bytes(),
    ));

    // The contact's device belongs to its own authority, not to Bob's account.
    let mobile_device_id_str = "demo:bob-mobile";
    let mobile_authority = AuthorityId::new_from_entropy(hash::hash(
        format!("authority:{mobile_device_id_str}").as_bytes(),
    ));

    let agent_config = AgentConfig {
        device_id: ids::device_id(bob_device_id_str),
        storage: aura_agent::core::config::StorageConfig {
            base_path: test_dir.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let effect_ctx = EffectContext::new(
        bob_authority,
        bob_context,
        ExecutionMode::Simulation { seed },
    );
    let agent = Arc::new(
        AgentBuilder::new()
            .with_config(agent_config)
            .with_authority(bob_authority)
            .build_simulation_async(seed, &effect_ctx)
            .await
            .expect("build simulation agent"),
    );
    let app_config = AppConfig {
        data_dir: test_dir.to_string_lossy().to_string(),
        ..AppConfig::default()
    };
    let app_core = Arc::new(RwLock::new(
        AppCore::with_runtime(app_config, agent.clone().as_runtime_bridge())
            .expect("create AppCore with runtime"),
    ));
    let _initialized = InitializedAppCore::new(app_core.clone())
        .await
        .expect("init signals");

    agent
        .clone()
        .as_runtime_bridge()
        .commit_relational_facts(&[ContactFact::added_ms(
            ContextId::new_from_entropy([3u8; 32]),
            bob_authority,
            mobile_authority,
            "Bob's Mobile".to_string(),
            1,
            aura_relational::contacts::test_support::fresh(1),
        )
        .to_generic()])
        .await
        .expect("commit contact facts");

    let error = aura_app::ui::workflows::ceremonies::start_device_threshold_ceremony(
        &app_core,
        FrostThreshold::new(2).expect("valid threshold"),
        2,
        vec![
            ids::device_id(bob_device_id_str).to_string(),
            ids::device_id(mobile_device_id_str).to_string(),
        ],
    )
    .await
    .map(|handle| handle.status_handle())
    .expect_err("a contact's device is not one of this account's enrolled devices");
    let message = error.to_string();
    assert!(
        message.contains("is not enrolled in this account"),
        "the refusal names the unenrolled device: {message}"
    );

    let _ = std::fs::remove_dir_all(&test_dir);
}
