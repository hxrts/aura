use async_lock::Mutex;
use std::sync::Arc;

#[allow(clippy::disallowed_types)]
pub(crate) type WorkflowStageTracker = Arc<Mutex<&'static str>>;

#[allow(clippy::disallowed_types)]
pub(crate) fn new_workflow_stage_tracker(stage: &'static str) -> WorkflowStageTracker {
    Arc::new(Mutex::new(stage))
}

pub(crate) fn update_workflow_stage(
    tracker: &Option<WorkflowStageTracker>,
    stage: &'static str,
) {
    if let Some(tracker) = tracker {
        update_workflow_stage_direct(tracker, stage);
    }
}

pub(crate) fn update_workflow_stage_direct(
    tracker: &WorkflowStageTracker,
    stage: &'static str,
) {
    if let Some(mut guard) = tracker.try_lock() {
        *guard = stage;
    }
}
