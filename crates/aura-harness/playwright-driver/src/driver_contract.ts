export const MUTATION_QUEUE_INSTALLED_KEY = "__AURA_DRIVER_MUTATION_QUEUE_INSTALLED";
export const PENDING_NAV_SCREEN_KEY = "__AURA_DRIVER_PENDING_NAV_SCREEN__";
export const PENDING_SEMANTIC_QUEUE_SEED_KEY = "__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__";
export const PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY = "__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__";
export const SEMANTIC_RESULTS_KEY = "__AURA_DRIVER_SEMANTIC_RESULTS__";
export const RUNTIME_STAGE_RESULTS_KEY = "__AURA_DRIVER_RUNTIME_STAGE_RESULTS__";
export const SEMANTIC_DEBUG_KEY = "__AURA_DRIVER_SEMANTIC_DEBUG__";
export const RUNTIME_STAGE_DEBUG_KEY = "__AURA_DRIVER_RUNTIME_STAGE_DEBUG__";
export const SEMANTIC_BUSY_KEY = "__AURA_DRIVER_SEMANTIC_BUSY__";
export const RUNTIME_STAGE_BUSY_KEY = "__AURA_DRIVER_RUNTIME_STAGE_BUSY__";
export const SEMANTIC_WAKE_SCHEDULED_KEY = "__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__";
export const RUNTIME_STAGE_WAKE_SCHEDULED_KEY = "__AURA_DRIVER_RUNTIME_STAGE_WAKE_SCHEDULED__";
export const SEMANTIC_QUEUE_KEY = "__AURA_DRIVER_SEMANTIC_QUEUE__";
export const RUNTIME_STAGE_QUEUE_KEY = "__AURA_DRIVER_RUNTIME_STAGE_QUEUE__";
export const SEMANTIC_ENQUEUE_KEY = "__AURA_DRIVER_SEMANTIC_ENQUEUE__";
export const RUNTIME_STAGE_ENQUEUE_KEY = "__AURA_DRIVER_RUNTIME_STAGE_ENQUEUE__";
export const WAKE_SEMANTIC_QUEUE_KEY = "__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__";
export const WAKE_RUNTIME_STAGE_QUEUE_KEY = "__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__";
export const WAKE_PENDING_NAV_KEY = "__AURA_DRIVER_WAKE_PENDING_NAV__";
export const PUSH_SEMANTIC_SUBMIT_STATE_KEY = "__AURA_DRIVER_PUSH_SEMANTIC_SUBMIT_STATE";
export const PUSH_SEMANTIC_RESULT_KEY = "__AURA_DRIVER_PUSH_SEMANTIC_RESULT";
export const PUSH_RUNTIME_STAGE_RESULT_KEY = "__AURA_DRIVER_PUSH_RUNTIME_STAGE_RESULT";

export type SemanticQueuePayload = {
  command_id: string;
  request_json: string;
};

export type RuntimeStageQueuePayload = {
  command_id: string;
  runtime_identity_json: string;
};

export function buildSemanticQueuePayloadJson(
  commandId: string,
  requestJson: string,
): string {
  const payload: SemanticQueuePayload = {
    command_id: commandId,
    request_json: requestJson,
  };
  return JSON.stringify(payload);
}

export function buildRuntimeStageQueuePayloadJson(
  commandId: string,
  runtimeIdentityJson: string,
): string {
  const payload: RuntimeStageQueuePayload = {
    command_id: commandId,
    runtime_identity_json: runtimeIdentityJson,
  };
  return JSON.stringify(payload);
}

export function seedQueuePayloadArray(payloadJson: string | null): string[] {
  return typeof payloadJson === "string" && payloadJson.length > 0
    ? [payloadJson]
    : [];
}
