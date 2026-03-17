import type { DriverMethod } from './contracts.js';

export const OBSERVATION_METHODS: ReadonlySet<DriverMethod> = new Set([
  'snapshot',
  'ui_state',
  'wait_for_ui_state',
  'dom_snapshot',
  'wait_for_dom_patterns',
  'wait_for_selector',
  'read_clipboard',
  'get_authority_id',
  'tail_log'
]);

export const ACTION_METHODS: ReadonlySet<DriverMethod> = new Set([
  'send_keys',
  'send_key',
  'navigate_screen',
  'click_button',
  'fill_input',
  'submit_semantic_command',
  'inject_message'
]);

export const RECOVERY_METHODS: ReadonlySet<DriverMethod> = new Set([
  'recover_ui_state',
  'restart_page_session'
]);
