//! Two-member home moderation over a shared in-memory transport.
//!
//! Barbara creates a home and invites her contact Alex, who joins. Barbara
//! then limits Alex's access and bans him. Both governance actions must take
//! effect on Alex's own client (his home view and his sender gate), and the
//! `/ban` strong command must observe the ban in Barbara's home ban list.

#![allow(missing_docs)]

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, LinkFault, SharedTransport};
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::signals::{CONTACTS_SIGNAL, HOMES_SIGNAL};
use aura_app::ui::workflows::{access, context, invitation, messaging, strong_command as sc};
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

async fn home_view(
    app: &Arc<RwLock<AppCore>>,
    home: ChannelId,
) -> Option<aura_app::views::home::HomeState> {
    let homes = app.read().await.read(&*HOMES_SIGNAL).await.ok()?;
    homes.home_state(&home).cloned()
}

async fn strong(
    app: &Arc<RwLock<AppCore>>,
    actor: AuthorityId,
    channel: ChannelId,
    parsed: sc::ParsedCommand,
) -> Result<sc::CommandExecutionResult, AuraError> {
    let resolver = sc::CommandResolver::default();
    let snapshot = resolver.capture_snapshot(app).await;
    let resolved = resolver
        .resolve(parsed, &snapshot)
        .map_err(|e| AuraError::invalid(format!("resolve: {e}")))?;
    let hint = channel.to_string();
    let plan = resolver
        .plan(resolved, &snapshot, Some(hint.as_str()), Some(actor))
        .map_err(|e| AuraError::invalid(format!("plan: {e}")))?;
    sc::execute_planned(app, plan).await
}

#[tokio::test]
async fn limited_then_banned_member_is_refused_on_own_client() -> Result<()> {
    moderation_reaches_member(None).await
}

/// The override and ban fanout is delayed past the moderation workflow's
/// completion (held on the Alex<->Barbara link, then released).
#[tokio::test]
async fn moderation_reaches_member_after_delayed_fanout() -> Result<()> {
    moderation_reaches_member(Some(LinkFault::Hold)).await
}

/// The override and ban fanout is lost on the Alex<->Barbara link (a
/// partition the sender cannot observe); after the link heals, Alex's
/// home-context journal sync pulls the missed facts.
#[tokio::test]
async fn moderation_reaches_member_after_lost_fanout() -> Result<()> {
    moderation_reaches_member(Some(LinkFault::Drop)).await
}

/// After a lost fanout, Alex's home-context journal sync pulls the facts.
async fn resync(alex: &Arc<RwLock<AppCore>>, fault: Option<LinkFault>) {
    if matches!(fault, Some(LinkFault::Drop)) {
        let _ = aura_app::ui::workflows::sync::force_sync(alex).await;
    }
}

/// Run `action` with the Alex<->Barbara link faulted, then heal it.
async fn faulted<T, Fut: std::future::Future<Output = T>>(
    transport: &SharedTransport,
    alex: AuthorityId,
    barbara: AuthorityId,
    fault: Option<LinkFault>,
    action: Fut,
) -> T {
    if let Some(fault) = fault {
        transport.fault_link(alex, barbara, fault);
    }
    let out = action.await;
    transport.heal_links();
    out
}

async fn moderation_reaches_member(fault: Option<LinkFault>) -> Result<()> {
    let transport = SharedTransport::new();
    let barbara = peer(91, transport.clone()).await?;
    let alex = peer(95, transport.clone()).await?;

    // Contacts first.
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
    wait_until("Barbara sees Alex as contact", || {
        is_contact(&barbara.app, alex.id)
    })
    .await?;
    wait_until("Alex sees Barbara as contact", || {
        is_contact(&alex.app, barbara.id)
    })
    .await?;

    // Home + home invitation (the `/homeinvite` path) + acceptance.
    let home = context::create_home(&barbara.app, Some("BarbHome".to_string()), None).await?;
    strong(
        &barbara.app,
        barbara.id,
        home,
        sc::ParsedCommand::HomeInvite {
            target: alex.id.to_string(),
        },
    )
    .await?;
    wait_until("Alex accepts the home invitation", || async {
        invitation::accept_pending_channel_invitation(&alex.app)
            .await
            .is_ok()
    })
    .await?;
    wait_until("Alex materializes the home", || async {
        home_view(&alex.app, home).await.is_some()
    })
    .await?;
    let alex_home = home_view(&alex.app, home)
        .await
        .ok_or_else(|| anyhow!("Alex home view missing"))?;
    eprintln!(
        "[joined] alex home members={} moderators={:?}",
        alex_home.members.len(),
        alex_home
            .members
            .iter()
            .filter(|m| m.is_moderator())
            .map(|m| m.id)
            .collect::<Vec<_>>()
    );
    messaging::send_message(&alex.app, home, "hello from alex", 1_700_000_000_100).await?;

    // Limited override (BarbHome is still the selected home): it must reach
    // Alex's view and his own client must refuse his send.
    faulted(
        &transport,
        alex.id,
        barbara.id,
        fault,
        access::set_access_override(
            &barbara.app,
            None,
            alex.id,
            aura_social::AccessLevel::Limited,
        ),
    )
    .await?;
    wait_until("Alex sees his Limited override", || async {
        resync(&alex.app, fault).await;
        home_view(&alex.app, home)
            .await
            .is_some_and(|h| h.access_override(&alex.id) == Some(aura_social::AccessLevel::Limited))
    })
    .await?;
    let limited_send =
        messaging::send_message(&alex.app, home, "limited send", 1_700_000_000_200).await;
    assert!(
        limited_send
            .as_ref()
            .is_err_and(|e| e.to_string().contains("access level")),
        "Alex's send at Limited must be refused: {limited_send:?}"
    );

    // Barbara owns a second home (as in the live run) that is now the
    // selected home, while she types `/ban` in BarbHome's channel.
    let other = context::create_home(&barbara.app, Some("OtherHome".to_string()), None).await?;
    let selected = barbara
        .app
        .read()
        .await
        .read(&*HOMES_SIGNAL)
        .await?
        .current_home_id()
        .copied();
    assert_eq!(
        selected,
        Some(other),
        "the new home becomes the selected home"
    );

    let ban = faulted(
        &transport,
        alex.id,
        barbara.id,
        fault,
        strong(
            &barbara.app,
            barbara.id,
            home,
            sc::ParsedCommand::Ban {
                target: alex.id.to_string(),
                reason: Some("spam".to_string()),
            },
        ),
    )
    .await?;
    assert!(
        matches!(
            ban.completion_outcome,
            sc::CommandCompletionOutcome::Satisfied(_)
        ),
        "ban barrier must be satisfied: {:?}",
        ban.completion_outcome
    );
    let barbara_other = home_view(&barbara.app, other)
        .await
        .ok_or_else(|| anyhow!("Barbara other home missing"))?;
    assert!(
        !barbara_other.ban_list.contains_key(&alex.id),
        "the ban targets the planned home, not the selected one"
    );
    wait_until("Alex sees his ban", || async {
        resync(&alex.app, fault).await;
        home_view(&alex.app, home)
            .await
            .is_some_and(|h| h.ban_list.contains_key(&alex.id))
    })
    .await?;
    let banned_send =
        messaging::send_message(&alex.app, home, "banned send", 1_700_000_000_300).await;
    assert!(
        banned_send
            .as_ref()
            .is_err_and(|e| e.to_string().contains("banned")),
        "Alex's send after the ban must be refused: {banned_send:?}"
    );
    Ok(())
}
