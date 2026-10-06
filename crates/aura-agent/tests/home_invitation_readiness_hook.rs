//! Task 86: after a home invitation is accepted, the inviter's readiness
//! refresh hooks must stay alive; a failed participant lookup for one update
//! may not end the long-lived attachment.

#![allow(missing_docs)]

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, SharedTransport};
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::signals::{CONTACTS_SIGNAL, HOMES_SIGNAL};
use aura_app::ui::workflows::{context, invitation, strong_command as sc};
use aura_core::context::EffectContext;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId};
use aura_core::AuraError;
use std::sync::Arc;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(20);

struct Peer {
    _temp: tempfile::TempDir,
    _agent: Arc<AuraAgent>,
    app: Arc<RwLock<AppCore>>,
    id: AuthorityId,
}

async fn peer(seed: u8, transport: SharedTransport) -> Result<Peer> {
    let id = AuthorityId::new_from_entropy([seed; 32]);
    let ctx = EffectContext::new(
        id,
        ContextId::new_from_entropy([seed.wrapping_add(1); 32]),
        ExecutionMode::Testing,
    );
    let temp = tempfile::tempdir()?;
    let mut config = AgentConfig {
        device_id: DeviceId::new_from_entropy([seed.wrapping_add(2); 32]),
        ..AgentConfig::default()
    };
    config.storage.base_path = temp.path().join("aura");
    let agent = Arc::new(
        AgentBuilder::new()
            .with_authority(id)
            .with_config(config)
            .build_simulation_async_with_shared_transport(u64::from(seed), &ctx, transport)
            .await?,
    );
    let app = Arc::new(RwLock::new(AppCore::with_runtime(
        AppConfig::default(),
        agent.clone().as_runtime_bridge(),
    )?));
    AppCore::init_signals_with_hooks(&app).await?;
    app.read()
        .await
        .bootstrap_signing_keys()
        .await
        .map_err(|e| anyhow!("bootstrap: {e}"))?;
    Ok(Peer {
        _temp: temp,
        _agent: agent,
        app,
        id,
    })
}

async fn wait_until<F, Fut>(what: &str, mut check: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + WAIT;
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(anyhow!("timed out waiting for {what}"))
}

async fn is_contact(app: &Arc<RwLock<AppCore>>, target: AuthorityId) -> bool {
    let state = app.read().await.read(&*CONTACTS_SIGNAL).await;
    state.is_ok_and(|s| s.all_contacts().any(|c| c.id == target))
}

async fn has_home(app: &Arc<RwLock<AppCore>>, home: ChannelId) -> bool {
    let homes = app.read().await.read(&*HOMES_SIGNAL).await;
    homes.is_ok_and(|h| h.home_state(&home).is_some())
}

async fn home_invite(
    app: &Arc<RwLock<AppCore>>,
    actor: AuthorityId,
    channel: ChannelId,
    target: AuthorityId,
) -> Result<(), AuraError> {
    let resolver = sc::CommandResolver::default();
    let snapshot = resolver.capture_snapshot(app).await;
    let resolved = resolver
        .resolve(
            sc::ParsedCommand::HomeInvite {
                target: target.to_string(),
            },
            &snapshot,
        )
        .map_err(|e| AuraError::invalid(format!("resolve: {e}")))?;
    let hint = channel.to_string();
    let plan = resolver
        .plan(resolved, &snapshot, Some(hint.as_str()), Some(actor))
        .map_err(|e| AuraError::invalid(format!("plan: {e}")))?;
    sc::execute_planned(app, plan).await.map(|_| ())
}

#[tokio::test]
async fn inviter_readiness_hooks_survive_home_invitation_acceptance() -> Result<()> {
    let transport = SharedTransport::new();
    let barbara = peer(101, transport.clone()).await?;
    let alex = peer(105, transport).await?;

    let invite = invitation::create_contact_invitation(
        &barbara.app,
        alex.id,
        None,
        None,
        Some("contact".to_string()),
        None,
    )
    .await?;
    let code = invitation::export_invitation(&barbara.app, invite.invitation_id()).await?;
    let imported = invitation::import_invitation_details(&alex.app, &code).await?;
    invitation::accept_invitation(&alex.app, imported).await?;
    wait_until("Barbara sees Alex", || is_contact(&barbara.app, alex.id)).await?;
    wait_until("Alex sees Barbara", || is_contact(&alex.app, barbara.id)).await?;

    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    home_invite(&barbara.app, barbara.id, home, alex.id).await?;
    wait_until("Alex accepts the home invitation", || async {
        invitation::accept_pending_channel_invitation(&alex.app)
            .await
            .is_ok()
    })
    .await?;
    wait_until("Alex materializes the home", || has_home(&alex.app, home)).await?;

    // Let the acceptance reach Barbara and drive her readiness hooks.
    tokio::time::sleep(Duration::from_secs(5)).await;
    for fact in barbara.app.read().await.authoritative_semantic_facts() {
        if matches!(
            fact,
            aura_app::ui_contract::AuthoritativeSemanticFact::ChannelMembershipReady { .. }
                | aura_app::ui_contract::AuthoritativeSemanticFact::RecipientPeersResolved { .. }
        ) {
            eprintln!("[barbara] {fact:?}");
        }
    }
    for (name, app) in [("barbara", &barbara.app), ("alex", &alex.app)] {
        let failure = app.read().await.refresh_hook_failure().await;
        assert!(
            failure.is_none(),
            "{name}'s refresh hooks died: {:?}",
            failure.map(|f| format!("{f} / native: {:?}", f.native_error()))
        );
        let updates = app.read().await.refresh_hook_update_failures().await;
        for update in &updates {
            eprintln!("[{name}] update failure: {update}");
        }
        // Task 88: shared-transport simulation runs the production sync
        // service, so no refresh update may fail (e.g. a missing sync status).
        assert!(
            updates.is_empty(),
            "{name}'s refresh updates failed: {updates:?}"
        );
    }
    Ok(())
}
