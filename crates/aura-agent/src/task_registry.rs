//! Structured task supervision for agent background work.
//!
//! This module provides a root supervisor plus named task groups. Tasks are
//! owned by a group, inherit cancellation from their ancestors, and must exit
//! before the group is considered drained.

#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::runtime::{
    RuntimeDiagnostic, RuntimeDiagnosticKind, RuntimeDiagnosticSeverity, RuntimeDiagnosticSink,
};
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{
    execute_with_timeout_budget, OwnedShutdownToken, OwnedTaskHandle, TimeoutBudget,
    TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;
use futures::future::{BoxFuture, LocalBoxFuture};
use futures::FutureExt;
#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;
#[cfg(target_arch = "wasm32")]
use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::sync::Notify;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

const DEFAULT_TASK_NAME: &str = "task.default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskSupervisionError {
    Timeout {
        group: String,
        active_tasks: Vec<String>,
    },
    ForcedAbort {
        group: String,
        aborted_tasks: Vec<String>,
    },
    Cancelled {
        group: String,
        task: String,
    },
    Panicked {
        group: String,
        task: String,
    },
}

impl std::fmt::Display for TaskSupervisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout {
                group,
                active_tasks,
            } => write!(
                f,
                "task group '{group}' timed out waiting for tasks: {}",
                active_tasks.join(", ")
            ),
            Self::ForcedAbort {
                group,
                aborted_tasks,
            } => write!(
                f,
                "task group '{group}' force-aborted tasks: {}",
                aborted_tasks.join(", ")
            ),
            Self::Cancelled { group, task } => {
                write!(f, "task '{task}' in group '{group}' was cancelled")
            }
            Self::Panicked { group, task } => {
                write!(f, "task '{task}' in group '{group}' panicked")
            }
        }
    }
}

impl std::error::Error for TaskSupervisionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TaskOutcome {
    Completed,
    Cancelled,
    Panicked,
}

#[derive(Debug)]
struct TaskMetadata {
    task_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    handle: Option<JoinHandle<()>>,
}

struct TaskGroupShared {
    name: String,
    next_task_id: AtomicU64,
    shutdown_tx: watch::Sender<bool>,
    inherited_cancellation: Option<Arc<dyn CancellationToken>>,
    diagnostics: Option<Arc<RuntimeDiagnosticSink>>,
    tasks: Mutex<BTreeMap<u64, TaskMetadata>>,
    notify: Arc<Notify>,
}

#[derive(Clone)]
pub struct TaskGroup {
    shared: Arc<TaskGroupShared>,
}

#[derive(Clone)]
pub struct TaskSupervisor {
    root: TaskGroup,
}

impl TaskSupervisor {
    pub fn new() -> Self {
        Self {
            root: TaskGroup::root("runtime", None),
        }
    }

    pub fn with_diagnostics(diagnostics: Arc<RuntimeDiagnosticSink>) -> Self {
        Self {
            root: TaskGroup::root("runtime", Some(diagnostics)),
        }
    }

    pub fn group(&self, name: impl Into<String>) -> TaskGroup {
        self.root.group(name)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.root.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.root.spawn_cancellable_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.root.spawn_local_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.root.spawn_local_cancellable_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.root
            .spawn_interval_until_named(name, time_effects, interval, f)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + 'static,
        Fut: Future<Output = bool> + 'static,
    {
        self.root
            .spawn_local_interval_until_named(name, time_effects, interval, f)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_child<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_periodic<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.spawn_interval_until_named(name, time_effects, interval, f)
    }

    pub fn request_cancellation(&self) {
        self.root.request_cancellation();
    }

    pub async fn wait_for_idle(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.root.wait_for_idle(timeout).await
    }

    pub fn force_abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.root.force_abort_remaining()
    }

    pub fn abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.force_abort_remaining()
    }

    pub async fn shutdown_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), TaskSupervisionError> {
        self.root.shutdown_with_timeout(timeout).await
    }

    pub async fn shutdown_gracefully(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.shutdown_with_timeout(timeout).await
    }

    pub fn shutdown(&self) {
        self.root.shutdown();
    }

    pub fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        self.root.cancellation_token()
    }

    pub fn active_tasks(&self) -> Vec<String> {
        self.root.active_tasks()
    }
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TaskSupervisor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl TaskGroup {
    fn root(name: impl Into<String>, diagnostics: Option<Arc<RuntimeDiagnosticSink>>) -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            shared: Arc::new(TaskGroupShared {
                name: name.into(),
                next_task_id: AtomicU64::new(1),
                shutdown_tx,
                inherited_cancellation: None,
                diagnostics,
                tasks: Mutex::new(BTreeMap::new()),
                notify: Arc::new(Notify::new()),
            }),
        }
    }

    pub fn name(&self) -> &str {
        &self.shared.name
    }

    pub fn group(&self, name: impl Into<String>) -> TaskGroup {
        let name = name.into();
        let full_name = format!("{}.{}", self.shared.name, name);
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        TaskGroup {
            shared: Arc::new(TaskGroupShared {
                name: full_name,
                next_task_id: AtomicU64::new(1),
                shutdown_tx,
                inherited_cancellation: Some(self.cancellation_token()),
                diagnostics: self.shared.diagnostics.clone(),
                tasks: Mutex::new(BTreeMap::new()),
                notify: Arc::new(Notify::new()),
            }),
        }
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_with_token<F>(
        &self,
        name: impl Into<String>,
        fut: F,
        token: Arc<dyn CancellationToken>,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), Some(token))
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_child<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let interval_ms = interval.as_millis().try_into().unwrap_or(u64::MAX);
        self.spawn_boxed(
            name.into(),
            Box::pin(async move {
                loop {
                    if !f().await {
                        break;
                    }

                    if time_effects.sleep_ms(interval_ms).await.is_err() {
                        break;
                    }
                }
            }),
            None,
        )
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + 'static,
        Fut: Future<Output = bool> + 'static,
    {
        let interval_ms = interval.as_millis().try_into().unwrap_or(u64::MAX);
        self.spawn_boxed_local(
            name.into(),
            Box::pin(async move {
                loop {
                    if !f().await {
                        break;
                    }

                    if time_effects.sleep_ms(interval_ms).await.is_err() {
                        break;
                    }
                }
            }),
            None,
        )
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_periodic<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.spawn_interval_until_named(name, time_effects, interval, f)
    }

    pub fn request_cancellation(&self) {
        let _ = self.shared.shutdown_tx.send(true);
        tracing::debug!(
            event = "runtime.task_group.cancel_requested",
            task_group = %self.shared.name,
            active_tasks = self.active_tasks().len(),
            "Task group cancellation requested"
        );
        self.shared.notify.notify_waiters();
    }

    pub async fn wait_for_idle(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        let group_name = self.shared.name.clone();
        let time = PhysicalTimeHandler::new();
        let started_at = time
            .physical_time()
            .await
            .map_err(|_| TaskSupervisionError::Timeout {
                group: group_name.clone(),
                active_tasks: self.active_tasks(),
            })?;
        let budget = TimeoutBudget::from_start_and_timeout(&started_at, timeout).map_err(|_| {
            TaskSupervisionError::Timeout {
                group: group_name.clone(),
                active_tasks: self.active_tasks(),
            }
        })?;
        let result = execute_with_timeout_budget(&time, &budget, || async {
            loop {
                if self.shared.tasks.lock().is_empty() {
                    return Ok::<(), ()>(());
                }
                self.shared.notify.notified().await;
            }
        })
        .await;

        match result {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Timeout(_)) | Err(TimeoutRunError::Operation(_)) => {
                Err(TaskSupervisionError::Timeout {
                    group: group_name,
                    active_tasks: self.active_tasks(),
                })
            }
        }
    }

    pub fn force_abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        let mut tasks = self.shared.tasks.lock();
        if tasks.is_empty() {
            return Ok(());
        }

        let mut aborted_tasks = Vec::with_capacity(tasks.len());
        #[cfg(not(target_arch = "wasm32"))]
        for (_, entry) in tasks.iter() {
            if let Some(handle) = &entry.handle {
                handle.abort();
            }
            aborted_tasks.push(entry.task_name.clone());
            emit_task_diagnostic(
                self.shared.diagnostics.as_ref(),
                RuntimeDiagnosticSeverity::Warn,
                "task_supervisor",
                format!(
                    "force-aborted supervised task '{}' in group '{}'",
                    entry.task_name, self.shared.name
                ),
            );
            tracing::warn!(
                event = "runtime.task.abort_forced",
                task_group = %self.shared.name,
                task_name = %entry.task_name,
                "Force-aborted supervised task"
            );
        }

        #[cfg(target_arch = "wasm32")]
        for (_, entry) in tasks.iter() {
            aborted_tasks.push(entry.task_name.clone());
        }

        tasks.clear();
        self.shared.notify.notify_waiters();

        Err(TaskSupervisionError::ForcedAbort {
            group: self.shared.name.clone(),
            aborted_tasks,
        })
    }

    pub fn abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.force_abort_remaining()
    }

    pub async fn shutdown_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), TaskSupervisionError> {
        self.request_cancellation();
        match self.wait_for_idle(timeout).await {
            Ok(()) => Ok(()),
            Err(TaskSupervisionError::Timeout { .. }) => self.force_abort_remaining(),
            Err(other) => Err(other),
        }
    }

    pub async fn shutdown_gracefully(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.shutdown_with_timeout(timeout).await
    }

    pub fn shutdown(&self) {
        self.request_cancellation();
        let _ = self.force_abort_remaining();
    }

    pub fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(TaskGroupCancellationToken {
            shutdown_rx: self.shared.shutdown_tx.subscribe(),
            inherited: self.shared.inherited_cancellation.clone(),
        })
    }

    pub fn active_tasks(&self) -> Vec<String> {
        self.shared
            .tasks
            .lock()
            .values()
            .map(|task| task.task_name.clone())
            .collect()
    }

    fn register_task(&self, task_id: u64, task_name: String) {
        self.shared.tasks.lock().insert(
            task_id,
            TaskMetadata {
                task_name,
                #[cfg(not(target_arch = "wasm32"))]
                handle: None,
            },
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn attach_native_handle(&self, task_id: u64, handle: JoinHandle<()>) {
        if let Some(metadata) = self.shared.tasks.lock().get_mut(&task_id) {
            metadata.handle = Some(handle);
        }
    }

    fn complete_task(&self, task_id: u64, task_name: &str, outcome: TaskOutcome) {
        let removed = self.shared.tasks.lock().remove(&task_id);
        if removed.is_none() {
            return;
        }

        if matches!(outcome, TaskOutcome::Cancelled | TaskOutcome::Panicked) {
            tracing::warn!(
                event = "runtime.task.exit_non_success",
                task_group = %self.shared.name,
                task_name = %task_name,
                outcome = ?outcome,
                "Supervised task exited abnormally"
            );
        }

        self.shared.notify.notify_waiters();
    }

    fn spawn_boxed(
        &self,
        task_name: String,
        fut: BoxFuture<'static, ()>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        let task_id = self.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        self.register_task(task_id, task_name.clone());
        let group_name = self.shared.name.clone();
        let mut shutdown_rx = self.shared.shutdown_tx.subscribe();
        let inherited = self.shared.inherited_cancellation.clone();
        let diagnostics = self.shared.diagnostics.clone();
        let task_name_for_wrapper = task_name.clone();
        let group = self.clone();

        tracing::debug!(
            event = "runtime.task.spawned",
            task_group = %group_name,
            task_name = %task_name,
            task_id,
            "Spawned supervised task"
        );

        #[cfg(not(target_arch = "wasm32"))]
        let handle = tokio::spawn(async move {
            let outcome = AssertUnwindSafe(async {
                tokio::select! {
                    _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                    _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                    _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                    _ = fut => TaskOutcome::Completed,
                }
            })
            .catch_unwind()
            .await
            .unwrap_or(TaskOutcome::Panicked);

            emit_task_completion(
                diagnostics.as_ref(),
                &group_name,
                &task_name_for_wrapper,
                task_id,
                &outcome,
            );
            group.complete_task(task_id, &task_name_for_wrapper, outcome);
        });

        #[cfg(not(target_arch = "wasm32"))]
        self.attach_native_handle(task_id, handle);

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                let outcome = AssertUnwindSafe(async {
                    tokio::select! {
                        _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                        _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                        _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                        _ = fut => TaskOutcome::Completed,
                    }
                })
                .catch_unwind()
                .await
                .unwrap_or(TaskOutcome::Panicked);

                emit_task_completion(
                    diagnostics.as_ref(),
                    &group_name,
                    &task_name_for_wrapper,
                    task_id,
                    &outcome,
                );
                group.complete_task(task_id, &task_name_for_wrapper, outcome);
            });
        }

        OwnedTaskHandle::new(
            task_id,
            OwnedShutdownToken::attached(self.cancellation_token()),
        )
    }

    fn spawn_boxed_local(
        &self,
        task_name: String,
        fut: LocalBoxFuture<'static, ()>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        let task_id = self.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        self.register_task(task_id, task_name.clone());
        let mut shutdown_rx = self.shared.shutdown_tx.subscribe();
        let inherited = self.shared.inherited_cancellation.clone();
        let group_name = self.shared.name.clone();
        let diagnostics = self.shared.diagnostics.clone();
        let task_name_for_wrapper = task_name.clone();
        let group = self.clone();

        #[cfg(not(target_arch = "wasm32"))]
        let handle = tokio::task::spawn_local(async move {
            let outcome = AssertUnwindSafe(async {
                tokio::select! {
                    _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                    _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                    _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                    _ = fut => TaskOutcome::Completed,
                }
            })
            .catch_unwind()
            .await
            .unwrap_or(TaskOutcome::Panicked);

            emit_task_completion(
                diagnostics.as_ref(),
                &group_name,
                &task_name_for_wrapper,
                task_id,
                &outcome,
            );
            group.complete_task(task_id, &task_name_for_wrapper, outcome);
        });

        #[cfg(not(target_arch = "wasm32"))]
        self.attach_native_handle(task_id, handle);

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                let outcome = AssertUnwindSafe(async {
                    tokio::select! {
                        _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                        _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                        _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                        _ = fut => TaskOutcome::Completed,
                    }
                })
                .catch_unwind()
                .await
                .unwrap_or(TaskOutcome::Panicked);

                emit_task_completion(
                    diagnostics.as_ref(),
                    &group_name,
                    &task_name_for_wrapper,
                    task_id,
                    &outcome,
                );
                group.complete_task(task_id, &task_name_for_wrapper, outcome);
            });
        }

        OwnedTaskHandle::new(
            task_id,
            OwnedShutdownToken::attached(self.cancellation_token()),
        )
    }
}

struct TaskGroupCancellationToken {
    shutdown_rx: watch::Receiver<bool>,
    inherited: Option<Arc<dyn CancellationToken>>,
}

#[async_trait::async_trait]
impl CancellationToken for TaskGroupCancellationToken {
    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let mut shutdown_rx = self.shutdown_rx.clone();
        match self.inherited.clone() {
            Some(inherited) => {
                tokio::select! {
                    _ = shutdown_cancelled(&mut shutdown_rx) => {}
                    _ = inherited.cancelled() => {}
                }
            }
            None => {
                shutdown_cancelled(&mut shutdown_rx).await;
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        *self.shutdown_rx.borrow()
            || self
                .inherited
                .as_ref()
                .map(|token| token.is_cancelled())
                .unwrap_or(false)
    }
}

impl TaskSpawner for TaskSupervisor {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        let _ = self.spawn_named(DEFAULT_TASK_NAME, fut);
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        let _ = self
            .root
            .spawn_boxed(DEFAULT_TASK_NAME.to_string(), fut, Some(token));
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        let _ = self
            .root
            .spawn_boxed_local(DEFAULT_TASK_NAME.to_string(), fut, None);
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        let _ = self
            .root
            .spawn_boxed_local(DEFAULT_TASK_NAME.to_string(), fut, Some(token));
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        self.cancellation_token()
    }
}

fn emit_task_completion(
    diagnostics: Option<&Arc<RuntimeDiagnosticSink>>,
    group: &str,
    task_name: &str,
    task_id: u64,
    outcome: &TaskOutcome,
) {
    match outcome {
        TaskOutcome::Completed => tracing::debug!(
            event = "runtime.task.completed",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task completed"
        ),
        TaskOutcome::Cancelled => tracing::info!(
            event = "runtime.task.cancelled",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task cancelled"
        ),
        TaskOutcome::Panicked => tracing::error!(
            event = "runtime.task.panicked",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task panicked"
        ),
    }

    if matches!(outcome, TaskOutcome::Panicked) {
        emit_task_diagnostic(
            diagnostics,
            RuntimeDiagnosticSeverity::Error,
            "task_supervisor",
            format!("supervised task '{task_name}' in group '{group}' panicked"),
        );
    }
}

fn emit_task_diagnostic(
    diagnostics: Option<&Arc<RuntimeDiagnosticSink>>,
    severity: RuntimeDiagnosticSeverity,
    component: &'static str,
    message: String,
) {
    if let Some(diagnostics) = diagnostics {
        diagnostics.emit(RuntimeDiagnostic {
            severity,
            kind: RuntimeDiagnosticKind::SupervisedTaskFailed,
            component,
            message,
        });
    }
}

async fn shutdown_cancelled(shutdown_rx: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

async fn inherited_cancelled(token: Option<&Arc<dyn CancellationToken>>) {
    match token {
        Some(token) => token.cancelled().await,
        None => futures::future::pending::<()>().await,
    }
}

async fn external_cancelled(token: Option<&dyn CancellationToken>) {
    match token {
        Some(token) => token.cancelled().await,
        None => futures::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{RuntimeDiagnosticKind, RuntimeDiagnosticSeverity};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn shutdown_with_timeout_cancels_supervised_tasks() {
        let supervisor = TaskSupervisor::new();
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        supervisor
            .shutdown_with_timeout(Duration::from_millis(50))
            .await
            .expect("shutdown should cancel pending task");
        assert!(supervisor.active_tasks().is_empty());
    }

    #[tokio::test]
    async fn child_groups_inherit_parent_cancellation() {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("child");
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = child.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        supervisor.request_cancellation();
        child
            .wait_for_idle(Duration::from_millis(50))
            .await
            .expect("child tasks should stop when parent is cancelled");
    }

    #[tokio::test]
    async fn wait_for_idle_times_out_and_force_abort_reports_tasks() {
        let supervisor = TaskSupervisor::new();
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        let timeout = supervisor.wait_for_idle(Duration::from_millis(10)).await;
        assert!(matches!(timeout, Err(TaskSupervisionError::Timeout { .. })));

        let abort = supervisor.force_abort_remaining();
        assert!(matches!(
            abort,
            Err(TaskSupervisionError::ForcedAbort { .. })
        ));
        assert!(supervisor.active_tasks().is_empty());
    }

    #[tokio::test]
    async fn force_abort_emits_runtime_diagnostic() {
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let supervisor = TaskSupervisor::with_diagnostics(diagnostics.clone());
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        let mut rx = diagnostics.subscribe();
        let abort = supervisor.force_abort_remaining();
        assert!(matches!(
            abort,
            Err(TaskSupervisionError::ForcedAbort { .. })
        ));

        let diagnostic = rx.try_recv().expect("diagnostic emitted");
        assert_eq!(diagnostic.kind, RuntimeDiagnosticKind::SupervisedTaskFailed);
        assert_eq!(diagnostic.severity, RuntimeDiagnosticSeverity::Warn);
    }

    #[test]
    fn loom_shutdown_race_does_not_leave_task_registered() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::{Arc as LoomArc, Mutex as LoomMutex};
            use loom::thread;

            let active = LoomArc::new(LoomMutex::new(Vec::<u8>::new()));
            let cancelled = LoomArc::new(AtomicBool::new(false));

            let register_active = LoomArc::clone(&active);
            let register_cancelled = LoomArc::clone(&cancelled);
            let register = thread::spawn(move || {
                {
                    let mut tasks = register_active.lock().unwrap();
                    tasks.push(1);
                }
                if register_cancelled.load(Ordering::Acquire) {
                    let mut tasks = register_active.lock().unwrap();
                    tasks.retain(|task| *task != 1);
                }
            });

            let shutdown_active = LoomArc::clone(&active);
            let shutdown_cancelled = LoomArc::clone(&cancelled);
            let shutdown = thread::spawn(move || {
                shutdown_cancelled.store(true, Ordering::Release);
                let mut tasks = shutdown_active.lock().unwrap();
                tasks.retain(|task| *task != 1);
            });

            register.join().expect("register thread");
            shutdown.join().expect("shutdown thread");
            assert!(
                active.lock().unwrap().is_empty(),
                "task bookkeeping should not leak active entries across shutdown races"
            );
        });
    }

    #[test]
    fn loom_shutdown_token_propagation_reaches_child() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::Arc as LoomArc;
            use loom::thread;

            let cancelled = LoomArc::new(AtomicBool::new(false));
            let child_observed = LoomArc::new(AtomicBool::new(false));

            let child = {
                let cancelled = cancelled.clone();
                let child_observed = child_observed.clone();
                thread::spawn(move || {
                    while !cancelled.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    child_observed.store(true, Ordering::Release);
                })
            };

            let parent = {
                let cancelled = cancelled.clone();
                thread::spawn(move || {
                    cancelled.store(true, Ordering::Release);
                })
            };

            parent.join().expect("parent joins");
            child.join().expect("child joins");

            assert!(
                child_observed.load(Ordering::Acquire),
                "child cancellation observer must see parent-driven shutdown"
            );
        });
    }
}
