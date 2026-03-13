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
  data_dir?: string;
  artifact_dir?: string;
  headless?: boolean;
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
  lastObservationResetReason?: string;
  observationEpoch?: number;
  clipboardCache?: string;
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
