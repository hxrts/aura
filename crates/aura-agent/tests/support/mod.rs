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
/// Real time one wait may take before it reports a virtual-time stall.
const WALL_WATCHDOG: Duration = Duration::from_secs(90);

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
    seed: u8,
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
        self.simulation_peer(seed, tempfile::tempdir()?).await
    }

    async fn simulation_peer(&self, seed: u8, temp: tempfile::TempDir) -> Result<Peer> {
        let transport = self.transport.clone();
        // Boxed: runtime assembly is a large future, kept off the test stack.
        Box::pin(Peer::build(
            seed,
            &self.clock,
            temp,
            |builder, ctx| async move {
                builder
                    .build_simulation_async_with_shared_transport(u64::from(seed), &ctx, transport)
                    .await
            },
        ))
        .await
    }

    /// End-of-test assertion: once all runtimes are quiescent, no supervised
    /// task or refresh hook died and no refresh update failed.
    pub async fn finish(&self) -> Result<()> {
        quiesce().await;
        check_runtimes_alive().await?;
        for (seed, agent, app) in live_runtimes() {
            // One-shot invitation work must settle; a task still running
            // here is waiting on a peer step that will not arrive.
            let lingering: Vec<String> = agent
                .active_supervised_tasks()
                .into_iter()
                .filter(|task| task.starts_with("invitation_service."))
                .collect();
            if !lingering.is_empty() {
                return Err(anyhow!(
                    "peer {seed}: invitation tasks still running: {lingering:?}"
                ));
            }
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
        Peer::build(
            seed,
            &self.clock,
            tempfile::tempdir()?,
            |builder, ctx| async move { builder.build_testing_async(&ctx).await },
        )
        .await
    }
}

impl Peer {
    fn context(seed: u8) -> EffectContext {
        EffectContext::new(
            AuthorityId::new_from_entropy([seed; 32]),
            ContextId::new_from_entropy([seed.wrapping_add(1); 32]),
            ExecutionMode::Testing,
        )
    }

    /// Shut this simulation runtime down and reopen it from its persisted
    /// profile, on the same transport and clock.
    pub async fn restart(self, net: &SimNet) -> Result<Peer> {
        let Peer {
            _temp: temp,
            agent,
            app,
            seed,
            ..
        } = self;
        drop(app);
        // Runtime tasks (periodic sync, services) hold the agent; stop them
        // first so this handle becomes the last one.
        agent
            .runtime()
            .tasks()
            .shutdown_with_timeout(Duration::from_secs(5))
            .await?;
        quiesce().await;
        let agent = Arc::try_unwrap(agent)
            .map_err(|_| anyhow!("peer {seed}: runtime still shared at restart"))?;
        agent.shutdown(&Self::context(seed)).await?;
        net.simulation_peer(seed, temp).await
    }

    async fn build<F, Fut>(
        seed: u8,
        clock: &QuiescentClock,
        temp: tempfile::TempDir,
        assemble: F,
    ) -> Result<Self>
    where
        F: FnOnce(AgentBuilder, EffectContext) -> Fut,
        Fut: std::future::Future<Output = aura_agent::AgentResult<AuraAgent>>,
    {
        let id = AuthorityId::new_from_entropy([seed; 32]);
        let ctx = Self::context(seed);
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
            seed,
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
    let watchdog = aura_testkit::time::VirtualTimeStallWatchdog::start(WALL_WATCHDOG);
    loop {
        // A wait that burns real time without reaching its virtual deadline
        // is a livelock; fail fast instead of spinning for minutes.
        if let Some(elapsed) = watchdog.stalled() {
            return Err(anyhow!(
                "{what}: virtual time stalled ({:?} of real time elapsed, {:?} of virtual time left)",
                elapsed,
                deadline.saturating_duration_since(tokio::time::Instant::now())
            ));
        }
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
    let last_error = std::cell::RefCell::new(None::<String>);
    let accepted = wait_until("invitee accepts the home invitation", || async {
        match invitation::accept_pending_channel_invitation(&invitee.app).await {
            Ok(_) => true,
            Err(error) => {
                let error = error.to_string();
                if last_error.borrow().as_deref() != Some(error.as_str()) {
                    eprintln!("home invitation accept not yet possible: {error}");
                }
                *last_error.borrow_mut() = Some(error);
                false
            }
        }
    })
    .await;
    accepted.map_err(|e| e.context(format!("last accept error: {:?}", last_error.borrow())))?;
    wait_until("invitee materializes the home", || async {
        home_view(&invitee.app, home).await.is_some()
    })
    .await
}

/// Wait until `check` holds on every peer: the context state the check reads
/// has converged across them. Convergence comes only from the runtimes' own
/// periodic relational-context sync, driven by virtual time.
pub async fn wait_converged<F, Fut>(what: &str, peers: &[&Peer], check: F) -> Result<()>
where
    F: Fn(Arc<AuraAgent>, Arc<RwLock<AppCore>>) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    wait_until(what, || async {
        for peer in peers {
            if !check(peer.agent.clone(), peer.app.clone()).await {
                return false;
            }
        }
        true
    })
    .await
}

/// The AMP channel membership facts `agent` holds for `context`, as their
/// canonical encodings.
pub async fn membership_facts(
    agent: &AuraAgent,
    context: ContextId,
) -> std::collections::BTreeSet<Vec<u8>> {
    use aura_protocol::amp::AmpJournalEffects;
    let Ok(journal) = agent
        .runtime()
        .effects()
        .fetch_context_journal(context)
        .await
    else {
        return std::collections::BTreeSet::default();
    };
    journal
        .iter_facts()
        .filter_map(|fact| match &fact.content {
            aura_journal::fact::FactContent::Relational(
                aura_journal::fact::RelationalFact::Generic { envelope, .. },
            ) if envelope.type_id.as_str() == aura_amp::CHANNEL_MEMBERSHIP_FACT_TYPE_ID => {
                aura_core::util::serialization::to_vec(envelope).ok()
            }
            _ => None,
        })
        .collect()
}

/// Wait until every peer holds the same AMP membership facts for `context`.
pub async fn wait_membership_converged(peers: &[&Peer], context: ContextId) -> Result<()> {
    wait_until("channel membership facts converge", || async {
        let mut sets = Vec::new();
        for peer in peers {
            sets.push(membership_facts(&peer.agent, context).await);
        }
        sets.windows(2).all(|pair| pair[0] == pair[1])
    })
    .await
}
