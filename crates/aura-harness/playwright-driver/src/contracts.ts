export const HARNESS_API_KEY = "__AURA_HARNESS__";
export const HARNESS_OBSERVE_KEY = "__AURA_HARNESS_OBSERVE__";
export const UI_PUBLICATION_STATE_KEY = "__AURA_UI_PUBLICATION_STATE__";
export const RENDER_HEARTBEAT_PUBLICATION_STATE_KEY =
  "__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__";
export const RENDER_HEARTBEAT_KEY = "__AURA_RENDER_HEARTBEAT__";
export const RENDER_HEARTBEAT_JSON_KEY = "__AURA_RENDER_HEARTBEAT_JSON__";
export const SEMANTIC_SUBMIT_PUBLICATION_STATE_KEY =
  "__AURA_SEMANTIC_SUBMIT_PUBLICATION_STATE__";
export const UI_ACTIVE_GENERATION_KEY = "__AURA_UI_ACTIVE_GENERATION__";
export const UI_READY_GENERATION_KEY = "__AURA_UI_READY_GENERATION__";
export const UI_GENERATION_PHASE_KEY = "__AURA_UI_GENERATION_PHASE__";
export const UI_STATE_JSON_KEY = "__AURA_UI_STATE_JSON__";
export const UI_STATE_CACHE_KEY = "__AURA_UI_STATE_CACHE__";
export const UI_STATE_OBSERVE_KEY = "__AURA_UI_STATE__";
export const DRIVER_PUSH_STATE_KEY = "__AURA_DRIVER_PUSH_STATE";
export const DRIVER_OBSERVER_INSTALLED_KEY = "__AURA_DRIVER_OBSERVER_INSTALLED";
export const DRIVER_PUSH_UI_STATE_KEY = "__AURA_DRIVER_PUSH_UI_STATE";
export const DRIVER_PUSH_RENDER_HEARTBEAT_KEY =
  "__AURA_DRIVER_PUSH_RENDER_HEARTBEAT";
export const DRIVER_PUSH_CLIPBOARD_KEY = "__AURA_DRIVER_PUSH_CLIPBOARD";
export const PENDING_SEMANTIC_QUEUE_SEED_KEY =
  "__AURA_DRIVER_PENDING_SEMANTIC_QUEUE_SEED__";
export const PENDING_RUNTIME_STAGE_QUEUE_SEED_KEY =
  "__AURA_DRIVER_PENDING_RUNTIME_STAGE_QUEUE_SEED__";
export const WAKE_SEMANTIC_QUEUE_KEY = "__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__";
export const WAKE_RUNTIME_STAGE_QUEUE_KEY =
  "__AURA_DRIVER_WAKE_RUNTIME_STAGE_QUEUE__";

export interface ProjectionRevision {
  semantic_seq?: number;
  render_seq?: number;
}

export interface UiSnapshotPayload {
  screen?: string;
  open_modal?: string | null;
  readiness?: string;
  revision?: ProjectionRevision;
  quiescence?: Record<string, unknown>;
  lists?: unknown[];
  selections?: unknown[];
  messages?: unknown[];
  operations?: unknown[];
  toasts?: unknown[];
  runtime_events?: unknown[];
}

export interface SnapshotResult {
  screen: string;
  raw_screen: string;
  authoritative_screen: string;
  normalized_screen: string;
  capture_consistency: string;
}

export interface StructuredActionResult {
  status: string;
  operation_id?: string;
  operation_state?: string;
  code?: string;
  authority_id?: string;
  [key: string]: unknown;
}

export interface RecoveryResult {
  status: string;
  reason?: string;
  restarted?: boolean;
  [key: string]: unknown;
}

export interface ReadClipboardResult {
  text: string;
}

export interface TailLogResult {
  lines: string[];
}

export interface SemanticSubmitPublicationState {
  surface?: string;
  status?: string;
  detail?: string;
  binding_mode?: string;
  generation_id?: number | null;
  active_generation?: number | null;
  ready_generation?: number | null;
  generation_ready?: boolean;
  phase?: string | null;
  controller_present?: boolean;
  bootstrap_transition_detail?: string | null;
  enqueue_ready?: boolean;
}

export interface DriverRequest {
  id: number | null;
  method: DriverMethod;
  params?: DriverParams;
}

export interface DriverSuccessResponse {
  id: number | null;
  ok: true;
  result: unknown;
}

export interface DriverErrorResponse {
  id: number | null;
  ok: false;
  error: string;
}

export type DriverResponse = DriverSuccessResponse | DriverErrorResponse;

export interface StartPageParams extends Record<string, unknown> {
  instance_id: string;
  app_url?: string;
  harness_run_token?: string;
  data_dir?: string;
  artifact_dir?: string;
  headless?: boolean;
  startup_readiness?: string;
  require_semantic_ready?: boolean;
  pending_semantic_payload?: string;
  pending_runtime_stage_payload?: string;
}

export interface InstanceParams extends Record<string, unknown> {
  instance_id: string;
}

export type DriverParams =
  | StartPageParams
  | InstanceParams
  | (Record<string, unknown> & { instance_id: string });

export type DriverMethod =
  | 'start_page'
  | 'send_keys'
  | 'send_key'
  | 'navigate_screen'
  | 'open_settings_section'
  | 'click_button'
  | 'fill_input'
  | 'snapshot'
  | 'ui_state'
  | 'wait_for_ui_state'
  | 'dom_snapshot'
  | 'wait_for_dom_patterns'
  | 'wait_for_selector'
  | 'read_clipboard'
  | 'submit_semantic_command'
  | 'get_authority_id'
  | 'reload_page'
  | 'recover_ui_state'
  | 'stage_runtime_identity'
  | 'restart_page_session'
  | 'tail_log'
  | 'inject_message'
  | 'stop';

export interface UiStateWaiter {
  afterVersion: number;
  resolve: (value: { snapshot: UiSnapshotPayload; version: number }) => void;
  reject: (reason: Error) => void;
  timer: NodeJS.Timeout | null;
}

export interface DriverSession {
  context: any;
  page: any;
  dataDir: string;
  artifactDir: string;
  uiStateCache: UiSnapshotPayload | null;
  uiStateCacheJson: string | null;
  uiStateVersion: number;
  uiStateWaiters: UiStateWaiter[];
  domState: { text: string; ids: Set<string> | string[] };
  renderHeartbeat: Record<string, unknown> | null;
  requiredUiStateRevision: number | null;
  requiredUiGeneration?: number | null;
  currentUiGeneration?: number | null;
  semanticSubmitState?: SemanticSubmitPublicationState | null;
  semanticQueueInstalled?: boolean;
  lastMainFrameNavigationAt?: number;
  lastObservationResetReason?: string;
  observationEpoch?: number;
  clipboardCache?: string;
  pendingSemanticPayload?: string | null;
  semanticResultCache?: Record<string, unknown>;
  lastMutationReason?: string | null;
  tracePath?: string | null;
  lastUiStateSource?: string;
  logPath?: string | null;
  [key: string]: unknown;
}

declare global {
  interface Window {
    __AURA_HARNESS__?: Record<string, (...args: any[]) => any>;
    __AURA_HARNESS_OBSERVE__?: Record<string, (...args: any[]) => any>;
  }
}

export {};
