//! # Reactive Scheduler
//!
//! Core orchestration component for reactive view updates from journal facts.
//!
//! The ReactiveScheduler is a Tokio task that:
//! - Receives facts from multiple sources (journal, network, timer)
//! - Batches facts with a 5ms window to reduce update thrashing
//! - Updates views in topological order for glitch-freedom
//! - Emits ViewUpdate events for UI consumption
//!
//! ## Runtime Layer Note
//!
//! This module is part of Layer 6 (Runtime Composition) and uses effect-injected
//! time via `PhysicalTimeEffects` for simulator control. The batch window is a
//! performance optimization, not a correctness requirement - facts are processed
//! regardless of timing. All sleeps go through `PhysicalTimeEffects::sleep_ms` so
//! the simulator can control time advancement.

mod publication;
use super::state::SchedulerStats;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::types::identifiers::AuthorityId;
use aura_core::util::graph::{CycleError, DagNode};
use aura_core::{AuraError, TimeoutBudget, TimeoutBudgetError};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::FactRegistry;
use publication::ProcessingProgress;
pub use publication::{
    FactIngress, FactProcessingError, FactProcessingTargetCapability, IssuedFactPublication,
};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::sync::{broadcast, mpsc, RwLock};

/// Large channel because scheduler updates fan out to UI observers and commit
/// awaiters; short bursts should not immediately force subscribers into lag.
const VIEW_UPDATE_CHANNEL_CAPACITY: usize = 1024;

#[cfg(target_arch = "wasm32")]
pub type ReactiveUpdateFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AuraError>> + 'a>>;
#[cfg(not(target_arch = "wasm32"))]
pub type ReactiveUpdateFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), AuraError>> + Send + 'a>>;

type ApplyFuture<'a> = ReactiveUpdateFuture<'a>;
type ApplyFn<V, Delta> = Arc<dyn for<'a> Fn(&'a V, Delta) -> ApplyFuture<'a> + Send + Sync>;

/// Sources of facts that flow into the scheduler
#[derive(Debug, Clone)]
pub enum FactSource {
    /// Facts from local journal
    Journal(Vec<Fact>),
    /// Facts from network (anti-entropy, consensus)
    Network(Vec<Fact>),
    /// Scheduled/deferred facts (timers, delayed operations)
    Timer(Vec<Fact>),
    /// Actual original-ingress publication with a private issued processing target.
    Published(IssuedFactPublication),
}

/// Pure function that transforms facts into view deltas
///
/// This trait represents a monotone function F → ΔV, where:
/// - F is the fact lattice
/// - ΔV is the delta (incremental update) for a view
///
/// ## Design Principles
///
/// 1. **Pure**: No side effects, deterministic output from facts
/// 2. **Monotone**: More facts → more deltas (never fewer)
/// 3. **Composable**: Reductions can be combined and chained
/// 4. **Testable**: Easy to unit test without async/state
///
/// ## Example
///
/// ```ignore
/// struct ChatReduction;
///
/// impl ViewReduction<ChatDelta> for ChatReduction {
///     fn reduce(&self, facts: &[Fact]) -> Vec<ChatDelta> {
///         facts.iter()
///             .filter_map(|fact| match fact {
///                 Fact::MessageSent { channel_id, msg_id, .. } =>
///                     Some(ChatDelta::MessageAdded {
///                         channel_id: channel_id.clone(),
///                         message_id: msg_id.clone(),
///                     }),
///                 _ => None,
///             })
///             .collect()
///     }
/// }
/// ```
pub trait ViewReduction<Delta>: Send + Sync {
    /// Pure function: facts → deltas
    ///
    /// This should be:
    /// - Deterministic: same facts always produce same deltas
    /// - Monotone: F₁ ⊆ F₂ ⇒ reduce(F₁) ⊆ reduce(F₂)
    /// - Idempotent: applying same delta multiple times is safe
    ///
    /// # Arguments
    /// * `facts` - The journal facts to reduce
    /// * `own_authority` - The current user's authority ID, used for contextual
    ///   reduction (e.g., determining if a message is own, invitation direction)
    fn reduce(&self, facts: &[Fact], own_authority: Option<AuthorityId>) -> Vec<Delta>;
}

/// Update events emitted by the scheduler to subscribers
#[derive(Debug, Clone)]
pub enum ViewUpdate {
    /// A batch of facts was processed
    Batch { count: usize },
    /// A specific view changed
    ViewChanged { view_id: String },
    /// Scheduler statistics
    Stats {
        batch_count: u64,
        facts_processed: u64,
        avg_batch_latency_ms: Option<f64>,
    },
}

/// Trait for reactive views that can be updated from journal facts
pub trait ReactiveView: Send + Sync {
    /// Update the view based on new facts
    ///
    /// This function should be deterministic and idempotent - given the same
    /// sequence of facts, it should always produce the same view state.
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a>;

    /// Get the view's identifier for dependency ordering
    fn view_id(&self) -> &str;

    /// Get dependencies (other view IDs this view depends on)
    ///
    /// Used for topological sorting to guarantee glitch-freedom.
    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Type-erased reactive view for heterogeneous collections
///
/// This trait uses `Box<dyn Future>` instead of `impl Future` to be object-safe,
/// allowing us to store heterogeneous views in `Vec<Arc<dyn AnyView>>`.
pub trait AnyView: Send + Sync {
    /// Update the view based on new facts
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a>;

    /// Get the view's identifier
    fn view_id(&self) -> &str;

    /// Get dependencies
    fn dependencies(&self) -> Vec<String>;
}

impl<T: ReactiveView> AnyView for T {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        ReactiveView::update(self, facts)
    }

    fn view_id(&self) -> &str {
        ReactiveView::view_id(self)
    }

    fn dependencies(&self) -> Vec<String> {
        ReactiveView::dependencies(self)
    }
}

/// Configuration for the reactive scheduler
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Batching window duration (default: 5ms)
    pub batch_window: Duration,
    /// Maximum batch size before forcing flush (default: 1000)
    pub max_batch_size: usize,
    /// Enable statistics collection (default: false)
    pub collect_stats: bool,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            batch_window: Duration::from_millis(5),
            max_batch_size: 1000,
            collect_stats: false,
        }
    }
}

/// The reactive scheduler orchestrates fact ingestion and view updates
///
/// ## Design Principles
///
/// 1. **Batching**: Facts are batched with a 5ms window to reduce update thrashing
/// 2. **Ordering**: Views are updated in topological order to guarantee glitch-freedom
/// 3. **Determinism**: Given the same fact sequence, produces the same outputs
/// 4. **Backpressure**: Handles high fact rates gracefully
///
/// ## Example
///
/// ```ignore
/// let scheduler = scheduler_with_registry(config);
/// let fact_tx = scheduler.fact_sender();
/// let update_rx = scheduler.subscribe();
///
/// // Spawn scheduler task
/// tokio::spawn(scheduler.run());
///
/// // Send facts
/// fact_tx.send(FactSource::Journal(vec![fact])).await?;
///
/// // Receive updates
/// while let Ok(update) = update_rx.recv().await {
///     println!("View updated: {:?}", update);
/// }
/// ```
pub struct ReactiveScheduler {
    /// Configuration
    config: SchedulerConfig,
    /// Registered views (in topological order)
    views: Vec<Arc<dyn AnyView>>,
    /// Fact ingestion channel (receiver side)
    fact_rx: mpsc::Receiver<FactSource>,
    /// Retained exact acknowledgment, owned only by this actual scheduler.
    processed_tx: watch::Sender<ProcessingProgress>,
    original_ingress: FactIngress,
    /// View update broadcaster
    update_tx: broadcast::Sender<ViewUpdate>,
    /// Shutdown signal
    shutdown_rx: mpsc::Receiver<()>,
    /// Shared mutable scheduler state.
    shared: Arc<ReactiveSchedulerShared>,
    /// Fact registry for domain reducers
    fact_registry: Arc<FactRegistry>,
    /// Time effects for simulator-controllable sleeps
    time_effects: Arc<dyn PhysicalTimeEffects>,
}

struct ReactiveSchedulerShared {
    stats: RwLock<SchedulerStats>,
}

impl ReactiveScheduler {
    /// Create a new reactive scheduler
    ///
    /// Returns a tuple of (scheduler, fact_ingress, shutdown_sender)
    ///
    /// # Parameters
    /// - `config`: Scheduler configuration
    /// - `fact_registry`: Registry for domain reducers
    /// - `time_effects`: Time effects for simulator-controllable sleeps
    pub fn new(
        config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
    ) -> (Self, FactIngress, mpsc::Sender<()>) {
        Self::new_internal(config, fact_registry, time_effects, None)
    }

    #[aura_macros::capability_boundary(category = "capability_gated", capability = "pipeline_runtime_owner", capability_type = super::pipeline::PipelineRuntimeOwnerCapability, family = "runtime_helper")]
    pub(super) fn new_owned(
        config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        owner: super::pipeline::PipelineRuntimeOwnerCapability,
    ) -> (Self, FactIngress, mpsc::Sender<()>) {
        Self::new_internal(
            config,
            fact_registry,
            time_effects,
            Some(owner.into_runtime()),
        )
    }

    fn new_internal(
        config: SchedulerConfig,
        fact_registry: Arc<FactRegistry>,
        time_effects: Arc<dyn PhysicalTimeEffects>,
        runtime: Option<std::sync::Weak<crate::runtime::AuraEffectSystem>>,
    ) -> (Self, FactIngress, mpsc::Sender<()>) {
        let (raw_fact_tx, fact_rx) = mpsc::channel(256);
        let (processed_tx, processed_rx) = watch::channel(ProcessingProgress::default());
        let fact_tx = FactIngress::original(raw_fact_tx, processed_rx, runtime);
        let (update_tx, _) = broadcast::channel(VIEW_UPDATE_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let scheduler = Self {
            config,
            views: Vec::new(),
            fact_rx,
            processed_tx,
            original_ingress: fact_tx.clone(),
            update_tx: update_tx.clone(),
            shutdown_rx,
            shared: Arc::new(ReactiveSchedulerShared {
                stats: RwLock::new(SchedulerStats::default()),
            }),
            fact_registry,
            time_effects,
        };

        (scheduler, fact_tx, shutdown_tx)
    }

    /// Register a view with the scheduler
    ///
    /// Views will be sorted topologically based on their dependencies
    /// when the scheduler runs.
    pub fn register_view(&mut self, view: Arc<dyn AnyView>) {
        self.views.push(view);
    }

    /// Subscribe to view updates
    pub fn subscribe(&self) -> broadcast::Receiver<ViewUpdate> {
        self.update_tx.subscribe()
    }

    /// Main scheduler loop
    ///
    /// This is the heart of the reactive system. It:
    /// 1. Receives facts from multiple sources
    /// 2. Batches them with a time window
    /// 3. Updates views in topological order
    /// 4. Emits update events
    ///
    /// The loop runs until a shutdown signal is received.
    pub async fn run(mut self) -> Result<(), AuraError> {
        let result = self.run_owned().await;
        if let Err(source) = &result {
            self.processed_tx
                .send_modify(|progress| progress.failure = Some(source.clone()));
        }
        result
    }

    async fn run_owned(&mut self) -> Result<(), AuraError> {
        tracing::info!(
            "ReactiveScheduler starting (batch_window={:?})",
            self.config.batch_window
        );

        // Sort views topologically for glitch-freedom
        self.views = topological_sort(std::mem::take(&mut self.views));

        // Batching state: None = no batch in progress, Some(deadline_ms) = batch deadline
        let mut batch: Vec<Fact> = Vec::new();
        let mut batch_window: Option<TimeoutBudget> = None;
        let mut processed_through: Option<u64> = None;

        loop {
            // The batch scheduler owns its performance window. Required caller
            // readiness still uses its own original operation budget.
            let sleep_ms = if let Some(window) = &batch_window {
                let remaining = {
                    let _observation = window.acquire_observation().await;
                    let now = self
                        .time_effects
                        .physical_time()
                        .await
                        .map_err(required_scheduler_source)?;
                    window.remaining_at(&now)
                };
                match remaining {
                    Ok(remaining) => {
                        u64::try_from(remaining.as_millis()).map_err(required_scheduler_source)?
                    }
                    Err(TimeoutBudgetError::DeadlineExceeded { .. }) => {
                        batch_window = None;
                        self.process_batch(std::mem::take(&mut batch), processed_through.take())
                            .await?;
                        continue;
                    }
                    Err(source) => return Err(required_scheduler_source(source)),
                }
            } else {
                0
            };

            // Clone time_effects for use in select! statement
            let time_effects = self.time_effects.clone();

            tokio::select! {
                // Priority 1: Receive fact from any source
                Some(source) = self.fact_rx.recv() => {
                    let (facts, sequence) = self.extract_facts(source).map_err(required_scheduler_source)?;
                    if let Some(sequence) = sequence {
                        processed_through = Some(processed_through.map_or(sequence, |previous| previous.max(sequence)));
                    }
                    let was_empty = batch.is_empty();
                    batch.extend(facts);

                    if batch.is_empty() {
                        self.acknowledge_processed(processed_through.take());
                    }
                    // Start batching window if this is the first fact
                    if was_empty && !batch.is_empty() {
                        let now = self.time_effects.physical_time().await.map_err(required_scheduler_source)?;
                        if self.config.batch_window.is_zero() {
                            self.process_batch(std::mem::take(&mut batch), processed_through.take()).await?;
                        } else {
                            batch_window = Some(TimeoutBudget::from_start_and_timeout(&now, self.config.batch_window).map_err(required_scheduler_source)?);
                        }
                    }

                    // Force flush if batch is too large
                    if batch.len() >= self.config.max_batch_size {
                        tracing::warn!(
                            "Batch size {} exceeded max {}, forcing flush",
                            batch.len(),
                            self.config.max_batch_size
                        );
                        batch_window = None;
                        self.process_batch(std::mem::take(&mut batch), processed_through.take()).await?;
                    }
                }

                // Priority 2: Batch deadline expired (effect-based sleep for simulator control)
                slept = time_effects.sleep_ms(sleep_ms), if batch_window.is_some() => {
                    slept.map_err(required_scheduler_source)?;
                    let remaining = if let Some(window) = &batch_window {
                        let _observation = window.acquire_observation().await;
                        let now = self.time_effects.physical_time().await.map_err(required_scheduler_source)?;
                        window.remaining_at(&now)
                    } else { unreachable!("sleep branch requires an original batch window") };
                    match remaining {
                        Ok(_) => continue,
                        Err(TimeoutBudgetError::DeadlineExceeded { .. }) => {},
                        Err(source) => return Err(required_scheduler_source(source)),
                    }
                    batch_window = None;
                    self.process_batch(std::mem::take(&mut batch), processed_through.take()).await?;
                }

                // Priority 3: Graceful shutdown
                _ = self.shutdown_rx.recv() => {
                    self.flush_accepted_facts_on_shutdown(batch, processed_through.take()).await?;
                    break;
                }
            }
        }

        tracing::info!("ReactiveScheduler stopped");

        // Emit final statistics if enabled
        if self.config.collect_stats {
            let stats = self.shared.stats.read().await;
            self.emit_view_update(ViewUpdate::Stats {
                batch_count: stats.batch_count,
                facts_processed: stats.facts_processed,
                avg_batch_latency_ms: None,
            });
        }
        Ok(())
    }

    /// Close original ingress and process every accepted queue envelope before
    /// acknowledging graceful completion. Pending exact targets are never
    /// silently discarded merely because they have not entered a local batch.
    async fn flush_accepted_facts_on_shutdown(
        &mut self,
        mut batch: Vec<Fact>,
        mut processed_through: Option<u64>,
    ) -> Result<(), AuraError> {
        self.fact_rx.close();
        while let Some(source) = self.fact_rx.recv().await {
            let (facts, sequence) = self
                .extract_facts(source)
                .map_err(required_scheduler_source)?;
            if let Some(sequence) = sequence {
                processed_through =
                    Some(processed_through.map_or(sequence, |previous| previous.max(sequence)));
            }
            batch.extend(facts);
            if batch.len() >= self.config.max_batch_size {
                self.process_batch(std::mem::take(&mut batch), processed_through.take())
                    .await?;
            }
        }
        if !batch.is_empty() {
            self.process_batch(batch, processed_through.take()).await?;
        } else {
            self.acknowledge_processed(processed_through.take());
        }
        Ok(())
    }

    /// Process configured views in topological order before exact acknowledgment.
    async fn process_batch(
        &self,
        facts: Vec<Fact>,
        processed_through: Option<u64>,
    ) -> Result<(), AuraError> {
        if facts.is_empty() {
            self.acknowledge_processed(processed_through);
            return Ok(());
        }

        self.inspect_generic_facts(&facts);

        let fact_count = facts.len();

        tracing::trace!("Processing batch of {} facts", fact_count);

        // Update all views in topological order
        // This guarantees glitch-freedom: downstream views only see consistent state
        for view in &self.views {
            view.update(&facts).await?;
        }

        self.acknowledge_processed(processed_through);

        // Emit update event
        self.emit_view_update(ViewUpdate::Batch { count: fact_count });

        // Counts reflect actual processing. Optional latency is unavailable here;
        // metric clock sampling cannot delay required processing acknowledgment.
        if self.config.collect_stats {
            let mut stats = self.shared.stats.write().await;
            stats.batch_count += 1;
            stats.facts_processed += fact_count as u64;
        }
        Ok(())
    }

    fn inspect_generic_facts(&self, facts: &[Fact]) {
        for fact in facts {
            if let FactContent::Relational(RelationalFact::Generic {
                context_id,
                envelope,
            }) = &fact.content
            {
                let binding = self.fact_registry.reduce_envelope(*context_id, envelope);
                let binding_type_desc = format!("{:?}", binding.binding_type);
                let context_display = context_id.to_string();
                tracing::trace!(
                    fact_type = envelope.type_id.as_str(),
                    binding = binding_type_desc,
                    ctx = context_display,
                    "reduced generic fact"
                );
            }
        }
    }

    /// Extract facts from a fact source
    fn acknowledge_processed(&self, through: Option<u64>) {
        if let Some(sequence) = through {
            self.processed_tx.send_modify(|processed| {
                processed.processed = processed.processed.max(sequence);
            });
        }
    }

    fn extract_facts(
        &self,
        source: FactSource,
    ) -> Result<(Vec<Fact>, Option<u64>), publication::FactProcessingError> {
        Ok(match source {
            FactSource::Journal(facts) => (facts, None),
            FactSource::Network(facts) => (facts, None),
            FactSource::Timer(facts) => (facts, None),
            FactSource::Published(publication) => {
                self.original_ingress
                    .require_publication_owner(&publication)?;
                let (facts, sequence) = publication.into_parts();
                (facts, Some(sequence))
            }
        })
    }

    fn emit_view_update(&self, update: ViewUpdate) {
        let update_kind = match &update {
            ViewUpdate::Batch { .. } => "batch",
            ViewUpdate::ViewChanged { .. } => "view_changed",
            ViewUpdate::Stats { .. } => "stats",
        };

        if self.update_tx.send(update).is_err() {
            tracing::debug!(
                event = "runtime.reactive.view_update_dropped",
                reason = "no_subscribers",
                update_kind,
                "Reactive scheduler dropped a non-fatal view update because no subscribers were attached"
            );
        }
    }
}

// =============================================================================
// View Adapter - Bridges ReactiveView with apply_delta() pattern
// =============================================================================

/// Adapter that implements ReactiveView for views with apply_delta() methods
///
/// This bridges the gap between:
/// - ReactiveView trait (used by scheduler)
/// - Views with apply_delta(Delta) methods (TUI views)
///
/// ## Example
///
/// ```ignore
/// let chat_view = Arc::new(ChatView::new());
/// let chat_adapter = ViewAdapter::new(
///     "chat",
///     ChatReduction,
///     chat_view.clone(),
///     |view, delta| async move { view.apply_delta(delta).await },
///     own_authority,  // Pass own_authority for contextual reduction
/// );
/// scheduler.register_view(Arc::new(chat_adapter));
/// ```
pub struct ViewAdapter<Delta, R, V>
where
    Delta: Send + Sync + 'static,
    R: ViewReduction<Delta>,
    V: Send + Sync,
{
    /// View identifier
    view_id: String,
    /// Reduction function (facts → deltas)
    reduction: R,
    /// View reference
    view: Arc<V>,
    /// Delta application function (view + delta → ())
    apply_fn: ApplyFn<V, Delta>,
    /// Own authority for contextual reduction
    own_authority: Option<AuthorityId>,
}

impl<Delta, R, V> ViewAdapter<Delta, R, V>
where
    Delta: Send + Sync + 'static,
    R: ViewReduction<Delta>,
    V: Send + Sync,
{
    /// Create a new view adapter
    ///
    /// # Parameters
    /// - `view_id`: Unique identifier for dependency ordering
    /// - `reduction`: Pure function that converts facts to deltas
    /// - `view`: The actual view instance
    /// - `apply_fn`: Function that applies a delta to the view
    /// - `own_authority`: The current user's authority ID for contextual reduction
    pub fn new<F, Fut>(
        view_id: impl Into<String>,
        reduction: R,
        view: Arc<V>,
        apply_fn: F,
        own_authority: Option<AuthorityId>,
    ) -> Self
    where
        F: Fn(&V, Delta) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<(), AuraError>> + Send + 'static,
    {
        Self {
            view_id: view_id.into(),
            reduction,
            view,
            apply_fn: Arc::new(move |v, d| Box::pin(apply_fn(v, d))),
            own_authority,
        }
    }
}

impl<Delta, R, V> ReactiveView for ViewAdapter<Delta, R, V>
where
    Delta: Send + Sync + 'static,
    R: ViewReduction<Delta> + 'static,
    V: Send + Sync + 'static,
{
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            // Step 1: Reduce facts to deltas (passing own_authority for contextual reduction)
            let deltas = self.reduction.reduce(facts, self.own_authority);

            // Step 2: Apply each delta to the view
            for delta in deltas {
                (self.apply_fn)(&self.view, delta).await?;
            }
            Ok(())
        })
    }

    fn view_id(&self) -> &str {
        &self.view_id
    }
}

// =============================================================================
// Topological Sort for View Dependencies
// =============================================================================

/// Wrapper type that implements `DagNode` for `Arc<dyn AnyView>`.
///
/// This wrapper allows us to use the generic `DagNode` trait with
/// view types. The wrapper is transparent and simply delegates to
/// the underlying `AnyView` methods.
#[derive(Clone)]
pub struct ViewNode(pub Arc<dyn AnyView>);

impl DagNode for ViewNode {
    type Id = String;

    fn dag_id(&self) -> Self::Id {
        self.0.view_id().to_string()
    }

    fn dag_dependencies(&self) -> Vec<Self::Id> {
        self.0.dependencies()
    }
}

/// Generic topological sort for any type implementing `DagNode`.
///
/// Uses Kahn's algorithm for linear-time topological sorting.
/// Returns `Ok(sorted_nodes)` on success, or `Err(CycleError)` if
/// a cycle is detected.
///
/// # Type Parameters
///
/// - `T`: The node type, must implement `DagNode`
///
/// # Example
///
/// ```ignore
/// let nodes = vec![ViewNode(view_a), ViewNode(view_b), ViewNode(view_c)];
/// let sorted = topological_sort_dag(nodes)?;
/// ```
pub fn topological_sort_dag<T>(nodes: Vec<T>) -> Result<Vec<T>, CycleError<T::Id>>
where
    T: DagNode,
    T::Id: std::fmt::Debug,
{
    use std::collections::{HashMap, VecDeque};

    if nodes.is_empty() {
        return Ok(nodes);
    }

    // Build id -> index mapping
    let id_to_idx: HashMap<T::Id, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.dag_id(), i))
        .collect();

    // Build one_hop_link list: for each node, which nodes depend on it (outgoing edges)
    // If A depends on B, then B -> A (B must come before A)
    let mut in_degree: Vec<usize> = vec![0; nodes.len()];
    let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];

    for (idx, node) in nodes.iter().enumerate() {
        for dep_id in node.dag_dependencies() {
            if let Some(&dep_idx) = id_to_idx.get(&dep_id) {
                // This node (idx) depends on dep_idx
                // So dep_idx -> idx edge (dep_idx must be processed first)
                dependents[dep_idx].push(idx);
                in_degree[idx] += 1;
            }
            // Ignore dependencies on nodes not in our list
        }
    }

    // Kahn's algorithm: start with nodes that have no dependencies
    let mut queue: VecDeque<usize> = in_degree
        .iter()
        .enumerate()
        .filter(|(_, &deg)| deg == 0)
        .map(|(i, _)| i)
        .collect();

    let mut sorted_indices: Vec<usize> = Vec::with_capacity(nodes.len());

    while let Some(idx) = queue.pop_front() {
        sorted_indices.push(idx);

        for &dependent_idx in &dependents[idx] {
            in_degree[dependent_idx] -= 1;
            if in_degree[dependent_idx] == 0 {
                queue.push_back(dependent_idx);
            }
        }
    }

    // Check for cycles
    if sorted_indices.len() != nodes.len() {
        // Find which nodes are in the cycle for debugging
        let cycle_members: Vec<_> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, &deg)| deg > 0)
            .map(|(i, _)| nodes[i].dag_id())
            .collect();
        return Err(CycleError { cycle_members });
    }

    // Build sorted node list
    // We need to take ownership, so we use a trick with Option
    let mut nodes_opt: Vec<Option<T>> = nodes.into_iter().map(Some).collect();
    Ok(sorted_indices
        .into_iter()
        .map(|i| {
            nodes_opt[i]
                .take()
                .unwrap_or_else(|| unreachable!("sorted_indices are derived from nodes_opt"))
        })
        .collect())
}

/// Sort views in topological order based on their dependencies.
///
/// This guarantees glitch-freedom by ensuring that if view A depends on view B,
/// view B will be updated before view A. Uses Kahn's algorithm for linear-time
/// topological sorting.
///
/// # Panics
///
/// Panics if there is a dependency cycle among views.
fn topological_sort(views: Vec<Arc<dyn AnyView>>) -> Vec<Arc<dyn AnyView>> {
    // Wrap views as DagNode implementors
    let nodes: Vec<ViewNode> = views.into_iter().map(ViewNode).collect();

    // Use generic topological sort
    match topological_sort_dag(nodes) {
        Ok(sorted) => sorted.into_iter().map(|vn| vn.0).collect(),
        Err(cycle_error) => {
            panic!(
                "Dependency cycle detected among views: {:?}",
                cycle_error.cycle_members
            );
        }
    }
}

// =============================================================================
// Delta Types and Reduction Functions
// =============================================================================
//

fn required_scheduler_source(source: impl std::error::Error + Send + Sync + 'static) -> AuraError {
    AuraError::Internal {
        message: "required reactive scheduler operation failed".into(),
        source: Some(Arc::new(source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fact_registry::build_fact_registry;
    use crate::reactive::reductions::{
        ChatReduction, GuardianDelta, GuardianReduction, HomeDelta, HomeReduction,
        InvitationReduction, RecoveryReduction,
    };
    use aura_chat::{ChatDelta, ChatFact};
    use aura_core::{
        time::{OrderTime, PhysicalTime, TimeStamp},
        AuthorityId, ChannelId, ContextId, InvitationId,
    };
    use aura_invitation::{InvitationDelta, InvitationFact};
    use aura_journal::fact::{FactContent, RelationalFact};
    use aura_journal::DomainFact;
    use aura_recovery::RecoveryDelta;
    use aura_social::{HomeId, SocialFact};
    use tokio::sync::mpsc;

    /// Helper to create a test context ID
    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([0u8; 32])
    }

    /// Helper to create a test home ID
    fn test_home_id() -> HomeId {
        HomeId::from_bytes([1u8; 32])
    }

    /// Helper to create a test authority ID
    fn test_authority_id() -> AuthorityId {
        AuthorityId::new_from_entropy([2u8; 32])
    }

    /// Helper to create test facts with unique order tokens
    fn make_test_fact(order_index: u64, content: FactContent) -> Fact {
        // Create unique order token based on index
        let mut order_bytes = [0u8; 32];
        order_bytes[..8].copy_from_slice(&order_index.to_be_bytes());
        let order = OrderTime(order_bytes);

        // Use physical timestamp for test facts
        let timestamp = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000 + order_index,
            uncertainty: None,
        });

        Fact::new(order, timestamp, content)
    }

    struct SecondUpdateGate {
        count: Arc<RwLock<usize>>,
        release: Arc<tokio::sync::Notify>,
    }
    impl ReactiveView for SecondUpdateGate {
        fn update<'a>(&'a self, _facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
            Box::pin(async move {
                let ordinal = {
                    let mut count = self.count.write().await;
                    *count += 1;
                    *count
                };
                if ordinal == 2 {
                    self.release.notified().await;
                }
                Ok(())
            })
        }
        fn view_id(&self) -> &str {
            "exact-target-gated-view"
        }
    }

    #[tokio::test]
    async fn required_processing_target_ignores_older_batch_and_retains_completed_highwater() {
        use futures::FutureExt;
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(1_000));
        let (mut scheduler, ingress, shutdown) = ReactiveScheduler::new(
            SchedulerConfig {
                batch_window: Duration::ZERO,
                max_batch_size: 1,
                ..Default::default()
            },
            Arc::new(build_fact_registry()),
            clock.clone(),
        );
        let count = Arc::new(RwLock::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        scheduler.register_view(Arc::new(SecondUpdateGate {
            count: count.clone(),
            release: release.clone(),
        }));
        let fact = make_test_fact(
            91,
            FactContent::Relational(RelationalFact::Generic {
                context_id: test_context_id(),
                envelope: aura_core::types::facts::FactEnvelope {
                    type_id: aura_core::types::facts::FactTypeId::from("target_test"),
                    schema_version: 1,
                    encoding: aura_core::types::facts::FactEncoding::DagCbor,
                    payload: vec![1],
                },
            }),
        );
        let first = ingress
            .publish_required(vec![fact.clone()])
            .await
            .expect("original accepted publication");
        let second = ingress
            .publish_required(vec![fact])
            .await
            .expect("later accepted publication");
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 1_000,
                uncertainty: None,
            },
            Duration::from_secs(1),
        )
        .expect("original readiness window");
        let run = scheduler.run();
        tokio::pin!(run);
        assert!(run.as_mut().now_or_never().is_none());
        assert_eq!(
            *count.read().await,
            2,
            "later actual view update has entered but not completed"
        );
        first
            .await_processed(&ingress, clock.as_ref(), &budget)
            .await
            .expect("retained completed target works even after earlier notification");
        let later = second.await_processed(&ingress, clock.as_ref(), &budget);
        tokio::pin!(later);
        assert!(
            later.as_mut().now_or_never().is_none(),
            "older completed batch cannot satisfy later target"
        );
        release.notify_one();
        assert!(run.as_mut().now_or_never().is_none());
        later
            .await
            .expect("actual second view completion acknowledges later target");
        shutdown
            .send(())
            .await
            .expect("owned test shutdown ingress");
        run.await.expect("actual scheduler completed");
    }

    #[tokio::test]
    async fn required_processing_target_closed_scheduler_retains_native_watch_source() {
        let clock = aura_testkit::time::ManualPhysicalClock::new(2_000);
        let (scheduler, ingress, _) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            Arc::new(clock.clone()),
        );
        let target = ingress
            .publish_required(Vec::new())
            .await
            .expect("accepted original no-op still requires scheduler acknowledgment");
        drop(scheduler);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 2_000,
                uncertainty: None,
            },
            Duration::from_secs(1),
        )
        .expect("original wait window");
        let failure = target
            .await_processed(&ingress, &clock, &budget)
            .await
            .expect_err("unprocessed target cannot claim readiness after scheduler Drop");
        assert!(matches!(
            failure,
            aura_core::time::timeout::TimeoutRunError::Operation(
                publication::FactProcessingError::SchedulerStopped { .. }
            )
        ));
        let source = std::error::Error::source(&failure)
            .and_then(|cause| cause.source())
            .expect("original native watch cause");
        assert!(source
            .downcast_ref::<tokio::sync::watch::error::RecvError>()
            .is_some());
    }

    #[tokio::test]
    async fn required_processing_target_preserves_actual_scheduler_clock_failure() {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(3_000));
        let (scheduler, ingress, _shutdown) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            clock.clone(),
        );
        let fact = make_test_fact(
            92,
            FactContent::Relational(RelationalFact::Generic {
                context_id: test_context_id(),
                envelope: aura_core::types::facts::FactEnvelope {
                    type_id: aura_core::types::facts::FactTypeId::from("target_clock_test"),
                    schema_version: 1,
                    encoding: aura_core::types::facts::FactEncoding::DagCbor,
                    payload: vec![2],
                },
            }),
        );
        let target = ingress
            .publish_required(vec![fact])
            .await
            .expect("accepted actual publication");
        clock
            .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                reason: "scheduler provider fixture fault".into(),
            })
            .await;
        scheduler
            .run()
            .await
            .expect_err("required batch scheduling read failed before processing");
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 3_000,
                uncertainty: None,
            },
            Duration::from_secs(1),
        )
        .expect("original processing window");
        let failure = target
            .await_processed(&ingress, clock.as_ref(), &budget)
            .await
            .expect_err("actual scheduler failure is not processing success");
        let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut native = None;
        while let Some(source) = cursor {
            if let Some(cause) = source.downcast_ref::<aura_core::effects::TimeError>() {
                native = Some(cause);
                break;
            }
            cursor = source.source();
        }
        assert!(
            matches!(native, Some(aura_core::effects::TimeError::OperationFailed { reason }) if reason == "scheduler provider fixture fault")
        );
    }

    #[tokio::test]
    async fn required_processing_target_original_deadline_and_foreign_owner_fail_closed() {
        use futures::FutureExt;
        let clock = aura_testkit::time::ManualPhysicalClock::new(4_000);
        let (_scheduler, ingress, _) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            Arc::new(clock.clone()),
        );
        let (_foreign_scheduler, foreign, _) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            Arc::new(clock.clone()),
        );
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 4_000,
                uncertainty: None,
            },
            Duration::from_millis(100),
        )
        .expect("original operation window");
        let foreign_target = ingress
            .publish_required(Vec::new())
            .await
            .expect("original accepted target");
        let foreign_failure = foreign_target
            .await_processed(&foreign, &clock, &budget)
            .await
            .expect_err("another scheduler cannot stand in for original processed owner");
        assert!(matches!(
            foreign_failure,
            aura_core::time::timeout::TimeoutRunError::Operation(
                publication::FactProcessingError::ForeignOwner
            )
        ));
        let target = ingress
            .publish_required(Vec::new())
            .await
            .expect("next original accepted target");
        let wait = target.await_processed(&ingress, &clock, &budget);
        tokio::pin!(wait);
        assert!(wait.as_mut().now_or_never().is_none());
        clock.set_time(4_100);
        let deadline = wait
            .await
            .expect_err("unprocessed original target times out without fresh budget");
        assert!(matches!(
            deadline,
            aura_core::time::timeout::TimeoutRunError::Timeout(
                TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 4_100,
                    ..
                }
            )
        ));
    }

    #[tokio::test]
    async fn failed_publication_envelope_cannot_advance_foreign_scheduler_highwater() {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000));
        let (original, original_ingress, _) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            clock.clone(),
        );
        drop(original);
        let failed = original_ingress
            .publish_required(Vec::new())
            .await
            .err()
            .expect("actual original closed sink produces native failed-send envelope");
        let publication::FactProcessingError::SinkClosed { source } = failed else {
            panic!("actual required queue is closed");
        };
        let (foreign, foreign_ingress, _) = ReactiveScheduler::new(
            SchedulerConfig::default(),
            Arc::new(build_fact_registry()),
            clock.clone(),
        );
        foreign_ingress
            .send(source.0)
            .await
            .expect("adversarial replay reaches real foreign consumer");
        let target = foreign_ingress
            .publish_required(Vec::new())
            .await
            .expect("foreign real publication selects its own target");
        foreign.run().await.expect_err(
            "original-envelope identity mismatch fails before any foreign sequence acknowledgment",
        );
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime {
                ts_ms: 5_000,
                uncertainty: None,
            },
            Duration::from_secs(1),
        )
        .expect("original foreign operation budget");
        let failure = target
            .await_processed(&foreign_ingress, clock.as_ref(), &budget)
            .await
            .expect_err("foreign raw replay cannot manufacture own target completion");
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&failure);
        let mut found = false;
        while let Some(cause) = source {
            if matches!(
                cause.downcast_ref::<publication::FactProcessingError>(),
                Some(publication::FactProcessingError::ForeignOwner)
            ) {
                found = true;
                break;
            }
            source = cause.source();
        }
        assert!(
            found,
            "actual consumer retains original typed foreign-owner cause"
        );
    }

    /// Mock view for testing
    struct MockView {
        id: String,
        update_count: Arc<RwLock<usize>>,
    }

    impl ReactiveView for MockView {
        fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
            Box::pin(async move {
                let mut count = self.update_count.write().await;
                *count += facts.len();
                Ok(())
            })
        }

        fn view_id(&self) -> &str {
            &self.id
        }
    }

    #[tokio::test]
    async fn test_scheduler_basic_flow() {
        let config = SchedulerConfig::default();
        let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

        // Register a mock view
        let update_count = Arc::new(RwLock::new(0));
        let view = Arc::new(MockView {
            id: "test_view".to_string(),
            update_count: update_count.clone(),
        });
        scheduler.register_view(view);

        // Subscribe to updates
        let _update_rx = scheduler.subscribe();

        let scheduler_task = scheduler.run();
        let exercise = async {
            let facts = vec![make_test_fact(
                1,
                FactContent::Relational(RelationalFact::Generic {
                    context_id: test_context_id(),
                    envelope: aura_core::types::facts::FactEnvelope {
                        type_id: aura_core::types::facts::FactTypeId::from("test_fact"),
                        schema_version: 1,
                        encoding: aura_core::types::facts::FactEncoding::DagCbor,
                        payload: vec![1, 2, 3],
                    },
                }),
            )];
            fact_tx.send(FactSource::Journal(facts)).await.unwrap();

            let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
            let time = aura_effects::time::PhysicalTimeHandler::new();
            loop {
                if *update_count.read().await == 1 {
                    break;
                }
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "timed out waiting for scheduler to apply fact"
                );
                time.sleep_ms(10).await.unwrap();
            }

            shutdown_tx.send(()).await.unwrap();
            drop(fact_tx); // Close channel to allow scheduler to exit
        };

        let (result, ()) = tokio::join!(scheduler_task, exercise);
        result.expect("actual scheduler ownership completed without hidden failure");
    }

    fn scheduler_with_registry(
        config: SchedulerConfig,
    ) -> (ReactiveScheduler, FactIngress, mpsc::Sender<()>) {
        use aura_effects::time::PhysicalTimeHandler;
        let time_effects = Arc::new(PhysicalTimeHandler);
        let (scheduler, fact_tx, shutdown_tx) =
            ReactiveScheduler::new(config, Arc::new(build_fact_registry()), time_effects);
        (scheduler, fact_tx, shutdown_tx)
    }

    #[test]
    fn test_chat_reduction() {
        let reduction = ChatReduction;

        // Create proper ChatFact instances using backward-compatible constructors
        let channel_fact = ChatFact::channel_created_ms(
            test_context_id(),
            ChannelId::default(),
            "general".to_string(),
            Some("General discussion".to_string()),
            false,
            1234567890,
            AuthorityId::new_from_entropy([1u8; 32]),
        );

        let message_fact = ChatFact::message_sent_sealed_ms(
            test_context_id(),
            ChannelId::default(),
            "msg-123".to_string(),
            AuthorityId::new_from_entropy([2u8; 32]),
            "Alice".to_string(),
            b"Hello, world!".to_vec(),
            1234567900,
            None,
            Some(1), // epoch_hint
        );

        // Test with facts that should produce deltas
        let facts = vec![
            make_test_fact(1, FactContent::Relational(channel_fact.to_generic())),
            make_test_fact(2, FactContent::Relational(message_fact.to_generic())),
            // A non-chat fact that should be ignored
            make_test_fact(
                3,
                FactContent::Relational(RelationalFact::Generic {
                    context_id: test_context_id(),
                    envelope: aura_core::types::facts::FactEnvelope {
                        type_id: aura_core::types::facts::FactTypeId::from("other_action"),
                        schema_version: 1,
                        encoding: aura_core::types::facts::FactEncoding::DagCbor,
                        payload: vec![7, 8, 9],
                    },
                }),
            ),
        ];

        // Use a test authority for contextual reduction
        let test_authority = Some(AuthorityId::new_from_entropy([99u8; 32]));
        let deltas = reduction.reduce(&facts, test_authority);

        // Should produce 2 deltas (channel and message)
        assert_eq!(deltas.len(), 2);
        assert!(
            matches!(&deltas[0], ChatDelta::ChannelAdded(creation) if creation.name() == "general")
        );
        assert!(
            matches!(&deltas[1], ChatDelta::MessageAdded { content, .. } if content == "<sealed message>")
        );

        // Test determinism: same facts → same deltas
        let deltas2 = reduction.reduce(&facts, test_authority);
        assert_eq!(deltas, deltas2);

        // Test monotonicity: more facts → same or more deltas
        let another_message = ChatFact::message_sent_sealed_ms(
            test_context_id(),
            ChannelId::default(),
            "msg-124".to_string(),
            AuthorityId::new_from_entropy([3u8; 32]),
            "Bob".to_string(),
            b"Reply here".to_vec(),
            1234567910,
            Some("msg-123".to_string()),
            Some(1), // epoch_hint
        );

        let mut more_facts = facts.clone();
        more_facts.push(make_test_fact(
            4,
            FactContent::Relational(another_message.to_generic()),
        ));

        let more_deltas = reduction.reduce(&more_facts, test_authority);
        assert!(more_deltas.len() >= deltas.len());
        assert_eq!(more_deltas.len(), 3);
    }

    #[tokio::test]
    async fn test_scheduler_batching() {
        let config = SchedulerConfig {
            batch_window: Duration::from_millis(20),
            ..Default::default()
        };
        let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);

        let update_count = Arc::new(RwLock::new(0));
        let view = Arc::new(MockView {
            id: "test_view".to_string(),
            update_count: update_count.clone(),
        });
        scheduler.register_view(view);

        let scheduler_task = scheduler.run();
        let exercise = async {
            for i in 0..5 {
                fact_tx
                    .send(FactSource::Journal(vec![make_test_fact(
                        i,
                        FactContent::Relational(RelationalFact::Generic {
                            context_id: test_context_id(),
                            envelope: aura_core::types::facts::FactEnvelope {
                                type_id: aura_core::types::facts::FactTypeId::from(
                                    format!("test_fact_{}", i).as_str(),
                                ),
                                schema_version: 1,
                                encoding: aura_core::types::facts::FactEncoding::DagCbor,
                                payload: vec![i as u8],
                            },
                        }),
                    )]))
                    .await
                    .unwrap();
            }

            aura_effects::time::PhysicalTimeHandler::new()
                .sleep_ms(30)
                .await
                .unwrap();

            assert_eq!(*update_count.read().await, 5);

            shutdown_tx.send(()).await.unwrap();
            drop(fact_tx);
        };

        let (result, ()) = tokio::join!(scheduler_task, exercise);
        result.expect("actual scheduler ownership completed without hidden failure");
    }

    #[test]
    fn test_guardian_reduction() {
        use aura_core::{types::identifiers::AuthorityId, Hash32};

        let reduction = GuardianReduction;
        let facts = vec![
            make_test_fact(
                1,
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding {
                        account_id: AuthorityId::new_from_entropy([1u8; 32]),
                        guardian_id: AuthorityId::new_from_entropy([2u8; 32]),
                        binding_hash: Hash32([0u8; 32]),
                    },
                )),
            ),
            make_test_fact(
                2,
                FactContent::Relational(RelationalFact::Generic {
                    context_id: test_context_id(),
                    envelope: aura_core::types::facts::FactEnvelope {
                        type_id: aura_core::types::facts::FactTypeId::from("threshold_updated"),
                        schema_version: 1,
                        encoding: aura_core::types::facts::FactEncoding::DagCbor,
                        payload: vec![2, 3],
                    },
                }),
            ),
        ];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 2);
        assert!(matches!(&deltas[0], GuardianDelta::GuardianAdded { .. }));
        assert!(matches!(
            &deltas[1],
            GuardianDelta::ThresholdUpdated {
                threshold: 2,
                total: 3
            }
        ));
    }

    #[test]
    fn test_recovery_reduction() {
        use aura_core::types::identifiers::AuthorityId;
        use aura_journal::DomainFact;
        use aura_recovery::RecoveryFact;

        let reduction = RecoveryReduction;

        // Create proper RecoveryFact instances using backward-compatible constructors
        let setup_initiated = RecoveryFact::guardian_setup_initiated_ms(
            test_context_id(),
            AuthorityId::new_from_entropy([1u8; 32]),
            vec![
                AuthorityId::new_from_entropy([2u8; 32]),
                AuthorityId::new_from_entropy([3u8; 32]),
            ],
            2,
            1234567890,
            aura_recovery::recovery_initiation_causal(
                test_context_id(),
                aura_recovery::RecoveryInitiationKind::GuardianSetup,
                &[],
                &aura_core::time::LogicalTime {
                    vector: aura_core::time::VectorClock::new(),
                    lamport: 1,
                },
            ),
        );

        let guardian_accepted = RecoveryFact::guardian_accepted_ms(
            test_context_id(),
            AuthorityId::new_from_entropy([2u8; 32]),
            1234567900,
        );

        let setup_completed = RecoveryFact::guardian_setup_completed_ms(
            test_context_id(),
            vec![
                AuthorityId::new_from_entropy([2u8; 32]),
                AuthorityId::new_from_entropy([3u8; 32]),
            ],
            2,
            1234567999,
        );

        let facts = vec![
            make_test_fact(1, FactContent::Relational(setup_initiated.to_generic())),
            make_test_fact(2, FactContent::Relational(guardian_accepted.to_generic())),
            make_test_fact(3, FactContent::Relational(setup_completed.to_generic())),
        ];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 3);
        assert!(matches!(
            &deltas[0],
            RecoveryDelta::GuardianSetupStarted { .. }
        ));
        assert!(matches!(
            &deltas[1],
            RecoveryDelta::GuardianResponded { .. }
        ));
        assert!(matches!(
            &deltas[2],
            RecoveryDelta::GuardianSetupCompleted { .. }
        ));
    }

    #[test]
    fn test_invitation_reduction() {
        let reduction = InvitationReduction;

        // Create proper InvitationFact instances using backward-compatible constructors
        let sent_fact = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv-123"),
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
            aura_invitation::InvitationType::Contact { nickname: None },
            1234567890,
            None,
            None,
        );

        let accepted_fact = InvitationFact::accepted_ms(
            InvitationId::new("inv-123"),
            AuthorityId::new_from_entropy([2u8; 32]),
            1234567900,
        );

        let facts = vec![
            make_test_fact(1, FactContent::Relational(sent_fact.to_generic())),
            make_test_fact(2, FactContent::Relational(accepted_fact.to_generic())),
        ];

        // Use a test authority for invitation direction determination
        let test_authority = Some(AuthorityId::new_from_entropy([99u8; 32]));
        let deltas = reduction.reduce(&facts, test_authority);
        assert_eq!(deltas.len(), 2);
        assert!(matches!(
            &deltas[0],
            InvitationDelta::InvitationAdded { .. }
        ));
        assert!(matches!(
            &deltas[1],
            InvitationDelta::InvitationStatusChanged { new_status, .. }
                if *new_status == aura_invitation::InvitationStatus::Accepted
        ));
    }

    #[test]
    fn test_home_reduction() {
        let reduction = HomeReduction;

        // Create properly serialized SocialFact instances
        let home_created = SocialFact::home_created_ms(
            test_home_id(),
            test_context_id(),
            1000,
            test_authority_id(),
            "Test Home".to_string(),
        );

        let member_joined = SocialFact::member_joined_ms(
            test_authority_id(),
            test_home_id(),
            test_context_id(),
            2000,
            "Alice".to_string(),
            "inv".to_string(),
        );

        let storage_updated = SocialFact::storage_updated_ms(
            test_home_id(),
            test_context_id(),
            1024 * 1024,      // 1 MB used
            10 * 1024 * 1024, // 10 MB total
            3000,
            aura_core::time::CausalMetadata {
                revokes: Vec::new(),
                supersedes: Vec::new(),
                clock: aura_core::time::CausalClock {
                    lamport: 1,
                    vector: Vec::new(),
                },
            },
        );

        let facts = vec![
            make_test_fact(1, FactContent::Relational(home_created.to_generic())),
            make_test_fact(2, FactContent::Relational(member_joined.to_generic())),
            make_test_fact(3, FactContent::Relational(storage_updated.to_generic())),
        ];

        let deltas = reduction.reduce(&facts, None);
        assert_eq!(deltas.len(), 3);
        assert!(matches!(&deltas[0], HomeDelta::HomeCreated { name, .. } if name == "Test Home"));
        assert!(matches!(&deltas[1], HomeDelta::MemberAdded { name, .. } if name == "Alice"));
        assert!(
            matches!(&deltas[2], HomeDelta::StorageUpdated { used_bytes, total_bytes, .. }
            if *used_bytes == 1024 * 1024 && *total_bytes == 10 * 1024 * 1024)
        );
    }

    // =========================================================================
    // Topological Sort Tests
    // =========================================================================

    #[derive(Debug, thiserror::Error)]
    #[error("selected projection fixture failed")]
    struct SelectedProjectionFailure;

    struct FailingProjection;

    impl ReactiveView for FailingProjection {
        fn update<'a>(&'a self, _facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
            Box::pin(async {
                Err(AuraError::Internal {
                    message: "required projection failed".into(),
                    source: Some(Arc::new(SelectedProjectionFailure)),
                })
            })
        }

        fn view_id(&self) -> &str {
            "failing-projection"
        }
    }

    #[tokio::test]
    async fn required_projection_failure_preserves_native_cause_and_blocks_batch_ack() {
        use std::error::Error;
        let (mut scheduler, _ingress, _shutdown) =
            scheduler_with_registry(SchedulerConfig::default());
        let downstream_count = Arc::new(RwLock::new(0));
        scheduler.register_view(Arc::new(FailingProjection));
        scheduler.register_view(Arc::new(MockView {
            id: "downstream-projection".into(),
            update_count: downstream_count.clone(),
        }));
        let mut diagnostics = scheduler.subscribe();
        let failed = scheduler
            .process_batch(
                vec![make_test_fact(
                    1,
                    FactContent::Relational(RelationalFact::Generic {
                        context_id: test_context_id(),
                        envelope: aura_core::types::facts::FactEnvelope {
                            type_id: aura_core::types::facts::FactTypeId::from("test_fact"),
                            schema_version: 1,
                            encoding: aura_core::types::facts::FactEncoding::DagCbor,
                            payload: vec![1],
                        },
                    }),
                )],
                Some(1),
            )
            .await
            .expect_err("required projection must fail the batch");
        assert!(failed
            .source()
            .is_some_and(|source| source.is::<SelectedProjectionFailure>()));
        assert_eq!(*downstream_count.read().await, 0);
        assert_eq!(scheduler.processed_tx.borrow().processed, 0);
        assert!(matches!(
            diagnostics.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
    }

    /// Test view for topological sort testing
    #[tokio::test]
    async fn required_issued_target_retains_failed_projection_without_processing_ack() {
        use std::error::Error;
        let (mut scheduler, ingress, _shutdown) = scheduler_with_registry(SchedulerConfig {
            batch_window: Duration::ZERO,
            ..SchedulerConfig::default()
        });
        scheduler.register_view(Arc::new(FailingProjection));
        let target = ingress
            .publish_required(vec![make_test_fact(
                1,
                FactContent::Relational(RelationalFact::Generic {
                    context_id: test_context_id(),
                    envelope: aura_core::types::facts::FactEnvelope {
                        type_id: aura_core::types::facts::FactTypeId::from("test_fact"),
                        schema_version: 1,
                        encoding: aura_core::types::facts::FactEncoding::DagCbor,
                        payload: vec![1],
                    },
                }),
            )])
            .await
            .expect("actual admitted publication");
        scheduler
            .run()
            .await
            .expect_err("actual projection terminates scheduler");
        let time = aura_effects::time::PhysicalTimeHandler::new();
        let started = time
            .physical_time()
            .await
            .expect("original observation clock");
        let original = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))
            .expect("original target observation window");
        let failed = target
            .await_processed(&ingress, &time, &original)
            .await
            .expect_err("issued target cannot become processed after projection failure");
        let mut cause: Option<&(dyn Error + 'static)> = Some(&failed);
        let mut retained = false;
        while let Some(source) = cause {
            retained |= source.is::<SelectedProjectionFailure>();
            cause = source.source();
        }
        assert!(retained, "target retains actual projection source");
    }

    struct TestView {
        id: String,
        deps: Vec<String>,
    }

    impl TestView {
        fn new(id: &str) -> Self {
            Self {
                id: id.to_string(),
                deps: Vec::new(),
            }
        }

        fn with_deps(id: &str, deps: &[&str]) -> Self {
            Self {
                id: id.to_string(),
                deps: deps.iter().map(|s| s.to_string()).collect(),
            }
        }
    }

    impl ReactiveView for TestView {
        fn update<'a>(&'a self, _facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
            Box::pin(async { Ok(()) })
        }

        fn view_id(&self) -> &str {
            &self.id
        }

        fn dependencies(&self) -> Vec<String> {
            self.deps.clone()
        }
    }

    #[test]
    fn test_topological_sort_empty() {
        let views: Vec<Arc<dyn AnyView>> = vec![];
        let sorted = topological_sort(views);
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_topological_sort_single() {
        let views: Vec<Arc<dyn AnyView>> = vec![Arc::new(TestView::new("a"))];
        let sorted = topological_sort(views);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].view_id(), "a");
    }

    #[test]
    fn test_topological_sort_no_deps() {
        let views: Vec<Arc<dyn AnyView>> = vec![
            Arc::new(TestView::new("a")),
            Arc::new(TestView::new("b")),
            Arc::new(TestView::new("c")),
        ];
        let sorted = topological_sort(views);
        assert_eq!(sorted.len(), 3);
        // Order doesn't matter when there are no dependencies
    }

    #[test]
    fn test_topological_sort_linear_chain() {
        // c -> b -> a (c depends on b, b depends on a)
        let views: Vec<Arc<dyn AnyView>> = vec![
            Arc::new(TestView::with_deps("c", &["b"])),
            Arc::new(TestView::with_deps("b", &["a"])),
            Arc::new(TestView::new("a")),
        ];
        let sorted = topological_sort(views);
        assert_eq!(sorted.len(), 3);

        // Find positions
        let pos_a = sorted.iter().position(|v| v.view_id() == "a").unwrap();
        let pos_b = sorted.iter().position(|v| v.view_id() == "b").unwrap();
        let pos_c = sorted.iter().position(|v| v.view_id() == "c").unwrap();

        // a must come before b, b must come before c
        assert!(pos_a < pos_b, "a should come before b");
        assert!(pos_b < pos_c, "b should come before c");
    }

    #[test]
    fn test_topological_sort_diamond() {
        // Diamond dependency: d depends on b and c, both b and c depend on a
        //     a
        //    / \
        //   b   c
        //    \ /
        //     d
        let views: Vec<Arc<dyn AnyView>> = vec![
            Arc::new(TestView::with_deps("d", &["b", "c"])),
            Arc::new(TestView::with_deps("b", &["a"])),
            Arc::new(TestView::with_deps("c", &["a"])),
            Arc::new(TestView::new("a")),
        ];
        let sorted = topological_sort(views);
        assert_eq!(sorted.len(), 4);

        let pos_a = sorted.iter().position(|v| v.view_id() == "a").unwrap();
        let pos_b = sorted.iter().position(|v| v.view_id() == "b").unwrap();
        let pos_c = sorted.iter().position(|v| v.view_id() == "c").unwrap();
        let pos_d = sorted.iter().position(|v| v.view_id() == "d").unwrap();

        // a must come first
        assert!(pos_a < pos_b, "a should come before b");
        assert!(pos_a < pos_c, "a should come before c");
        // d must come last
        assert!(pos_b < pos_d, "b should come before d");
        assert!(pos_c < pos_d, "c should come before d");
    }

    #[test]
    fn test_topological_sort_ignores_missing_deps() {
        // a depends on "missing" which doesn't exist in the view list
        let views: Vec<Arc<dyn AnyView>> = vec![Arc::new(TestView::with_deps("a", &["missing"]))];
        let sorted = topological_sort(views);
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].view_id(), "a");
    }

    #[test]
    #[should_panic(expected = "Dependency cycle detected")]
    fn test_topological_sort_cycle_panics() {
        // Cycle: a -> b -> a
        let views: Vec<Arc<dyn AnyView>> = vec![
            Arc::new(TestView::with_deps("a", &["b"])),
            Arc::new(TestView::with_deps("b", &["a"])),
        ];
        topological_sort(views);
    }
    #[tokio::test]
    async fn required_processing_target_survives_actual_diagnostic_lag_and_coalescing() {
        let config = SchedulerConfig {
            batch_window: Duration::ZERO,
            ..SchedulerConfig::default()
        };
        let (mut scheduler, ingress, shutdown) = scheduler_with_registry(config);
        let count = Arc::new(RwLock::new(0));
        scheduler.register_view(Arc::new(MockView {
            id: "actual-lag-view".into(),
            update_count: count.clone(),
        }));
        let mut diagnostics = scheduler.subscribe();
        let time = aura_effects::time::PhysicalTimeHandler::new();
        let started = time
            .physical_time()
            .await
            .expect("actual original lag observation");
        let original = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))
            .expect("original bounded processing scenario");
        let publications = VIEW_UPDATE_CHANNEL_CAPACITY + 1;
        let exercise = async {
            let mut first = None;
            let mut last = None;
            for index in 0..publications {
                let target = ingress
                    .publish_required(vec![make_test_fact(
                        u64::try_from(index).expect("bounded fixture index"),
                        FactContent::Relational(RelationalFact::Generic {
                            context_id: test_context_id(),
                            envelope: aura_core::types::facts::FactEnvelope {
                                type_id: aura_core::types::facts::FactTypeId::from("test_fact"),
                                schema_version: 1,
                                encoding: aura_core::types::facts::FactEncoding::DagCbor,
                                payload: vec![1],
                            },
                        }),
                    )])
                    .await
                    .expect("actual accepted original ingress");
                if index == 0 {
                    first = Some(target);
                } else if let Some(previous) = last.replace(target) {
                    previous.acknowledge_observed_only();
                }
            }
            last.expect("actual final target")
                .await_processed(&ingress, &time, &original)
                .await
                .expect("actual latest target processing acknowledgment");
            first
                .expect("actual original retained target")
                .await_processed(&ingress, &time, &original)
                .await
                .expect("coalesced retained highwater acknowledges previously completed target");
            assert!(
                matches!(
                    diagnostics.recv().await,
                    Err(broadcast::error::RecvError::Lagged(_))
                ),
                "diagnostic subscriber genuinely missed intermediate batches"
            );
            assert_eq!(*count.read().await, publications);
            shutdown.send(()).await.expect("owned scheduler shutdown");
        };
        let (result, ()) = futures::join!(scheduler.run(), exercise);
        result.expect("actual scheduler owner completion");
    }

    #[tokio::test]
    async fn required_processing_cancelled_blocked_enqueue_releases_original_sequence_owner() {
        use futures::FutureExt;
        let (scheduler, ingress, shutdown) = scheduler_with_registry(SchedulerConfig::default());
        // Fill the real bounded queue before the consumer is first polled.
        for _ in 0..256 {
            ingress
                .publish_required(Vec::new())
                .await
                .expect("actual accepted queue entry")
                .acknowledge_observed_only();
        }
        let mut blocked = Box::pin(ingress.publish_required(Vec::new()));
        assert!(
            blocked.as_mut().now_or_never().is_none(),
            "real queue backpressure retains pending ownership"
        );
        drop(blocked);
        let time = aura_effects::time::PhysicalTimeHandler::new();
        let started = time
            .physical_time()
            .await
            .expect("actual cancellation observation");
        let original = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))
            .expect("original bounded cancellation scenario");
        let exercise = async {
            let target = ingress
                .publish_required(Vec::new())
                .await
                .expect("cancellation releases original enqueue custody");
            target
                .await_processed(&ingress, &time, &original)
                .await
                .expect("actual newly accepted target resumes through original owner");
            shutdown.send(()).await.expect("owned scheduler shutdown");
        };
        let (result, ()) = futures::join!(scheduler.run(), exercise);
        result.expect("actual scheduler owner completion");
    }

    #[tokio::test]
    async fn required_graceful_shutdown_processes_accepted_queued_ingress_before_completion() {
        let config = SchedulerConfig {
            batch_window: Duration::from_secs(60),
            max_batch_size: 2,
            ..SchedulerConfig::default()
        };
        let (mut scheduler, fact_tx, shutdown_tx) = scheduler_with_registry(config);
        let update_count = Arc::new(RwLock::new(0));
        scheduler.register_view(Arc::new(MockView {
            id: "actual-shutdown-view".into(),
            update_count: update_count.clone(),
        }));
        // Both ingress and shutdown are ready before the first scheduler poll.
        // Selecting shutdown must drain accepted queued facts, whichever branch
        // was selected first; this is not merely a local-batch flush.
        for index in 0..4 {
            fact_tx
                .send(FactSource::Journal(vec![make_test_fact(
                    index,
                    FactContent::Relational(RelationalFact::Generic {
                        context_id: test_context_id(),
                        envelope: aura_core::types::facts::FactEnvelope {
                            type_id: aura_core::types::facts::FactTypeId::from("test_fact"),
                            schema_version: 1,
                            encoding: aura_core::types::facts::FactEncoding::DagCbor,
                            payload: vec![u8::try_from(index).expect("bounded fixture index")],
                        },
                    }),
                )]))
                .await
                .expect("actual accepted scheduler ingress");
        }
        shutdown_tx
            .send(())
            .await
            .expect("actual graceful shutdown signal");
        scheduler
            .run()
            .await
            .expect("actual graceful scheduler completion");
        assert_eq!(
            *update_count.read().await,
            4,
            "owned completion follows all accepted view processing"
        );
        assert!(
            fact_tx.send(FactSource::Journal(Vec::new())).await.is_err(),
            "original ingress is closed after acknowledged destruction"
        );
    }
}
