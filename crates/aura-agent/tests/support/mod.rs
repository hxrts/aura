//! Shared fixture for multi-runtime home tests on virtual time.
//!
//! Every runtime of a test shares one [`QuiescentClock`]; tests run under a
//! paused tokio clock (`#[tokio::test(start_paused = true)]`), so virtual
//! time, and with it every runtime timeout, retry and periodic sync, advances
//! only once all runtimes are idle. Waits are bounded in virtual time and
//! re-check their condition at each quiescent point, so outcomes do not
//! depend on host load.

#![allow(dead_code)]

use anyhow::{anyhow, Result};
use async_lock::RwLock;
use aura_agent::{AgentBuilder, AgentConfig, AuraAgent, SharedTransport};
use aura_app::core::{AppConfig, AppCore};
use aura_app::ui::signals::{CONTACTS_SIGNAL, HOMES_SIGNAL};
use aura_app::ui::workflows::{invitation, strong_command as sc};
use aura_core::context::EffectContext;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::ExecutionMode;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, DeviceId};
use aura_core::AuraError;
use aura_testkit::time::QuiescentClock;
use std::sync::{Arc, Weak};
use std::time::Duration;
/// A runtime of the current test: (seed, agent, app core).
type TestRuntime = (u8, Weak<AuraAgent>, Weak<RwLock<AppCore>>);

std::thread_local! {
    /// Runtimes of the current test (each `#[tokio::test]` owns its thread).
    static RUNTIMES: std::cell::RefCell<Vec<TestRuntime>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn live_runtimes() -> Vec<(u8, Arc<AuraAgent>, Arc<RwLock<AppCore>>)> {
    RUNTIMES.with(|r| {
        r.borrow()
            .iter()
            .filter_map(|(seed, agent, app)| Some((*seed, agent.upgrade()?, app.upgrade()?)))
            .collect()
    })
}

/// Fails with the details of every dead supervised runtime task or dead
/// refresh hook attachment, on any runtime of this test.
pub async fn check_runtimes_alive() -> Result<()> {
    let mut dead = Vec::new();
    for (seed, agent, app) in live_runtimes() {
        for failure in agent.supervised_task_failures() {
            dead.push(format!(
                "peer {seed}: supervised task died: {failure} / {failure:?}"
            ));
        }
        if let Some(failure) = app.read().await.refresh_hook_failure().await {
            dead.push(format!(
                "peer {seed}: refresh hooks died: {failure} / native: {:?}",
                failure.native_error()
            ));
        }
    }
    if dead.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("runtime failure:\n  {}", dead.join("\n  ")))
    }
}

/// Virtual-time bound for one awaited condition.
const WAIT: Duration = Duration::from_secs(60);
/// Virtual time between re-checks of a condition.
const RECHECK: Duration = Duration::from_millis(50);

/// A test's runtimes: one shared transport and one virtual clock.
pub struct SimNet {
    pub transport: SharedTransport,
    pub clock: QuiescentClock,
}

pub struct Peer {
    _temp: tempfile::TempDir,
    pub agent: Arc<AuraAgent>,
    pub app: Arc<RwLock<AppCore>>,
    pub id: AuthorityId,
}

impl SimNet {
    /// Start the network; call on a paused tokio runtime. Runtime tracing
    /// goes to stderr when `RUST_LOG` is set.
    pub fn new() -> Self {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
        RUNTIMES.with(|r| r.borrow_mut().clear());
        Self {
            transport: SharedTransport::new(),
            clock: QuiescentClock::start(),
        }
    }

    /// A simulation runtime on the shared transport and clock.
    pub async fn peer(&self, seed: u8) -> Result<Peer> {
        let transport = self.transport.clone();
        Peer::build(seed, &self.clock, |builder, ctx| async move {
            builder
                .build_simulation_async_with_shared_transport(u64::from(seed), &ctx, transport)
                .await
        })
        .await
    }

    /// End-of-test assertion: once all runtimes are quiescent, no supervised
    /// task or refresh hook died and no refresh update failed.
    pub async fn finish(&self) -> Result<()> {
        quiesce().await;
        check_runtimes_alive().await?;
        for (seed, _, app) in live_runtimes() {
            let updates = app.read().await.refresh_hook_update_failures().await;
            if !updates.is_empty() {
                return Err(anyhow!(
                    "peer {seed}: refresh updates failed: {:?}",
                    updates
                        .iter()
                        .map(|f| format!("{f} / native: {:?}", f.native_error()))
                        .collect::<Vec<_>>()
                ));
            }
        }
        Ok(())
    }

    /// A standalone testing runtime on the shared clock.
    pub async fn testing_peer(&self, seed: u8) -> Result<Peer> {
        Peer::build(seed, &self.clock, |builder, ctx| async move {
            builder.build_testing_async(&ctx).await
        })
        .await
    }
}

impl Peer {
    async fn build<F, Fut>(seed: u8, clock: &QuiescentClock, assemble: F) -> Result<Self>
    where
        F: FnOnce(AgentBuilder, EffectContext) -> Fut,
        Fut: std::future::Future<Output = aura_agent::AgentResult<AuraAgent>>,
    {
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
        let builder = AgentBuilder::new()
            .with_authority(id)
            .with_config(config)
            .with_physical_time_provider(clock.provider());
        let agent = Arc::new(assemble(builder, ctx).await?);
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
        RUNTIMES.with(|r| {
            r.borrow_mut()
                .push((seed, Arc::downgrade(&agent), Arc::downgrade(&app)));
        });
        Ok(Self {
            _temp: temp,
            agent,
            app,
            id,
        })
    }
}

/// Wait until `check` holds, re-checking at each quiescent point; fails after
/// [`WAIT`] of virtual time, or at once when any runtime of the test records
/// a dead supervised task or refresh hook.
pub async fn wait_until<F, Fut>(what: &str, mut check: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        check_runtimes_alive()
            .await
            .map_err(|e| e.context(format!("while waiting for: {what}")))?;
        if check().await {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!(
                "{what}: not reached within {WAIT:?} of virtual time"
            ));
        }
        tokio::time::sleep(RECHECK).await;
    }
}

/// Wait until every runtime is idle.
pub async fn quiesce() {
    tokio::time::sleep(RECHECK).await;
}

pub async fn is_contact(app: &Arc<RwLock<AppCore>>, target: AuthorityId) -> bool {
    let state = app.read().await.read(&*CONTACTS_SIGNAL).await;
    state.is_ok_and(|s| s.all_contacts().any(|c| c.id == target))
}

pub async fn home_view(
    app: &Arc<RwLock<AppCore>>,
    home: ChannelId,
) -> Option<aura_app::views::home::HomeState> {
    let homes = app.read().await.read(&*HOMES_SIGNAL).await.ok()?;
    homes.home_state(&home).cloned()
}

/// `inviter` invites `invitee` as a contact; both see each other.
pub async fn link_contacts(inviter: &Peer, invitee: &Peer) -> Result<()> {
    let invite = invitation::create_contact_invitation(
        &inviter.app,
        invitee.id,
        None,
        None,
        Some("contact".to_string()),
        None,
    )
    .await?;
    let code = invitation::export_invitation(&inviter.app, invite.invitation_id()).await?;
    let imported = invitation::import_invitation_details(&invitee.app, &code).await?;
    invitation::accept_invitation(&invitee.app, imported).await?;
    wait_until("inviter sees the invitee as contact", || {
        is_contact(&inviter.app, invitee.id)
    })
    .await?;
    wait_until("invitee sees the inviter as contact", || {
        is_contact(&invitee.app, inviter.id)
    })
    .await
}

/// Run a strong command typed by `actor` in `channel`.
pub async fn strong(
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

/// `/homeinvite` by `inviter` in `home`, accepted by `invitee`, who then
/// materializes the home.
pub async fn join_home(inviter: &Peer, invitee: &Peer, home: ChannelId) -> Result<()> {
    strong(
        &inviter.app,
        inviter.id,
        home,
        sc::ParsedCommand::HomeInvite {
            target: invitee.id.to_string(),
        },
    )
    .await?;
    wait_until("invitee accepts the home invitation", || async {
        invitation::accept_pending_channel_invitation(&invitee.app)
            .await
            .is_ok()
    })
    .await?;
    wait_until("invitee materializes the home", || async {
        home_view(&invitee.app, home).await.is_some()
    })
    .await
}
