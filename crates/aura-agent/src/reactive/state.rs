//! Internal reactive state containers.

use crate::task_registry::TaskRegistry;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Internal state for Dynamic<T>.
pub(crate) struct DynamicState<T: Clone + Send + Sync + 'static> {
    pub(crate) value: Arc<T>,
    pub(crate) updates: broadcast::Sender<Arc<T>>,
    pub(crate) tasks: Arc<TaskRegistry>,
}

/// Statistics collected by the scheduler.
#[derive(Debug, Clone, Default)]
pub(crate) struct SchedulerStats {
    pub(crate) batch_count: u64,
    pub(crate) facts_processed: u64,
    pub(crate) total_batch_latency_ms: f64,
}
