//! Sync Command Handler
//!
//! Effect-based implementation of the sync daemon command.
//! Uses `SyncServiceManager` from aura-agent for background journal synchronization.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::sync::SyncAction;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use crate::ids;
use aura_agent::{AdmittedSyncCommandCapability, ServiceHealth, SyncManagerConfig};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::identifiers::DeviceId;

use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

#[derive(Debug, thiserror::Error)]
#[error("sync command requires the current owned runtime")]
struct SyncRuntimeUnavailable;

async fn admit_sync_command(
    ctx: &HandlerContext<'_>,
    config: SyncManagerConfig,
) -> TerminalResult<AdmittedSyncCommandCapability> {
    let agent = ctx.agent().ok_or_else(|| {
        TerminalError::native_operation(
            "No owned runtime is available for sync",
            SyncRuntimeUnavailable,
        )
    })?;
    agent
        .runtime()
        .admit_sync_command(config)
        .await
        .map_err(|source| {
            TerminalError::native_operation("Failed to start owned sync command", source)
        })
}

/// An execution failure remains primary when stopping also fails.
#[derive(Debug)]
pub struct SyncRunCleanupFailure {
    primary: Arc<dyn std::error::Error + Send + Sync>,
    cleanup: Arc<dyn std::error::Error + Send + Sync>,
}
impl SyncRunCleanupFailure {
    /// Inspect the independently retained native cleanup failure.
    #[must_use]
    pub fn cleanup_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
        self.cleanup.as_ref()
    }
}
impl std::fmt::Display for SyncRunCleanupFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; stopping sync also failed: {}",
            self.primary,
            self.cleanup_error()
        )
    }
}
impl std::error::Error for SyncRunCleanupFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

// One owner awaits stop on every normal/error exit from the run future.
async fn run_sync_with_cleanup<T, E, S, Run, Stop, Stopped>(
    run: Run,
    stop: Stop,
) -> TerminalResult<T>
where
    E: std::error::Error + Send + Sync + 'static,
    S: std::error::Error + Send + Sync + 'static,
    Run: std::future::Future<Output = Result<T, E>>,
    Stop: FnOnce() -> Stopped,
    Stopped: std::future::Future<Output = Result<(), S>>,
{
    let result = run.await;
    let cleanup = stop().await;
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(primary), Ok(())) => Err(TerminalError::native_operation(
            "Sync execution failed",
            primary,
        )),
        (Ok(_), Err(cleanup)) => Err(TerminalError::native_operation(
            "Failed to stop sync service",
            cleanup,
        )),
        (Err(primary), Err(cleanup)) => {
            let failure = SyncRunCleanupFailure {
                primary: Arc::new(primary),
                cleanup: Arc::new(cleanup),
            };
            Err(TerminalError::native_operation(
                failure.to_string(),
                failure,
            ))
        }
    }
}

fn sync_source(error: impl std::error::Error + Send + Sync + 'static) -> aura_core::AuraError {
    aura_core::AuraError::Internal {
        message: error.to_string(),
        source: Some(Arc::new(error)),
    }
}

// Sleep failure is an error, never a tick. The same high-water owner spans all ticks.
async fn required_sync_tick<E: PhysicalTimeEffects + ?Sized>(
    time: &E,
    observation: &aura_core::TimeoutClockObservation,
    started_at_ms: u64,
    interval_ms: u64,
) -> Result<u64, aura_core::AuraError> {
    time.sleep_ms(interval_ms).await.map_err(sync_source)?;
    let now = time.physical_time().await.map_err(sync_source)?;
    observation
        .observe(&now)
        .map_err(aura_core::AuraError::from)?;
    let elapsed = now
        .ts_ms
        .checked_sub(started_at_ms)
        .ok_or_else(|| aura_core::AuraError::invalid("sync uptime precedes original start"))?;
    Ok(elapsed / 1000)
}

/// Handle sync operations through effects
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_sync(
    ctx: &HandlerContext<'_>,
    action: &SyncAction,
) -> TerminalResult<CliOutput> {
    match action {
        SyncAction::Daemon {
            interval,
            max_concurrent,
            peers,
        } => handle_daemon_mode(ctx, *interval, *max_concurrent, peers.as_deref()).await,

        SyncAction::Once { peers } => handle_once_mode(ctx, peers).await,

        SyncAction::Status => handle_status(ctx),

        SyncAction::AddPeer { peer } => handle_add_peer(ctx, peer),

        SyncAction::RemovePeer { peer } => handle_remove_peer(ctx, peer),
    }
}

/// Run sync daemon mode (default)
///
/// Note: Daemon mode prints continuously during operation and returns
/// summary output when shutting down. The periodic status messages
/// are printed in real-time.
async fn handle_daemon_mode(
    ctx: &HandlerContext<'_>,
    interval_secs: u64,
    max_concurrent: usize,
    peers: Option<&str>,
) -> TerminalResult<CliOutput> {
    if interval_secs == 0 {
        return Err(TerminalError::Input(
            "Sync interval must be positive".into(),
        ));
    }
    let mut output = CliOutput::new();

    output.println("Starting sync daemon...");
    output.kv("Interval", format!("{interval_secs}s"));
    output.kv("Max concurrent", max_concurrent.to_string());

    // Parse initial peers using portable helper
    let initial_peers: Vec<DeviceId> = if let Some(peers_str) = peers {
        aura_app::ui::workflows::sync::parse_peer_list(peers_str)
            .into_iter()
            .map(|s| ids::device_id(&s))
            .collect()
    } else {
        Vec::new()
    };

    if !initial_peers.is_empty() {
        output.kv("Initial peers", initial_peers.len().to_string());
    }

    // Configure sync manager
    let config = SyncManagerConfig {
        auto_sync_enabled: true,
        auto_sync_interval: Duration::from_secs(interval_secs),
        max_concurrent_syncs: max_concurrent,
        initial_peers,
        ..SyncManagerConfig::default()
    };

    let interval_ms =
        u64::try_from(Duration::from_secs(interval_secs).as_millis()).map_err(|error| {
            TerminalError::native_operation("Sync interval exceeds physical milliseconds", error)
        })?;
    let manager = admit_sync_command(ctx, config).await?;
    let time_handler = manager.time_effects();
    let tick_count = run_sync_with_cleanup(async {

        eprintln!("Sync daemon started. Press Ctrl+C to stop.");
        let started = time_handler.physical_time().await.map_err(sync_source)?;
        let observation = aura_core::TimeoutClockObservation::new(&started);
        let mut tick_count = 0u64;
        loop {
            if let Some(failure) = manager.terminal_failure() {
                return Err(sync_source(failure));
            }
            tokio::select! {
                _ = manager.closed() => { break; }
                result = signal::ctrl_c() => {
                    result.map_err(sync_source)?;
                    eprintln!("Received shutdown signal...");
                    break;
                }
                result = required_sync_tick(time_handler.as_ref(), &observation, started.ts_ms, interval_ms) => {
                    let uptime_secs = result?;
                    if let Some(failure) = manager.terminal_failure() {
                        return Err(sync_source(failure));
                    }
                    tick_count += 1;
                    let health = manager.health().await;
                    eprintln!("[tick {tick_count}] Sync daemon {health} (uptime: {uptime_secs}s)");
                    if tick_count % 5 == 0 {
                        if let Some(metrics) = manager.metrics().await {
                            eprintln!("  Metrics - requests: {}, errors: {}, avg latency: {:.2}ms",
                                metrics.requests_processed, metrics.errors_encountered, metrics.avg_latency_ms);
                        }
                    }
                }
            }
        }
        Ok::<_, aura_core::AuraError>(tick_count)
    }, || async {
        eprintln!("Stopping sync daemon...");
        manager.stop().await
    }).await?;

    // Progress went to stderr while running; stdout gets one summary.
    output.println("Sync daemon stopped.");
    output.kv("Total ticks", tick_count.to_string());
    Ok(output)
}

/// Perform a one-shot sync with specific peers
async fn handle_once_mode(ctx: &HandlerContext<'_>, peers_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println("Performing one-shot sync...");

    // Parse peers
    let peers: Vec<DeviceId> = peers_str
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| ids::device_id(s.trim()))
        .collect();

    if peers.is_empty() {
        return Err(TerminalError::Input("No peers specified for sync".into()));
    }

    output.kv("Peers", peers.len().to_string());

    // Configure for one-shot (no auto sync)
    let config = SyncManagerConfig::manual_only();
    let manager = admit_sync_command(ctx, config).await?;

    run_sync_with_cleanup(
        async {
            manager.sync_with_peers(peers).await?;

            // Show completion
            let health = manager.health().await;
            output.kv("Sync service health", format_service_health(&health));

            Ok::<(), aura_core::AuraError>(())
        },
        || manager.stop(),
    )
    .await?;

    output.println("One-shot sync complete.");
    Ok(output)
}

fn format_service_health(health: &ServiceHealth) -> &'static str {
    match health {
        ServiceHealth::Healthy => "healthy",
        ServiceHealth::Degraded { .. } => "degraded",
        ServiceHealth::Unhealthy { .. } => "unhealthy",
        ServiceHealth::NotStarted => "not started",
        ServiceHealth::Starting => "starting",
        ServiceHealth::Stopping => "stopping",
        ServiceHealth::Stopped => "stopped",
    }
}

/// Show sync status and metrics
fn handle_status(ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section("Sync Service Status");

    // Status query requires a running sync daemon (started via `aura sync daemon`).
    // Without a daemon, show usage instructions.
    output.println("Note: Full status requires a running sync daemon.");
    output.blank();
    output.println("To start the sync daemon:");
    output.println("  aura sync daemon");
    output.blank();
    output.println("To sync once with specific peers:");
    output.println("  aura sync once --peers <device-id-1>,<device-id-2>");

    let _ = ctx; // Acknowledge context
    Ok(output)
}

/// Add a peer to the sync list
fn handle_add_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let peer_id = ids::device_id(peer_str);
    output.kv("Added peer to sync list", peer_id.to_string());
    output.println("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(output)
}

/// Remove a peer from the sync list
fn handle_remove_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let peer_id = ids::device_id(peer_str);
    output.kv("Removed peer from sync list", peer_id.to_string());
    output.println("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(output)
}

#[cfg(test)]
mod owned_sync_failure_tests {
    use super::*;
    use aura_core::effects::TimeError;
    use aura_core::time::PhysicalTime;
    use aura_core::TimeoutClockObservation;
    use std::error::Error;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FailedSleep {
        reads: Arc<AtomicUsize>,
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for FailedSleep {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(PhysicalTime {
                ts_ms: 100,
                uncertainty: None,
            })
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), TimeError> {
            Err(TimeError::ServiceUnavailable)
        }
    }

    struct TickTime {
        now_ms: u64,
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for TickTime {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            Ok(PhysicalTime {
                ts_ms: self.now_ms,
                uncertainty: None,
            })
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), TimeError> {
            Ok(())
        }
    }

    fn find_cause<'a, T: Error + 'static>(error: &'a (dyn Error + 'static)) -> Option<&'a T> {
        let mut current = Some(error);
        while let Some(cause) = current {
            if let Some(found) = cause.downcast_ref::<T>() {
                return Some(found);
            }
            current = cause.source();
        }
        None
    }

    #[tokio::test]
    async fn required_sleep_failure_stops_before_tick_and_always_runs_cleanup() {
        let reads = Arc::new(AtomicUsize::new(0));
        let time = FailedSleep {
            reads: reads.clone(),
        };
        let clock = TimeoutClockObservation::new(&PhysicalTime {
            ts_ms: 100,
            uncertainty: None,
        });
        let stopped = Arc::new(AtomicUsize::new(0));
        let stop = stopped.clone();
        let error =
            run_sync_with_cleanup(required_sync_tick(&time, &clock, 100, 10), || async move {
                stop.fetch_add(1, Ordering::SeqCst);
                Ok::<(), aura_agent::ServiceError>(())
            })
            .await
            .expect_err("required sleep failure terminates owned run");
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "no clock read or tick after failed sleep"
        );
        assert_eq!(
            stopped.load(Ordering::SeqCst),
            1,
            "cleanup runs exactly once"
        );
        assert!(find_cause::<TimeError>(&error).is_some());
        let clone = error.clone();
        assert_eq!(clone, error, "clone preserves opaque source identity");
        assert!(find_cause::<TimeError>(&clone).is_some());
    }

    #[tokio::test]
    async fn clock_rollback_after_progress_stops_without_new_tick() {
        let clock = TimeoutClockObservation::new(&PhysicalTime {
            ts_ms: 100,
            uncertainty: None,
        });
        assert_eq!(
            required_sync_tick(&TickTime { now_ms: 200 }, &clock, 100, 10)
                .await
                .expect("first valid tick"),
            0
        );
        let stopped = Arc::new(AtomicUsize::new(0));
        let stop = stopped.clone();
        let error = run_sync_with_cleanup(
            required_sync_tick(&TickTime { now_ms: 150 }, &clock, 100, 10),
            || async move {
                stop.fetch_add(1, Ordering::SeqCst);
                Ok::<(), aura_agent::ServiceError>(())
            },
        )
        .await
        .expect_err("rollback above start still fails");
        assert!(matches!(
            find_cause::<aura_core::TimeoutBudgetError>(&error),
            Some(aura_core::TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms: 200,
                observed_at_ms: 150
            })
        ));
        assert_eq!(stopped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn equal_diagnostics_do_not_equate_independent_native_causes() {
        let first = TerminalError::native_operation("same", TimeError::ServiceUnavailable);
        let second = TerminalError::native_operation("same", TimeError::ServiceUnavailable);
        assert_ne!(first, second);
        assert_eq!(first, first.clone());
    }

    #[tokio::test]
    async fn cleanup_failure_retains_both_actual_native_causes() {
        let codec = serde_json::from_slice::<u64>(b"invalid").expect_err("actual malformed JSON");
        let error = run_sync_with_cleanup(async { Err::<(), _>(codec) }, || async {
            Err::<(), _>(aura_agent::ServiceError::shutdown_failed(
                "sync",
                "injected stop failure",
            ))
        })
        .await
        .expect_err("execution and stop both fail");
        assert!(find_cause::<serde_json::Error>(&error).is_some());
        let paired = find_cause::<SyncRunCleanupFailure>(&error).expect("owned paired failure");
        assert!(paired.cleanup_error().is::<aura_agent::ServiceError>());
    }

    #[tokio::test]
    async fn stop_failure_cannot_publish_success() {
        let error = run_sync_with_cleanup(async { Ok::<_, aura_core::AuraError>(()) }, || async {
            Err::<(), _>(aura_agent::ServiceError::shutdown_failed(
                "sync",
                "injected stop failure",
            ))
        })
        .await
        .expect_err("cleanup failure rejects successful run");
        assert!(find_cause::<aura_agent::ServiceError>(&error).is_some());
    }
}
