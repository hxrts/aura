use serde::{Deserialize, Serialize};

pub(crate) const MUTATION_QUEUE_INSTALLED_KEY: &str = "__AURA_DRIVER_MUTATION_QUEUE_INSTALLED";
pub(crate) const PENDING_NAV_SCREEN_KEY: &str = "__AURA_DRIVER_PENDING_NAV_SCREEN__";
pub(crate) const PENDING_SEMANTIC_QUEUE_SEED_KEY: &str =
    "__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__";
pub(crate) const PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY: &str =
    "__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__";
pub(crate) const SEMANTIC_RESULTS_KEY: &str = "__AURA_DRIVER_SEMANTIC_RESULTS__";
pub(crate) const RUNTIME_STAGE_RESULTS_KEY: &str = "__AURA_DRIVER_RUNTIME_STAGE_RESULTS__";
pub(crate) const SEMANTIC_DEBUG_KEY: &str = "__AURA_DRIVER_SEMANTIC_DEBUG__";
pub(crate) const RUNTIME_STAGE_DEBUG_KEY: &str = "__AURA_DRIVER_RUNTIME_STAGE_DEBUG__";
pub(crate) const SEMANTIC_BUSY_KEY: &str = "__AURA_DRIVER_SEMANTIC_BUSY__";
pub(crate) const RUNTIME_STAGE_BUSY_KEY: &str = "__AURA_DRIVER_RUNTIME_STAGE_BUSY__";
pub(crate) const SEMANTIC_WAKE_SCHEDULED_KEY: &str = "__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__";
pub(crate) const RUNTIME_STAGE_WAKE_SCHEDULED_KEY: &str =
    "__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__";
pub(crate) const SEMANTIC_QUEUE_KEY: &str = "__AURA_DRIVER_SEMANTIC_QUEUE__";
pub(crate) const RUNTIME_STAGE_QUEUE_KEY: &str = "__AURA_DRIVER_RUNTIME_STAGE_QUEUE__";
pub(crate) const SEMANTIC_ENQUEUE_KEY: &str = "__AURA_DRIVER_SEMANTIC_ENQUEUE__";
pub(crate) const RUNTIME_STAGE_ENQUEUE_KEY: &str = "__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE__";
pub(crate) const WAKE_SEMANTIC_QUEUE_KEY: &str = "__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__";
pub(crate) const WAKE_RUNTIME_STAGE_QUEUE_KEY: &str = "__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__";
pub(crate) const WAKE_PENDING_NAV_KEY: &str = "__AURA_DRIVER_WAKE_PENDING_NAV__";
pub(crate) const PUSH_SEMANTIC_RESULT_KEY: &str = "__AURA_DRIVER_PUSH_SEMANTIC_RESULT";
pub(crate) const PUSH_RUNTIME_STAGE_RESULT_KEY: &str = "__AURA_DRIVER_PUSH_RUNTIME_STAGE_RESULT";
pub(crate) const PUSH_SEMANTIC_SUBMIT_STATE_KEY: &str = "__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SemanticQueuePayload {
    pub(crate) command_id: String,
    pub(crate) request_json: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RuntimeStageQueuePayload {
    pub(crate) command_id: String,
    pub(crate) runtime_identity_json: String,
}
