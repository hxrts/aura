#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { chromium } from 'playwright';

const sessions = new Map();
let requestChain = Promise.resolve();
const UI_STATE_LOG_PREFIX = '[aura-ui-state]';
const UI_STATE_JSON_LOG_PREFIX = '[aura-ui-json]';
const PLAYWRIGHT_TRACE_ENABLED =
  process.env.AURA_HARNESS_PLAYWRIGHT_TRACE === '1' ||
  process.env.AURA_HARNESS_PLAYWRIGHT_TRACE === 'true';
const DEFAULT_PAGE_GOTO_TIMEOUT_MS = 90000;
const DEFAULT_HARNESS_READY_TIMEOUT_MS = 90000;
const DEFAULT_START_MAX_ATTEMPTS = 3;
const DEFAULT_START_RETRY_BACKOFF_MS = 1200;
const MAX_TIMEOUT_MS = 600000;
const MAX_START_ATTEMPTS = 10;

process.on('uncaughtException', (error) => {
  console.error(`[driver] uncaughtException: ${error?.stack ?? error?.message ?? String(error)}`);
  process.exitCode = 1;
});

process.on('unhandledRejection', (reason) => {
  console.error(`[driver] unhandledRejection: ${reason?.stack ?? reason?.message ?? String(reason)}`);
  process.exitCode = 1;
});

function nowIso() {
  return new Date().toISOString();
}

function ensureDir(dirPath) {
  if (!dirPath) {
    return;
  }
  fs.mkdirSync(dirPath, { recursive: true });
}

function jsonResponse(id, ok, payload) {
  if (ok) {
    return { id, ok: true, result: payload };
  }
  return { id, ok: false, error: String(payload) };
}

function writeResponse(response) {
  process.stdout.write(`${JSON.stringify(response)}\n`);
}

function removeUiStateWaiter(session, waiter) {
  if (!Array.isArray(session.uiStateWaiters)) {
    return;
  }
  const index = session.uiStateWaiters.indexOf(waiter);
  if (index >= 0) {
    session.uiStateWaiters.splice(index, 1);
  }
}

function trySerializeUiState(value) {
  if (!value || typeof value !== 'object') {
    return null;
  }
  try {
    return JSON.stringify(value);
  } catch {
    return null;
  }
}

function notifyUiStateWaiters(session) {
  if (!session.uiStateCache || !Array.isArray(session.uiStateWaiters) || session.uiStateWaiters.length === 0) {
    return;
  }
  const ready = session.uiStateWaiters.filter(
    (waiter) => session.uiStateVersion > waiter.afterVersion
  );
  for (const waiter of ready) {
    removeUiStateWaiter(session, waiter);
    clearTimeout(waiter.timer);
    waiter.resolve({
      snapshot: session.uiStateCache,
      version: session.uiStateVersion
    });
  }
}

function storeUiState(session, payload) {
  const parsed =
    typeof payload === 'string'
      ? (() => {
          try {
            return JSON.parse(payload);
          } catch {
            return null;
          }
        })()
      : payload && typeof payload === 'object'
        ? payload
        : null;
  if (!parsed || typeof parsed !== 'object') {
    return false;
  }

  const nextJson = trySerializeUiState(parsed);
  const changed = nextJson !== session.uiStateCacheJson;
  session.uiStateCache = parsed;
  session.uiStateCacheJson = nextJson;
  if (changed) {
    session.uiStateVersion = (session.uiStateVersion ?? 0) + 1;
    notifyUiStateWaiters(session);
  }
  return true;
}

function waitForUiStateVersion(session, afterVersion, timeoutMs) {
  if (
    session.uiStateCache &&
    typeof session.uiStateCache === 'object' &&
    session.uiStateVersion > afterVersion
  ) {
    return Promise.resolve({
      snapshot: session.uiStateCache,
      version: session.uiStateVersion
    });
  }

  return new Promise((resolve, reject) => {
    const waiter = {
      afterVersion,
      resolve,
      reject,
      timer: null
    };
    waiter.timer = setTimeout(() => {
      removeUiStateWaiter(session, waiter);
      reject(
        new Error(
          `wait_for_ui_state timed out after ${timeoutMs}ms after_version=${afterVersion} current_version=${
            session.uiStateVersion ?? 0
          }`
        )
      );
    }, timeoutMs);
    session.uiStateWaiters.push(waiter);
  });
}

function normalizeInstanceId(params) {
  const instanceId = params?.instance_id;
  if (!instanceId || typeof instanceId !== 'string') {
    throw new Error('instance_id is required');
  }
  return instanceId;
}

function parseSnapshotPayload(payload) {
  const fallback = String(payload ?? '');
  if (payload && typeof payload === 'object') {
    return {
      screen: String(payload.screen ?? payload.authoritative_screen ?? fallback),
      raw_screen: String(payload.raw_screen ?? payload.screen ?? fallback),
      authoritative_screen: String(payload.authoritative_screen ?? payload.screen ?? fallback),
      normalized_screen: String(payload.normalized_screen ?? payload.screen ?? fallback),
      capture_consistency: String(payload.capture_consistency ?? 'settled')
    };
  }

  return {
    screen: fallback,
    raw_screen: fallback,
    authoritative_screen: fallback,
    normalized_screen: fallback,
    capture_consistency: 'settled'
  };
}

function normalizeScreenText(value) {
  return String(value ?? '')
    .split('\n')
    .map((line) => line.replace(/\s+/g, ' ').trim())
    .filter((line) => line.length > 0)
    .join('\n')
    .trim();
}

function normalizeDomState(payload) {
  const ids = Array.isArray(payload?.ids)
    ? payload.ids
        .map((value) => String(value ?? '').trim())
        .filter((value) => value.length > 0)
    : [];
  return {
    text: normalizeScreenText(payload?.text ?? ''),
    ids: new Set(ids)
  };
}

function domStateIdSet(session) {
  const ids = session?.domState?.ids;
  if (ids instanceof Set) {
    return ids;
  }
  if (Array.isArray(ids)) {
    return new Set(
      ids
        .map((value) => String(value ?? '').trim())
        .filter((value) => value.length > 0)
    );
  }
  return new Set();
}

function domStateHasId(session, id) {
  return domStateIdSet(session).has(String(id ?? '').trim());
}

function domStateIdList(session) {
  return Array.from(domStateIdSet(session));
}

function normalizeRenderHeartbeat(payload) {
  if (!payload || typeof payload !== 'object') {
    return null;
  }
  const screen = contractEnumKey(payload.screen);
  const openModal = contractEnumKey(payload.open_modal);
  const renderSeq = Number(payload.render_seq ?? 0);
  if (!screen || !Number.isFinite(renderSeq)) {
    return null;
  }
  return {
    screen,
    open_modal: openModal,
    render_seq: renderSeq
  };
}

const SCREEN_DOM_IDS = Object.freeze({
  onboarding: 'aura-onboarding-root',
  neighborhood: 'aura-screen-neighborhood',
  chat: 'aura-screen-chat',
  contacts: 'aura-screen-contacts',
  notifications: 'aura-screen-notifications',
  settings: 'aura-screen-settings'
});

const MODAL_DOM_IDS = Object.freeze({
  help: 'aura-modal-help',
  create_invitation: 'aura-modal-create-invitation',
  invitation_code: 'aura-modal-invitation-code',
  accept_invitation: 'aura-modal-accept-invitation',
  create_home: 'aura-modal-create-home',
  create_channel: 'aura-modal-create-channel',
  set_channel_topic: 'aura-modal-set-channel-topic',
  channel_info: 'aura-modal-channel-info',
  edit_nickname: 'aura-modal-edit-nickname',
  remove_contact: 'aura-modal-remove-contact',
  guardian_setup: 'aura-modal-guardian-setup',
  request_recovery: 'aura-modal-request-recovery',
  add_device: 'aura-modal-add-device',
  import_device_enrollment_code: 'aura-modal-import-device-enrollment-code',
  select_device_to_remove: 'aura-modal-select-device-to-remove',
  confirm_remove_device: 'aura-modal-confirm-remove-device',
  mfa_setup: 'aura-modal-mfa-setup',
  assign_moderator: 'aura-modal-assign-moderator',
  switch_authority: 'aura-modal-switch-authority',
  access_override: 'aura-modal-access-override',
  capability_config: 'aura-modal-capability-config'
});

function contractEnumKey(value) {
  if (typeof value !== 'string') {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed.toLowerCase() : null;
}

function expectedScreenDomId(state) {
  return SCREEN_DOM_IDS[contractEnumKey(state?.screen)] ?? null;
}

function expectedModalDomId(state) {
  return MODAL_DOM_IDS[contractEnumKey(state?.open_modal)] ?? null;
}

function onboardingShellPresent(session) {
  return domStateHasId(session, 'aura-onboarding-root');
}

async function ensureUiStateRenderConvergence(session, state, reason, timeoutMs = 1500) {
  if (onboardingShellPresent(session)) {
    return;
  }
  const heartbeat = session.renderHeartbeat;
  if (
    heartbeat &&
    contractEnumKey(state?.screen) === heartbeat.screen &&
    contractEnumKey(state?.open_modal) === heartbeat.open_modal
  ) {
    return;
  }

  const screenDomId = expectedScreenDomId(state);
  if (screenDomId) {
    if (domStateHasId(session, screenDomId)) {
      return;
    }
    try {
      await withOperationTimeout(
        `ui_state_converge_screen_${reason}`,
        session.page.locator(`#${screenDomId}`).first().waitFor({ state: 'attached' }),
        timeoutMs
      );
    } catch (error) {
      throw new Error(
        `semantic screen '${state?.screen ?? 'unknown'}' did not converge to DOM id #${screenDomId}: ${
          error?.message ?? String(error)
        } current_ids=${JSON.stringify(domStateIdList(session))} text_snippet=${JSON.stringify(
          session?.domState?.text ?? ''
        )}`
      );
    }
  }

  const modalDomId = expectedModalDomId(state);
  if (modalDomId) {
    if (domStateHasId(session, modalDomId)) {
      return;
    }
    try {
      await withOperationTimeout(
        `ui_state_converge_modal_${reason}`,
        session.page.locator(`#${modalDomId}`).first().waitFor({ state: 'attached' }),
        timeoutMs
      );
    } catch (error) {
      throw new Error(
        `semantic modal '${state?.open_modal ?? 'unknown'}' did not converge to DOM id #${modalDomId}: ${
          error?.message ?? String(error)
        } current_ids=${JSON.stringify(domStateIdList(session))} text_snippet=${JSON.stringify(
          session?.domState?.text ?? ''
        )}`
      );
    }
  }
}

function cachedUiStateConverged(session, state) {
  if (onboardingShellPresent(session)) {
    return true;
  }
  const heartbeat = session.renderHeartbeat;
  if (
    heartbeat &&
    contractEnumKey(state?.screen) === heartbeat.screen &&
    contractEnumKey(state?.open_modal) === heartbeat.open_modal
  ) {
    return true;
  }
  const screenDomId = expectedScreenDomId(state);
  if (screenDomId && !domStateHasId(session, screenDomId)) {
    return false;
  }
  const modalDomId = expectedModalDomId(state);
  if (modalDomId && !domStateHasId(session, modalDomId)) {
    return false;
  }
  return true;
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function runSelfTest() {
  const chatState = { screen: 'chat', open_modal: null };
  const modalState = { screen: 'neighborhood', open_modal: 'accept_invitation' };
  const convergedChatSession = {
    domState: normalizeDomState({ text: '', ids: ['aura-screen-chat'] })
  };
  const divergedChatSession = {
    domState: normalizeDomState({ text: '', ids: ['aura-screen-neighborhood'] })
  };
  const convergedModalSession = {
    domState: normalizeDomState({
      text: '',
      ids: ['aura-screen-neighborhood', 'aura-modal-accept-invitation']
    })
  };
  const divergedModalSession = {
    domState: normalizeDomState({ text: '', ids: ['aura-screen-neighborhood'] })
  };
  const heartbeatSession = {
    domState: normalizeDomState({ text: '', ids: [] }),
    renderHeartbeat: normalizeRenderHeartbeat({
      screen: 'chat',
      open_modal: null,
      render_seq: 4
    })
  };

  assert(expectedScreenDomId(chatState) === 'aura-screen-chat', 'chat screen id mapping failed');
  assert(
    expectedModalDomId(modalState) === 'aura-modal-accept-invitation',
    'accept invitation modal id mapping failed'
  );
  assert(
    cachedUiStateConverged(convergedChatSession, chatState),
    'converged chat state should be accepted'
  );
  assert(
    !cachedUiStateConverged(divergedChatSession, chatState),
    'diverged chat state should be rejected'
  );
  assert(
    cachedUiStateConverged(convergedModalSession, modalState),
    'converged modal state should be accepted'
  );
  assert(
    !cachedUiStateConverged(divergedModalSession, modalState),
    'diverged modal state should be rejected'
  );
  assert(
    cachedUiStateConverged(heartbeatSession, chatState),
    'heartbeat-aligned state should be accepted without DOM ids'
  );
  console.error('[driver] selftest ok');
}

function consoleTailText(session, lines = 40) {
  const tail = session.consoleLog.slice(-lines);
  return tail.length > 0 ? tail.join('\n') : 'none';
}

async function ensureHarnessWithTimeout(page, timeoutMs) {
  await page.waitForFunction(() => {
    const bridge = window.__AURA_HARNESS__;
    return bridge && typeof bridge.snapshot === 'function';
  }, null, { timeout: timeoutMs });
}

async function ensurePageInteractive(page, timeoutMs) {
  await page.waitForFunction(() => {
    const title = document.title || '';
    const bodyText = document.body?.innerText || '';
    const buildScreenVisible =
      title.includes('Dioxus Build') ||
      bodyText.includes("We're building your app now") ||
      bodyText.includes('Starting the build...');
    const mainRoot = document.getElementById('main');
    return !buildScreenVisible && !!mainRoot;
  }, null, { timeout: timeoutMs });
}

async function installDomObserver(page, session) {
  await page.exposeBinding('__AURA_DRIVER_PUSH_STATE', (_source, payload) => {
    session.domState = normalizeDomState(payload);
  });
  await page.evaluate(() => {
    const pushState = () => {
      const root =
        document.getElementById('aura-app-root') ??
        document.querySelector('main:last-of-type') ??
        document.body;
      const ids = Array.from(document.querySelectorAll('[id]'))
        .map((element) => element.id)
        .filter((id) => id.startsWith('aura-'));
      return window.__AURA_DRIVER_PUSH_STATE({
        text: root?.textContent ?? '',
        ids
      });
    };

    if (window.__AURA_DRIVER_OBSERVER_INSTALLED) {
      pushState();
      return;
    }

    let scheduled = false;
    const schedulePush = () => {
      if (scheduled) {
        return;
      }
      scheduled = true;
      requestAnimationFrame(() => {
        scheduled = false;
        pushState().catch(() => {});
      });
    };

    const observer = new MutationObserver(() => {
      schedulePush();
    });
    observer.observe(document.body, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
      attributeFilter: ['id', 'class', 'aria-hidden', 'open', 'data-state']
    });

    window.addEventListener('load', schedulePush, { once: true });
    window.__AURA_DRIVER_OBSERVER_INSTALLED = true;
    schedulePush();
  });
}

async function installUiStateObserver(page, session) {
  await page.exposeFunction('__AURA_DRIVER_PUSH_UI_STATE', (payload) => {
    storeUiState(session, payload);
  });
  await page.exposeFunction('__AURA_DRIVER_PUSH_RENDER_HEARTBEAT', (payload) => {
    if (typeof payload === 'string') {
      try {
        session.renderHeartbeat = normalizeRenderHeartbeat(JSON.parse(payload));
      } catch {
        session.renderHeartbeat = null;
      }
      return;
    }
    session.renderHeartbeat = normalizeRenderHeartbeat(payload);
  });
}

function installPageNavigationReset(session) {
  const clearSessionCache = () => {
    session.uiStateCache = null;
    session.uiStateCacheJson = null;
    session.renderHeartbeat = null;
  };
  const onNavigation = () => {
    clearSessionCache();
    console.error(`[driver] navigation_cache_clear instance=${session.id}`);
  };
  session.page.on('framenavigated', onNavigation);
  session.page.on('domcontentloaded', onNavigation);
  session.page.on('load', onNavigation);
}

async function assertRootStructure(session, reason) {
  let structure = await withOperationTimeout(
    `root_structure_${reason}`,
    session.page.evaluate(() => {
      if (typeof window.__AURA_HARNESS__?.root_structure === 'function') {
        return window.__AURA_HARNESS__.root_structure();
      }
      return null;
    }),
    2000
  );

  if (!structure || typeof structure !== 'object') {
    const expectedScreen =
      contractEnumKey(session.renderHeartbeat?.screen) ??
      contractEnumKey(session.uiStateCache?.screen);
    const expectedScreenDomId = expectedScreen ? SCREEN_DOM_IDS[expectedScreen] ?? null : null;
    structure = await withOperationTimeout(
      `root_structure_dom_fallback_${reason}`,
      session.page.evaluate((screenDomId) => {
        const count = (selector) =>
          document.querySelectorAll(selector).length;
        return {
          app_root_count: count('#aura-app-root'),
          modal_region_count: count('#aura-modal-region'),
          onboarding_root_count: count('#aura-onboarding-root'),
          toast_region_count: count('#aura-toast-region'),
          active_screen_root_count: screenDomId ? count(`#${screenDomId}`) : 0
        };
      }, expectedScreenDomId),
      2000
    );
  }

  if (!structure || typeof structure !== 'object') {
    throw new Error(`root structure export unavailable during ${reason}`);
  }

  const appRootCount = Number(structure.app_root_count ?? 0);
  const onboardingRootCount = Number(structure.onboarding_root_count ?? 0);
  const modalRegionCount = Number(structure.modal_region_count ?? 0);
  const toastRegionCount = Number(structure.toast_region_count ?? 0);
  const activeScreenRootCount = Number(structure.active_screen_root_count ?? 0);
  const onboardingShellValid =
    onboardingRootCount === 1 &&
    appRootCount === 0 &&
    modalRegionCount === 0 &&
    toastRegionCount === 0 &&
    activeScreenRootCount === 0;
  if (onboardingShellValid) {
    return;
  }
  if (
    appRootCount !== 1 ||
    modalRegionCount !== 1 ||
    toastRegionCount !== 1 ||
    activeScreenRootCount !== 1
  ) {
    throw new Error(
      `invalid root structure during ${reason}: ${JSON.stringify(structure)}`
    );
  }
}

function isNavigationTransitionError(error) {
  const message = String(error?.message ?? error ?? '');
  return (
    message.includes('Execution context was destroyed') ||
    message.includes('most likely because of a navigation') ||
    message.includes('Target page, context or browser has been closed')
  );
}

async function waitForPageNavigationStabilization(session, reason) {
  console.error(`[driver] navigation_wait start instance=${session.id} reason=${reason}`);
  try {
    await withOperationTimeout(
      `navigation_wait_load_${reason}`,
      session.page.waitForLoadState('load', { timeout: 5000 }),
      6000
    );
  } catch {}
  try {
    await withOperationTimeout(
      `navigation_wait_domcontentloaded_${reason}`,
      session.page.waitForLoadState('domcontentloaded', { timeout: 5000 }),
      6000
    );
  } catch {}
  await delay(300);
  console.error(`[driver] navigation_wait done instance=${session.id} reason=${reason}`);
}

async function focusAuraPage(page) {
  try {
    await withOperationTimeout('focus_page_bring_to_front', page.bringToFront(), 1500);
  } catch (error) {
    console.error(`[driver] focus_page_bring_to_front_skipped reason=${normalizeClickError(error)}`);
  }
  return true;
}

async function focusAuraPageSafe(page, instanceId, context) {
  try {
    return await focusAuraPage(page);
  } catch (error) {
    console.error(
      `[driver] focus_page skipped instance=${instanceId ?? 'unknown'} context=${context} reason=${normalizeClickError(
        error
      )}`
    );
    return false;
  }
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function selectorToFallbackLabel(selector) {
  const raw = String(selector ?? '').trim();
  if (!raw.startsWith('#')) {
    return '';
  }
  let label = raw.slice(1);
  if (!label.startsWith('aura-')) {
    return '';
  }
  label = label.replace(/^aura-/, '');
  label = label.replace(/^-/, '').replace(/-button$/, '');
  label = label.replace(/^nav-/, '');
  label = label.replace(/(?:^|[-_])(?:button|btn|input|field)$/g, '').trim();
  label = label.replace(/[\-_]+/g, ' ').trim();
  if (!label) {
    return '';
  }
  return label.charAt(0).toUpperCase() + label.slice(1).toLowerCase();
}

function normalizeClickError(error) {
  return error?.message || String(error || 'unknown');
}

function keydownEventData(key) {
  const meta = {
    Backspace: { code: 'Backspace', keyCode: 8, which: 8 },
    Delete: { code: 'Delete', keyCode: 46, which: 46 },
    Enter: { code: 'Enter', keyCode: 13, which: 13 },
    Escape: { code: 'Escape', keyCode: 27, which: 27 },
    Tab: { code: 'Tab', keyCode: 9, which: 9 },
    ArrowUp: { code: 'ArrowUp', keyCode: 38, which: 38 },
    ArrowDown: { code: 'ArrowDown', keyCode: 40, which: 40 },
    ArrowLeft: { code: 'ArrowLeft', keyCode: 37, which: 37 },
    ArrowRight: { code: 'ArrowRight', keyCode: 39, which: 39 },
    Home: { code: 'Home', keyCode: 36, which: 36 },
    End: { code: 'End', keyCode: 35, which: 35 },
    PageUp: { code: 'PageUp', keyCode: 33, which: 33 },
    PageDown: { code: 'PageDown', keyCode: 34, which: 34 }
  };
  return meta[key] ?? null;
}

async function dispatchHarnessKey(page, rawKey, repeat = 1) {
  return page.evaluate(
    ({ key, repeatCount }) => {
      const harness = window.__AURA_HARNESS__;
      if (!harness) {
        return { ok: false, reason: 'harness_missing' };
      }
      if (key.length === 1 && typeof harness.send_keys === 'function') {
        for (let index = 0; index < repeatCount; index += 1) {
          harness.send_keys(key);
        }
        return { ok: true, mode: 'send_keys' };
      }
      if (typeof harness.send_key === 'function') {
        harness.send_key(key, repeatCount);
        return { ok: true, mode: 'send_key' };
      }
      return { ok: false, reason: 'harness_send_key_missing' };
    },
    { key: rawKey, repeatCount: repeat }
  );
}

async function dispatchHarnessKeysText(page, text) {
  return page.evaluate((value) => {
    const harness = window.__AURA_HARNESS__;
    if (!harness || typeof harness.send_keys !== 'function') {
      return { ok: false, reason: 'harness_send_keys_missing' };
    }
    harness.send_keys(value);
    return { ok: true, mode: 'send_keys' };
  }, text);
}

async function fallbackKeyboardPress(page, rawKey) {
  const mapped = mapPlaywrightKey(rawKey);
  const keyData = keydownEventData(mapped) || {
    code: mapped,
    keyCode: 0,
    which: 0
  };
  return page.evaluate(
    ({ targetKey, targetCode, keyCode, which, rawKey }) => {
      const active = document.activeElement || document.body;
      if (!active) {
        return { ok: false, reason: 'no_active_element' };
      }

      const eventInit = {
        key: targetKey,
        code: targetCode,
        keyCode,
        which,
        bubbles: true,
        cancelable: true
      };

      const down = new KeyboardEvent('keydown', eventInit);
      const up = new KeyboardEvent('keyup', eventInit);
      active.dispatchEvent(down);
      if (targetKey === 'Enter' || targetKey === 'Tab' || targetKey === 'Escape') {
        const press = new KeyboardEvent('keypress', eventInit);
        active.dispatchEvent(press);
      }
      active.dispatchEvent(up);

      const editableField =
        active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement;
      if (rawKey && rawKey.length === 1 && editableField) {
        const isEditable = targetKey.length === 1 && !(active.readOnly || active.disabled);
        if (isEditable) {
          const value = active.value || '';
          active.value = `${value}${targetKey}`;
          active.dispatchEvent(new Event('input', { bubbles: true }));
        }
      }

      return { ok: true, key: targetKey, activeTag: active.tagName };
    },
    {
      targetKey: mapped,
      targetCode: keyData.code,
      keyCode: keyData.keyCode,
      which: keyData.which,
      rawKey
    }
  );
}

async function clickLocatorWithDiagnostics(locator, context) {
  const actionTimeoutMs = 2800;
  const result = await locator.evaluate((element) => {
    if (!(element instanceof HTMLElement)) {
      return { ok: false, reason: 'not_html_element' };
    }
    const style = window.getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden') {
      return { ok: false, reason: 'not_visible' };
    }
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return { ok: false, reason: 'zero_size' };
    }
    if (element.hasAttribute('disabled') || element.getAttribute('aria-disabled') === 'true') {
      return { ok: false, reason: 'disabled' };
    }
    return {
      ok: true,
      id: element.id || null,
      text: String(element.textContent ?? '').replace(/\s+/g, ' ').trim()
    };
  });
  if (!result || result.ok !== true) {
    throw new Error(`${context} precheck_failed ${JSON.stringify(result ?? {})}`);
  }

  try {
    await locator.scrollIntoViewIfNeeded();
  } catch {
    // Non-fatal: some hidden or detached controls are still best-effort actionable.
  }

  try {
    await withOperationTimeout(
      `locator_click:${context}`,
      locator.click({
        timeout: actionTimeoutMs,
        noWaitAfter: true
      }),
      actionTimeoutMs + 200
    );
  } catch (error) {
    if (String(error?.message ?? error).includes('Timeout')) {
      throw error;
    }
    await withOperationTimeout(
      `locator_click_force:${context}`,
      locator.click({
        timeout: actionTimeoutMs,
        force: true,
        noWaitAfter: true
      }),
      actionTimeoutMs + 200
    ).catch(() => {
      throw error;
    });
  }

  return result;
}

async function clickLocatorBySelectorFallback(page, selector, session) {
  const normalizedSelector = String(selector || '').trim();
  try {
    const fallback = await page.evaluate(({ targetSelector }) => {
      const element = document.querySelector(targetSelector);
      if (!element) {
        return { ok: false, reason: 'element_not_found' };
      }
      if (
        !(element instanceof HTMLElement) ||
        element.hasAttribute('disabled') ||
        element.getAttribute('aria-disabled') === 'true'
      ) {
        return { ok: false, reason: 'not_clickable' };
      }
      element.focus();
      element.click();
      return {
        ok: true,
        id: String(element.id ?? ''),
        text: String(element.textContent ?? '').replace(/\s+/g, ' ').trim()
      };
    }, { targetSelector: normalizedSelector });
    if (fallback.ok) {
      console.error(
        `[driver] click_button js_fallback_ok instance=${session.id} selector=${normalizedSelector}`
      );
      return fallback;
    }
    throw new Error(`js_fallback_failed reason=${fallback.reason}`);
  } catch (error) {
    console.error(
      `[driver] click_button js_fallback_failed instance=${session.id} selector=${normalizedSelector} error=${normalizeClickError(
        error
      )}`
    );
    throw error;
  }
}

async function clickByCssSelector(page, selector, session) {
  const normalizedSelector = String(selector || '').trim();
  const maxAttempts = 2;
  let lastError = null;
  const navShortcut = navigationShortcutForSelector(normalizedSelector);

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const attemptContext = `css:${normalizedSelector}:attempt${attempt}`;
    try {
      const locator = page.locator(normalizedSelector).first();

      await withOperationTimeout(
        `click_wait_${attemptContext}`,
        locator.waitFor({ state: 'attached', timeout: 600 }),
        800
      );

      return await clickLocatorWithDiagnostics(locator, attemptContext);
    } catch (error) {
      lastError = error;
      const message = normalizeClickError(error);
      console.error(
        `[driver] click_button css attempt_failed instance=${session.id} selector=${normalizedSelector} attempt=${attempt} error=${message}`
      );
      if (isNavigationTransitionError(error)) {
        await waitForPageNavigationStabilization(session, attemptContext);
      }
      try {
        return await withOperationTimeout(
          `css_fallback_${attemptContext}`,
          clickLocatorBySelectorFallback(page, normalizedSelector, session),
          900
        );
      } catch (fallbackError) {
        lastError = fallbackError;
      }
      if (attempt + 1 < maxAttempts) {
        await delay(80);
        continue;
      }
    }
  }

  if (navShortcut) {
    const shortcutReason = `css_fallback_${normalizedSelector}`;
    try {
      console.error(
        `[driver] click_button css fallback_key instance=${session.id} selector=${normalizedSelector} key=${navShortcut} reason=${shortcutReason}`
      );
      await withOperationTimeout(
        `css_nav_shortcut_${shortcutReason}`,
        dispatchHarnessKey(page, navShortcut, 1),
        2000
      );
      return { ok: true, id: normalizedSelector, text: `shortcut:${navShortcut}` };
    } catch (fallbackError) {
      console.error(
        `[driver] click_button css fallback_key_failed instance=${session.id} selector=${normalizedSelector} key=${navShortcut} error=${normalizeClickError(fallbackError)}`
      );
    }
  }

  throw new Error(`css_click_retries_exhausted selector=${normalizedSelector} ${normalizeClickError(lastError)}`);
}

async function clickByLabelText(page, label, session) {
  const normalizedLabel = normalizeClickTarget(label);
  const candidates = [
    page.getByRole('button', { name: normalizedLabel, exact: true }),
    page.getByRole('link', { name: normalizedLabel, exact: true }),
    page.locator('button, a, [role="button"], [role="link"]').filter({
      hasText: new RegExp(`^${escapeRegex(normalizedLabel)}$`, 'i')
    })
  ];
  const maxAttempts = 3;
  let lastError = null;
  await focusAuraPageSafe(page, session.id, `label_start:${normalizedLabel}`);

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    for (const candidate of candidates) {
      const context = `label:${normalizedLabel}:attempt${attempt}`;
      try {
        await withOperationTimeout(
          `click_label_wait_${context}`,
          candidate.first().waitFor({ state: 'visible', timeout: 2500 }),
          2900
        );
        return await clickLocatorWithDiagnostics(candidate.first(), context);
      } catch (error) {
        lastError = error;
        if (isNavigationTransitionError(error)) {
          await waitForPageNavigationStabilization(session, context);
          continue;
        }
      }
    }
    if (attempt + 1 < maxAttempts) {
      await delay(175);
    }
  }

  throw new Error(`label_click_failed label=${normalizedLabel} ${normalizeClickError(lastError)}`);
}

function mapPlaywrightKey(key) {
  if (key.length === 1) {
    return key;
  }

  switch (String(key ?? '').trim().toLowerCase()) {
    case 'enter':
      return 'Enter';
    case 'esc':
    case 'escape':
      return 'Escape';
    case 'tab':
      return 'Tab';
    case 'backtab':
      return 'Shift+Tab';
    case 'up':
      return 'ArrowUp';
    case 'down':
      return 'ArrowDown';
    case 'left':
      return 'ArrowLeft';
    case 'right':
      return 'ArrowRight';
    case 'home':
      return 'Home';
    case 'end':
      return 'End';
    case 'pageup':
      return 'PageUp';
    case 'pagedown':
      return 'PageDown';
    case 'backspace':
      return 'Backspace';
    case 'delete':
      return 'Delete';
    default:
      throw new Error(`unsupported key: ${key}`);
  }
}

async function pressMappedKey(page, key) {
  const mapped = mapPlaywrightKey(key);
  const actionTimeoutMs = 1200;
  console.error(`[driver] key_press start key=${key} mapped=${mapped}`);
  try {
    await withOperationTimeout(`key_press:${mapped}`, page.keyboard.press(mapped), actionTimeoutMs);
    console.error(`[driver] key_press done key=${key} mapped=${mapped}`);
    return;
  } catch (error) {
    console.error(
      `[driver] key_press_failed key=${key} mapped=${mapped} error=${normalizeClickError(error)}`
    );
  }

  const fallback = await withOperationTimeout(
    `key_press_fallback_${mapped}`,
    dispatchHarnessKey(page, key, 1),
    700
  );
  if (!fallback?.ok) {
    const domFallback = await withOperationTimeout(
      `key_press_dom_fallback_${mapped}`,
      fallbackKeyboardPress(page, key),
      1200
    );
    if (!domFallback?.ok) {
      throw new Error(
        `keyboard press failed for ${mapped}: harness=${JSON.stringify(fallback)} dom=${JSON.stringify(domFallback)}`
      );
    }
  }
  console.error(`[driver] key_press done key=${key} mapped=${mapped}`);
}

async function flushTypedBuffer(page, buffer) {
  if (!buffer) {
    return '';
  }
  const actionTimeoutMs = 5000;
  const preview = JSON.stringify(buffer.length > 80 ? `${buffer.slice(0, 80)}…` : buffer);
  console.error(`[driver] key_type start bytes=${buffer.length} preview=${preview}`);
  for (const ch of buffer) {
    const mapped = ch === ' ' ? 'Space' : ch;
    await withOperationTimeout(
      `keyboard_type:${JSON.stringify(mapped)}`,
      page.keyboard.press(mapped),
      actionTimeoutMs
    );
  }
  console.error(`[driver] key_type done bytes=${buffer.length}`);
  return '';
}

function decodeEscapeSequence(value, startIndex) {
  if (value[startIndex] !== '\u001b') {
    return null;
  }
  const next = value[startIndex + 1];
  if (next !== '[') {
    return { consumed: 1, key: 'esc' };
  }
  let cursor = startIndex + 2;
  let body = '';
  while (cursor < value.length) {
    const ch = value[cursor];
    body += ch;
    if ((ch >= 'A' && ch <= 'Z') || ch === '~') {
      break;
    }
    cursor += 1;
  }

  switch (body) {
    case 'A':
      return { consumed: 3, key: 'up' };
    case 'B':
      return { consumed: 3, key: 'down' };
    case 'C':
      return { consumed: 3, key: 'right' };
    case 'D':
      return { consumed: 3, key: 'left' };
    case 'H':
      return { consumed: 3, key: 'home' };
    case 'F':
      return { consumed: 3, key: 'end' };
    case 'Z':
      return { consumed: 3, key: 'backtab' };
    case '5~':
      return { consumed: 4, key: 'pageup' };
    case '6~':
      return { consumed: 4, key: 'pagedown' };
    case '3~':
      return { consumed: 4, key: 'delete' };
    default:
      return { consumed: 1, key: 'esc' };
  }
}

async function typeKeyStream(page, rawKeys) {
  const value = String(rawKeys ?? '');
  let buffer = '';

  for (let index = 0; index < value.length; index += 1) {
    const ch = value[index];
    if (ch === '\r') {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, 'enter');
      if (value[index + 1] === '\n') {
        index += 1;
      }
      continue;
    }
    if (ch === '\n') {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, 'enter');
      continue;
    }
    if (ch === '\t') {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, 'tab');
      continue;
    }
    if (ch === '\u001b') {
      buffer = await flushTypedBuffer(page, buffer);
      const sequence = decodeEscapeSequence(value, index);
      if (sequence) {
        await pressMappedKey(page, sequence.key);
        index += sequence.consumed - 1;
        continue;
      }
    }
    buffer += ch;
  }

  await flushTypedBuffer(page, buffer);
}

async function pageLivenessProbe(page) {
  return withOperationTimeout(
    'page_liveness_probe',
    page.evaluate(() => {
      const active = document.activeElement;
      return {
        title: document.title ?? '',
        readyState: document.readyState ?? '',
        visibilityState: document.visibilityState ?? '',
        hasFocus: typeof document.hasFocus === 'function' ? document.hasFocus() : false,
        activeTag: active?.tagName ?? null,
        activeId: active?.id ?? null,
        activeClass: active?.className ?? null,
      };
    }),
    3000
  );
}

async function readDomSnapshot(page) {
  return withOperationTimeout(
    'dom_snapshot',
    page.evaluate(() => {
      const root =
        document.getElementById('aura-app-root') ??
        document.querySelector('main:last-of-type') ??
        document.body;
      const text = root?.textContent ?? '';
      return {
        screen: text,
        raw_screen: text,
        authoritative_screen: text,
        normalized_screen: text,
        capture_consistency: 'settled'
      };
    }),
    15000
  ).then((payload) => ({
    ...payload,
    screen: normalizeScreenText(payload.screen),
    raw_screen: normalizeScreenText(payload.raw_screen),
    authoritative_screen: normalizeScreenText(payload.authoritative_screen),
    normalized_screen: normalizeScreenText(payload.normalized_screen)
  }));
}

function domSnapshotFromCache(session) {
  const text = session.domState?.text ?? '';
  return {
    screen: text,
    raw_screen: text,
    authoritative_screen: text,
    normalized_screen: text,
    capture_consistency: 'settled'
  };
}

async function waitForDomPatterns(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const patterns = Array.isArray(params?.patterns)
    ? params.patterns.map((value) => normalizeScreenText(String(value))).filter(Boolean)
    : [];
  if (patterns.length === 0) {
    throw new Error('patterns is required');
  }
  const timeoutMs = Number(params?.timeout_ms ?? 30000);
  if (session.domState) {
    const text = session.domState?.text ?? '';
    if (patterns.some((pattern) => text.includes(pattern))) {
      return parseSnapshotPayload(domSnapshotFromCache(session));
    }
    console.error(
      `[driver] wait_for_dom_patterns cache_miss instance=${instanceId} patterns=${JSON.stringify(patterns)}; falling back to playwright`
    );
  }
  const deadline = Date.now() + timeoutMs;
  let lastText = '';
  while (Date.now() < deadline) {
    const text = session.domState?.text ?? '';
    lastText = text || lastText;
    if (patterns.some((pattern) => text.includes(pattern))) {
      return parseSnapshotPayload(domSnapshotFromCache(session));
    }
    await delay(50);
  }

  try {
    const snapshot = await withOperationTimeout(
      'wait_for_dom_patterns_snapshot_final',
      readDomSnapshot(session.page),
      2000
    );
    const text = normalizeScreenText(snapshot?.authoritative_screen ?? snapshot?.screen ?? '');
    lastText = text || lastText;
    if (patterns.some((pattern) => text.includes(pattern))) {
      return parseSnapshotPayload(snapshot);
    }
  } catch (error) {
    lastText = `${lastText}\n[dom-read-error] ${error.message}`.trim();
  }
  throw new Error(
    `wait_for_dom_patterns timed out after ${timeoutMs}ms patterns=${JSON.stringify(
      patterns
    )} text_snippet=${JSON.stringify(lastText.slice(0, 1600))} console_tail=${JSON.stringify(
      consoleTailText(session)
    )}`
  );
}

async function waitForSelector(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const selector = String(params?.selector ?? '').trim();
  if (!selector) {
    throw new Error('selector is required');
  }
  const timeoutMs = Number(params?.timeout_ms ?? 30000);
  console.error(`[driver] wait_for_selector start instance=${instanceId} selector=${selector} cache=${selector.startsWith('#') && !!session.domState}`);
  if (selector.startsWith('#') && session.domState?.ids?.has(selector.slice(1))) {
    console.error(`[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=cache`);
    return parseSnapshotPayload(domSnapshotFromCache(session));
  }
  if (selector.startsWith('#') && session.domState) {
    console.error(
      `[driver] wait_for_selector cache_miss instance=${instanceId} selector=${selector}; falling back to playwright`
    );
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      if (session.domState?.ids?.has(selector.slice(1))) {
        console.error(
          `[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=cache_poll`
        );
        return parseSnapshotPayload(domSnapshotFromCache(session));
      }
      await delay(50);
    }
  }
  try {
    await withOperationTimeout(
      `wait_for_selector:${selector}`,
      session.page.locator(selector).first().waitFor({ state: 'visible', timeout: timeoutMs }),
      timeoutMs + 1000
    );
  } catch (error) {
    const diagnostics = await session.page.evaluate(() => {
      const ids = Array.from(document.querySelectorAll('[id]'))
        .map((element) => element.id)
        .filter((id) => id.startsWith('aura-contact-item-'))
        .slice(0, 50);
      const root =
        document.getElementById('aura-app-root') ??
        document.querySelector('main:last-of-type') ??
        document.body;
      const text = String(root?.textContent ?? '')
        .replace(/\s+/g, ' ')
        .trim()
        .slice(0, 1200);
      return { ids, text };
    }).catch(() => ({ ids: [], text: '' }));
    throw new Error(
      `${error.message} current_contact_ids=${JSON.stringify(diagnostics.ids)} text_snippet=${JSON.stringify(diagnostics.text)}`
    );
  }
  console.error(`[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=playwright`);
  return parseSnapshotPayload(domSnapshotFromCache(session));
}

function withOperationTimeout(label, promise, timeoutMs = 5000) {
  let timer = null;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
  });
  return Promise.race([promise, timeout]).finally(() => {
    if (timer) {
      clearTimeout(timer);
    }
  });
}

const UI_STATE_TIMEOUT_MS = 15000;

function appRootLocator(page) {
  return page.locator('main').last();
}

function normalizeClickTarget(label) {
  return String(label || '')
    .trim()
    .replace(/^\(|^\"|^'|^\[|^\{|^</g, '')
    .replace(/\)|\"|\'|\]|\}|>$|:$|\.$/g, '')
    .trim();
}

function navigationShortcutForSelector(selector) {
  if (!selector) {
    return null;
  }
  if (selector.includes('aura-nav-neighborhood')) {
    return '1';
  }
  if (selector.includes('aura-nav-chat')) {
    return '2';
  }
  if (selector.includes('aura-nav-contacts')) {
    return '3';
  }
  if (selector.includes('aura-nav-notifications')) {
    return '4';
  }
  if (selector.includes('aura-nav-settings')) {
    return '5';
  }
  return null;
}

function parseBoundedInt(params, key, fallback, min, max) {
  const raw = params?.[key];
  if (raw == null) {
    return fallback;
  }
  if (typeof raw !== 'number' || !Number.isFinite(raw) || !Number.isInteger(raw)) {
    throw new Error(`${key} must be an integer number`);
  }
  if (raw < min || raw > max) {
    throw new Error(`${key} must be between ${min} and ${max}, got ${raw}`);
  }
  return raw;
}

function parseStartOptions(params) {
  const instanceId = normalizeInstanceId(params);
  const appUrl = String(params?.app_url ?? 'http://127.0.0.1:4173');
  const dataDir = String(params?.data_dir ?? path.join('.tmp', 'harness', instanceId));
  const headless = params?.headless !== false;
  const artifactDir = params?.artifact_dir ? String(params.artifact_dir) : null;
  const pageGotoTimeoutMs = parseBoundedInt(
    params,
    'page_goto_timeout_ms',
    DEFAULT_PAGE_GOTO_TIMEOUT_MS,
    1,
    MAX_TIMEOUT_MS
  );
  const harnessReadyTimeoutMs = parseBoundedInt(
    params,
    'harness_ready_timeout_ms',
    DEFAULT_HARNESS_READY_TIMEOUT_MS,
    1,
    MAX_TIMEOUT_MS
  );
  const startMaxAttempts = parseBoundedInt(
    params,
    'start_max_attempts',
    DEFAULT_START_MAX_ATTEMPTS,
    1,
    MAX_START_ATTEMPTS
  );
  const startRetryBackoffMs = parseBoundedInt(
    params,
    'start_retry_backoff_ms',
    DEFAULT_START_RETRY_BACKOFF_MS,
    0,
    MAX_TIMEOUT_MS
  );

  return {
    instanceId,
    appUrl,
    dataDir,
    headless,
    artifactDir,
    pageGotoTimeoutMs,
    harnessReadyTimeoutMs,
    startMaxAttempts,
    startRetryBackoffMs
  };
}

function requestTimeoutMs(method, params) {
  switch (method) {
    case 'wait_for_dom_patterns':
    case 'wait_for_selector': {
      const timeoutMs = Number(params?.timeout_ms ?? 30000);
      return Math.max(1000, timeoutMs + 5000);
    }
    case 'wait_for_ui_state': {
      const timeoutMs = Number(params?.timeout_ms ?? UI_STATE_TIMEOUT_MS);
      return Math.max(1000, timeoutMs + 5000);
    }
    case 'click_button':
    case 'fill_input':
      return 30000;
    case 'start_page': {
      const pageGotoTimeoutMs = Number(params?.page_goto_timeout_ms ?? DEFAULT_PAGE_GOTO_TIMEOUT_MS);
      const harnessReadyTimeoutMs = Number(
        params?.harness_ready_timeout_ms ?? DEFAULT_HARNESS_READY_TIMEOUT_MS
      );
      return Math.max(1000, pageGotoTimeoutMs + harnessReadyTimeoutMs + 10000);
    }
    default:
      return 15000;
  }
}

function withHarnessInstanceQuery(appUrl, instanceId) {
  const url = new URL(appUrl);
  url.searchParams.set('__aura_harness_instance', instanceId);
  return url.toString();
}

function delay(ms) {
  if (ms <= 0) {
    return Promise.resolve();
  }
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function startPage(params) {
  const options = parseStartOptions(params);
  const {
    instanceId,
    appUrl,
    dataDir,
    headless,
    artifactDir,
    pageGotoTimeoutMs,
    harnessReadyTimeoutMs,
    startMaxAttempts,
    startRetryBackoffMs
  } = options;
  const targetUrl = withHarnessInstanceQuery(appUrl, instanceId);

  if (sessions.has(instanceId)) {
    await stop({ instance_id: instanceId });
  }

  ensureDir(dataDir);
  ensureDir(artifactDir);

  const consoleLog = [];
  let lastError = null;

  for (let attempt = 1; attempt <= startMaxAttempts; attempt += 1) {
    let context = null;
    try {
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} launchPersistentContext start`
      );
      context = await chromium.launchPersistentContext(dataDir, {
        headless,
        viewport: { width: 1280, height: 900 },
        ignoreHTTPSErrors: true
      });
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} launchPersistentContext done`
      );

      const page = context.pages()[0] ?? (await context.newPage());
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} page acquired`
      );
      const session = {
        id: instanceId,
        context,
        page,
        headless,
        appUrl: targetUrl,
        dataDir,
        artifactDir,
        consoleLog,
        tracePath:
          artifactDir && PLAYWRIGHT_TRACE_ENABLED
            ? path.join(artifactDir, `${instanceId}-trace.zip`)
            : null,
        domState: normalizeDomState({ text: '', ids: [] }),
        uiStateCache: null,
        uiStateCacheJson: null,
        uiStateVersion: 0,
        uiStateWaiters: [],
        renderHeartbeat: null
      };
      sessions.set(instanceId, session);

      page.on('console', (message) => {
        const text = message.text();
        if (text.startsWith(UI_STATE_JSON_LOG_PREFIX)) {
          const payload = text.slice(UI_STATE_JSON_LOG_PREFIX.length);
          session.uiStateCacheJson = payload;
          try {
            session.uiStateCache = JSON.parse(payload);
          } catch {
            session.uiStateCache = null;
          }
          consoleLog.push(`[${nowIso()}] ${message.type()}: ${UI_STATE_JSON_LOG_PREFIX}<json>`);
          return;
        }
        if (text.startsWith(UI_STATE_LOG_PREFIX)) {
          consoleLog.push(`[${nowIso()}] ${message.type()}: ${text}`);
          return;
        }
        consoleLog.push(`[${nowIso()}] ${message.type()}: ${text}`);
      });

      if (artifactDir && PLAYWRIGHT_TRACE_ENABLED) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing start`
        );
        await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing done`
        );
      }

      await installUiStateObserver(page, session);
      installPageNavigationReset(session);

      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} goto start url=${targetUrl}`
      );
      await page.goto(targetUrl, { waitUntil: 'commit', timeout: pageGotoTimeoutMs });
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} goto done`
      );
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensurePageInteractive start`
      );
      await ensurePageInteractive(page, harnessReadyTimeoutMs);
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensurePageInteractive done`
      );
      try {
        const bindingType = await page.evaluate(
          () => typeof window.__AURA_DRIVER_PUSH_UI_STATE
        );
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} uiStateBinding type=${bindingType}`
        );
      } catch (error) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} uiStateBinding probe failed: ${
            error?.message ?? String(error)
          }`
        );
      }
      try {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout start`
        );
        await ensureHarnessWithTimeout(page, Math.min(harnessReadyTimeoutMs, 5000));
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout done`
      );
      await assertRootStructure({ page }, 'startup');
      } catch (error) {
        consoleLog.push(
          `[${nowIso()}] harness bridge not ready after startup: ${error?.message ?? String(error)}`
        );
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout optional failure: ${
            error?.message ?? String(error)
          }`
        );
      }
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installDomObserver start`
      );
      await installDomObserver(page, session);
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installDomObserver done`
      );
      try {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready start`
        );
        await uiState({ instance_id: instanceId });
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready done`
        );
      } catch (error) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready failed: ${
            error?.message ?? String(error)
          }`
        );
        throw error;
      }

      return {
        instance_id: instanceId,
        app_url: targetUrl,
        data_dir: dataDir,
        headless
      };
    } catch (error) {
      sessions.delete(instanceId);
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} failed: ${
          error?.stack ?? error?.message ?? String(error)
        }`
      );
      lastError = error;
      consoleLog.push(
        `[${nowIso()}] start_page attempt ${attempt}/${startMaxAttempts} failed: ${
          error?.message ?? String(error)
        }`
      );
      if (context) {
        try {
          await context.close();
        } catch {
          // Continue retries on close errors.
        }
      }
      if (attempt < startMaxAttempts) {
        await delay(startRetryBackoffMs);
      }
    }
  }

  throw new Error(
    `start_page failed after ${startMaxAttempts} attempts for ${instanceId} app_url=${appUrl}: ${
      lastError?.stack ?? lastError?.message ?? String(lastError)
    }`
  );
}

function getSession(instanceId) {
  const session = sessions.get(instanceId);
  if (!session) {
    throw new Error(`unknown session: ${instanceId}`);
  }
  return session;
}

async function sendKeys(params) {
  const instanceId = normalizeInstanceId(params);
  const keys = String(params?.keys ?? '');
  const session = getSession(instanceId);

  console.error(`[driver] send_keys start instance=${instanceId} bytes=${keys.length}`);
  try {
    const harnessResult = await withOperationTimeout(
      `send_keys_harness:${instanceId}`,
      dispatchHarnessKeysText(session.page, keys),
      2000
    );
    if (harnessResult?.ok) {
      console.error(
        `[driver] send_keys done instance=${instanceId} mode=${harnessResult.mode ?? 'harness'}`
      );
      return { status: 'sent', bytes: keys.length, mode: harnessResult.mode ?? 'harness' };
    }
  } catch (error) {
    console.error(
      `[driver] send_keys harness_path_failed instance=${instanceId} error=${normalizeClickError(error)}`
    );
  }
  console.error(`[driver] send_keys focus start instance=${instanceId}`);
  await focusAuraPageSafe(session.page, instanceId, 'send_keys');
  console.error(`[driver] send_keys focus done instance=${instanceId}`);
  console.error(`[driver] send_keys type start instance=${instanceId}`);
  await withOperationTimeout(
    `type_keys:${instanceId}`,
    typeKeyStream(session.page, keys),
    8000
  );
  console.error(`[driver] send_keys type done instance=${instanceId}`);

  console.error(`[driver] send_keys done instance=${instanceId}`);
  return { status: 'sent', bytes: keys.length };
}

async function sendKey(params) {
  const instanceId = normalizeInstanceId(params);
  const key = String(params?.key ?? '');
  const repeat = Number(params?.repeat ?? 1);
  const session = getSession(instanceId);
  const count = Number.isFinite(repeat) ? Math.max(1, Math.floor(repeat)) : 1;

  try {
    const harnessResult = await withOperationTimeout(
      `send_key_harness:${instanceId}`,
      dispatchHarnessKey(session.page, key, count),
      2000
    );
    if (harnessResult?.ok) {
      return { status: 'sent', mode: harnessResult.mode ?? 'harness' };
    }
  } catch (error) {
    console.error(
      `[driver] send_key harness_path_failed instance=${instanceId} key=${key} error=${normalizeClickError(error)}`
    );
  }

  await focusAuraPageSafe(session.page, instanceId, 'send_key');
  for (let index = 0; index < count; index += 1) {
    await pressMappedKey(session.page, key);
  }

  return { status: 'sent' };
}

async function snapshot(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const screenshot = params?.screenshot !== false;

  let payload;
  try {
    payload =
      (await withOperationTimeout(
        'snapshot',
        session.page.evaluate(() => {
          if (window.__AURA_HARNESS__?.snapshot) {
            return window.__AURA_HARNESS__.snapshot();
          }
          return null;
        })
      )) ?? (await readDomSnapshot(session.page));
  } catch (error) {
    throw new Error(
      `${error}\nBrowser console tail:\n${consoleTailText(session)}`
    );
  }
  const normalized = parseSnapshotPayload(payload);

  let screenshotPath = null;
  if (screenshot && session.artifactDir) {
    screenshotPath = path.join(session.artifactDir, `${instanceId}-${Date.now()}.png`);
    await session.page.screenshot({ path: screenshotPath, fullPage: true });
  }

  return {
    ...normalized,
    screenshot_path: screenshotPath
  };
}

async function uiState(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const recentConsole = consoleTailText(session, 8).replace(/\n/g, ' | ');
  const recentConsoleLower = recentConsole.toLowerCase();
  console.error(
    `[driver] ui_state start instance=${instanceId} cache_type=${typeof session.uiStateCache} cache_json=${
      typeof session.uiStateCacheJson
    } heartbeat_seq=${session.renderHeartbeat?.render_seq ?? 'none'} console_tail=${recentConsole}`
  );

  const tryParseUiState = (value) => {
    if (typeof value === 'string') {
      try {
        return JSON.parse(value);
      } catch {
        return null;
      }
    }
    return value && typeof value === 'object' ? value : null;
  };

  const readStructuredUiState = async (reason, timeoutMs = 1000) => {
    console.error(
      `[driver] ui_state structured_read start instance=${instanceId} reason=${reason} timeout_ms=${timeoutMs}`
    );
    const payload = await withOperationTimeout(
      `ui_state_structured_${reason}`,
      session.page.evaluate(() => {
        if (typeof window.__AURA_UI_STATE__ === 'function') {
          return window.__AURA_UI_STATE__();
        }
        if (typeof window.__AURA_HARNESS__?.ui_state === 'function') {
          return window.__AURA_HARNESS__.ui_state();
        }
        return null;
      }),
      timeoutMs
    );
    const parsed = tryParseUiState(payload);
    if (parsed && typeof parsed === 'object') {
      await ensureUiStateRenderConvergence(session, parsed, reason);
      storeUiState(session, parsed);
      console.error(
        `[driver] ui_state structured_read done instance=${instanceId} reason=${reason}`
      );
      return parsed;
    }
    console.error(
      `[driver] ui_state structured_read unavailable instance=${instanceId} reason=${reason}`
    );
    return null;
  };

  const readStructuredUiStateWithNavigationRecovery = async (reason, timeoutMs = 1000) => {
    try {
      return await readStructuredUiState(reason, timeoutMs);
    } catch (error) {
      if (!isNavigationTransitionError(error)) {
        throw error;
      }
      session.uiStateCache = null;
      session.uiStateCacheJson = null;
      console.error(
        `[driver] ui_state structured_navigation_recovery instance=${instanceId} reason=${reason}`
      );
      await waitForPageNavigationStabilization(session, `structured_navigation_${reason}`);
      await assertRootStructure(session, `ui_state_after_navigation_${reason}`);
      return readStructuredUiState(`post_navigation_${reason}`, UI_STATE_TIMEOUT_MS);
    }
  };

  if (session.uiStateCache && typeof session.uiStateCache === 'object') {
    console.error(`[driver] ui_state cache_hit instance=${instanceId}`);
    const cached =
      typeof session.uiStateCacheJson === 'string'
        ? tryParseUiState(session.uiStateCacheJson)
        : session.uiStateCache;
    const staleOnboardingCache =
      cached &&
      cached.screen === 'onboarding' &&
      cached.readiness === 'loading' &&
      (recentConsoleLower.includes('[web-onboarding] submit_account ok') ||
        recentConsoleLower.includes('[web-import-device] accepted runtime_devices=') ||
        recentConsoleLower.includes('[web-import-device] staged_reload') ||
        recentConsoleLower.includes('[web-import-device] accepting_on_bound_runtime'));
    if (staleOnboardingCache) {
      console.error(`[driver] ui_state stale_onboarding_cache instance=${instanceId}`);
      const refreshed = await readStructuredUiStateWithNavigationRecovery(
        'post_onboarding_submit',
        2000
      );
      if (refreshed) {
        return refreshed;
      }
    }
    if (cached && !cachedUiStateConverged(session, cached)) {
      console.error(`[driver] ui_state cache_diverged instance=${instanceId}`);
      try {
        const refreshed = await readStructuredUiStateWithNavigationRecovery(
          'cache_divergence',
          2000
        );
        if (refreshed) {
          console.error(`[driver] ui_state cache_divergence_recovered instance=${instanceId}`);
          return refreshed;
        }
      } catch (error) {
        throw new Error(
          `ui_state cache diverged from committed render instance=${instanceId} screen=${cached?.screen ?? 'unknown'} modal=${
            cached?.open_modal ?? 'none'
          } heartbeat=${JSON.stringify(session.renderHeartbeat)} current_ids=${JSON.stringify(domStateIdList(session))} text_snippet=${JSON.stringify(
            session?.domState?.text ?? ''
          )} refresh_error=${error?.message ?? String(error)}`
        );
      }
      throw new Error(
        `ui_state cache diverged from committed render instance=${instanceId} screen=${cached?.screen ?? 'unknown'} modal=${
          cached?.open_modal ?? 'none'
        } heartbeat=${JSON.stringify(session.renderHeartbeat)} current_ids=${JSON.stringify(domStateIdList(session))} text_snippet=${JSON.stringify(
          session?.domState?.text ?? ''
        )}`
      );
    }
    if (cached) {
      console.error(`[driver] ui_state authoritative_cache_hit instance=${instanceId}`);
      return cached;
    }
  }

  try {
    await assertRootStructure(session, 'ui_state');
  } catch (error) {
    if (!isNavigationTransitionError(error)) {
      throw error;
    }
    session.uiStateCache = null;
    session.uiStateCacheJson = null;
    console.error(`[driver] ui_state navigation_retry instance=${instanceId}`);
    await waitForPageNavigationStabilization(session, 'ui_state_root_structure');
    await assertRootStructure(session, 'ui_state_after_navigation');
  }

  console.error(`[driver] ui_state cache_miss instance=${instanceId}`);
  try {
    const recovered = await readStructuredUiStateWithNavigationRecovery(
      'recovery',
      UI_STATE_TIMEOUT_MS
    );
    if (recovered) {
      return recovered;
    }
  } catch (error) {
    throw new Error(
      `structured ui_state recovery failed for instance ${instanceId}: ${error}\nBrowser console tail:\n${consoleTailText(session)}`
    );
  }

  throw new Error(
    `browser UI state unavailable for instance ${instanceId}; primary_observation=driver_push_cache fallback=structured_ui_state heartbeat=${JSON.stringify(
      session.renderHeartbeat
    )}\nBrowser console tail:\n${consoleTailText(session)}`
  );
}

async function waitForUiState(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const timeoutMs = Number(params?.timeout_ms ?? UI_STATE_TIMEOUT_MS);
  const rawAfterVersion = params?.after_version;
  const afterVersion =
    rawAfterVersion == null ? null : Number.isFinite(rawAfterVersion) ? Number(rawAfterVersion) : 0;

  if (afterVersion == null) {
    const snapshot = await uiState({ instance_id: instanceId });
    return {
      snapshot,
      version: session.uiStateVersion ?? 0
    };
  }

  if (!session.uiStateCache || typeof session.uiStateCache !== 'object') {
    await uiState({ instance_id: instanceId });
  }

  const result = await waitForUiStateVersion(session, afterVersion, timeoutMs);
  await ensureUiStateRenderConvergence(session, result.snapshot, 'event_wait');
  return result;
}

async function domSnapshot(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  if (session.domState) {
    return parseSnapshotPayload(domSnapshotFromCache(session));
  }
  let payload;
  try {
    payload = await readDomSnapshot(session.page);
  } catch (error) {
    throw new Error(
      `${error}\nBrowser console tail:\n${consoleTailText(session)}`
    );
  }
  return parseSnapshotPayload(payload);
}

async function clickButton(params) {
  const instanceId = normalizeInstanceId(params);
  const selector = String(params?.selector ?? '').trim();
  const label = String(params?.label ?? '').trim();
  const labelFromSelector = selectorToFallbackLabel(selector);
  const effectiveLabel = label || labelFromSelector;
  const session = getSession(instanceId);
  console.error(`[driver] click_button start instance=${instanceId} selector=${selector || '-'} label=${label || '-'}`);

  if (selector) {
    let selectorError = null;
    try {
      await clickByCssSelector(session.page, selector, session);
      console.error(
        `[driver] click_button done instance=${instanceId} selector=${selector} via=css`
      );
      return { status: 'clicked' };
    } catch (selectorError) {
      console.error(
        `[driver] click_button selector_failed instance=${instanceId} selector=${selector} error=${selectorError?.message ?? String(selectorError)}`
      );
    }

    if (effectiveLabel) {
      const fallbackLabel = normalizeClickTarget(effectiveLabel);
      console.error(
        `[driver] click_button selector failed, trying fallback label instance=${instanceId} selector=${selector} label=${fallbackLabel}`
      );
      try {
        await clickByLabelText(session.page, fallbackLabel, session);
        console.error(
          `[driver] click_button done instance=${instanceId} selector=${selector} via=label fallbackLabel=${fallbackLabel}`
        );
        return { status: 'clicked' };
      } catch (fallbackError) {
        console.error(
          `[driver] click_button fallback_failed instance=${instanceId} selector=${selector} label=${fallbackLabel} error=${fallbackError?.message ?? String(fallbackError)}`
        );
      }
    }

    throw new Error(
      `click_button failed selector=${selector} label=${effectiveLabel || '-'} dom_error=${selectorError?.message ?? 'unknown'}`
    );
  }

  if (!label && !effectiveLabel) {
    throw new Error('label is required');
  }
  const activeLabel = effectiveLabel || label;

  try {
    await clickByLabelText(session.page, activeLabel, session);
  } catch {
    await clickByLabelText(session.page, activeLabel, session);
  }

  console.error(`[driver] click_button done instance=${instanceId} label=${activeLabel}`);

  return { status: 'clicked' };
}

async function fillInput(params) {
  const instanceId = normalizeInstanceId(params);
  const selector = String(params?.selector ?? '').trim();
  const value = String(params?.value ?? '');
  if (!selector) {
    throw new Error('selector is required');
  }
  const session = getSession(instanceId);
  console.error(`[driver] fill_input start instance=${instanceId} selector=${selector}`);
  const locator = session.page.locator(selector).first();
  const domCacheHasSelector = selector.startsWith('#') && domStateHasId(session, selector.slice(1));
  const attachTimeoutMs = domCacheHasSelector ? 2000 : 8000;
  const focusTimeoutMs = domCacheHasSelector ? 1500 : 3000;
  const fillTimeoutMs = domCacheHasSelector ? 2000 : 3000;
  console.error(
    `[driver] fill_input dom_cache instance=${instanceId} selector=${selector} present=${domCacheHasSelector}`
  );
  try {
    await focusAuraPageSafe(session.page, instanceId, `fill_input_start:${selector}`);
    console.error(`[driver] fill_input attach_wait start instance=${instanceId} selector=${selector}`);
    await withOperationTimeout(
      `fill_input_attach:${selector}`,
      locator.waitFor({ state: 'attached', timeout: attachTimeoutMs }),
      attachTimeoutMs + 1000
    );
    console.error(`[driver] fill_input attach_wait done instance=${instanceId} selector=${selector}`);
    console.error(`[driver] fill_input focus start instance=${instanceId} selector=${selector}`);
    await withOperationTimeout(
      `fill_input_focus:${selector}`,
      locator.focus({ timeout: focusTimeoutMs }),
      focusTimeoutMs + 1000
    );
    console.error(`[driver] fill_input focus done instance=${instanceId} selector=${selector}`);
    console.error(`[driver] fill_input playwright_fill start instance=${instanceId} selector=${selector}`);
    await withOperationTimeout(
      `fill_input_fill:${selector}`,
      locator.fill(value, { timeout: fillTimeoutMs }),
      fillTimeoutMs + 2000
    );
    console.error(`[driver] fill_input playwright_fill done instance=${instanceId} selector=${selector}`);
  } catch (error) {
    console.error(
      `[driver] fill_input playwright_path_failed instance=${instanceId} selector=${selector} error=${error?.message ?? String(error)}`
    );
    const fallbackResult =
      selector.startsWith('#') && !domStateHasId(session, selector.slice(1))
        ? { ok: false, reason: 'field_missing_in_dom_cache' }
        : await session.page
            .evaluate(
              ({ targetSelector, nextValue }) => {
                const element = document.querySelector(targetSelector);
                if (!(element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)) {
                  return { ok: false, reason: 'field_not_found' };
                }
                element.focus();
                element.value = nextValue;
                element.dispatchEvent(new Event('input', { bubbles: true }));
                element.dispatchEvent(new Event('change', { bubbles: true }));
                return { ok: true, readOnly: element.readOnly, disabled: element.disabled };
              },
              { targetSelector: selector, nextValue: value }
            )
            .catch(() => ({ ok: false, reason: 'dom_fallback_failed' }));
    if (fallbackResult?.ok) {
      console.error(
        `[driver] fill_input fallback_done instance=${instanceId} selector=${selector} readonly=${fallbackResult.readOnly} disabled=${fallbackResult.disabled}`
      );
      return { status: 'filled', bytes: value.length, fallback: true };
    }

    const diagnostics = {
      ids: domStateIdList(session)
        .filter(
          (id) =>
            id.startsWith('aura-screen-') ||
            id.startsWith('aura-field-') ||
            id.startsWith('aura-chat-')
        )
        .slice(0, 100),
      text: session.domState.text.slice(0, 1200)
    };
    throw new Error(
      `${error.message} fallback=${JSON.stringify(
        fallbackResult
      )} current_ids=${JSON.stringify(diagnostics.ids)} text_snippet=${JSON.stringify(
        diagnostics.text
      )}`
    );
  }
  console.error(`[driver] fill_input done instance=${instanceId} selector=${selector}`);
  return { status: 'filled', bytes: value.length };
}

async function readClipboard(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const text = await session.page.evaluate(() => window.__AURA_HARNESS__.read_clipboard());
  return { text: String(text ?? '') };
}

async function getAuthorityId(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const authorityId = await session.page.evaluate(() => {
    if (typeof window.__AURA_HARNESS__?.get_authority_id === 'function') {
      return window.__AURA_HARNESS__.get_authority_id();
    }
    return null;
  });
  if (authorityId == null) {
    return {};
  }
  return { authority_id: String(authorityId) };
}

async function tailLog(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const lines = Number(params?.lines ?? 20);
  const requested = Number.isFinite(lines) ? Math.max(1, Math.floor(lines)) : 20;

  const harnessLines = await session.page.evaluate((count) => {
    return window.__AURA_HARNESS__.tail_log(count);
  }, requested);

  const merged = [
    ...(Array.isArray(harnessLines) ? harnessLines.map(String) : []),
    ...session.consoleLog
  ].filter((line) => {
    const text = String(line);
    return !(
      text.includes('[driver] request start') ||
      text.includes('[driver] request done') ||
      text.includes('method=ui_state') ||
      text.includes('method=snapshot') ||
      text.includes('method=tail_log')
    );
  });

  return {
    lines: merged.slice(-requested)
  };
}

async function injectMessage(params) {
  const instanceId = normalizeInstanceId(params);
  const message = String(params?.message ?? '');
  const session = getSession(instanceId);

  await session.page.evaluate((value) => {
    if (window.__AURA_HARNESS__?.inject_message) {
      window.__AURA_HARNESS__.inject_message(value);
    }
  }, message);

  return { status: 'injected' };
}

async function stop(params) {
  const instanceId = normalizeInstanceId(params);
  const session = sessions.get(instanceId);
  if (!session) {
    return { status: 'already_stopped' };
  }

  try {
    for (const waiter of session.uiStateWaiters ?? []) {
      clearTimeout(waiter.timer);
      waiter.reject(new Error(`session stopped for ${instanceId}`));
    }
    session.uiStateWaiters = [];
    if (session.tracePath) {
      ensureDir(path.dirname(session.tracePath));
      await session.context.tracing.stop({ path: session.tracePath });
    }
  } catch {
    // Ignore tracing stop errors to preserve stop idempotency.
  }

  await session.context.close();
  sessions.delete(instanceId);

  return {
    status: 'stopped',
    trace_path: session.tracePath
  };
}

async function shutdownAll() {
  const ids = [...sessions.keys()];
  for (const instanceId of ids) {
    try {
      await stop({ instance_id: instanceId });
    } catch {
      // Continue shutdown.
    }
  }
}

async function dispatch(method, params) {
  switch (method) {
    case 'start_page':
      return startPage(params);
    case 'send_keys':
      return sendKeys(params);
    case 'send_key':
      return sendKey(params);
    case 'click_button':
      return clickButton(params);
    case 'fill_input':
      return fillInput(params);
    case 'snapshot':
      return snapshot(params);
    case 'ui_state':
      return uiState(params);
    case 'wait_for_ui_state':
      return waitForUiState(params);
    case 'dom_snapshot':
      return domSnapshot(params);
    case 'wait_for_dom_patterns':
      return waitForDomPatterns(params);
    case 'wait_for_selector':
      return waitForSelector(params);
    case 'read_clipboard':
      return readClipboard(params);
    case 'get_authority_id':
      return getAuthorityId(params);
    case 'tail_log':
      return tailLog(params);
    case 'inject_message':
      return injectMessage(params);
    case 'stop':
      return stop(params);
    default:
      throw new Error(`unsupported method: ${method}`);
  }
}

if (process.argv.includes('--selftest')) {
  try {
    runSelfTest();
    process.exit(0);
  } catch (error) {
    console.error(`[driver] selftest failed: ${error?.stack ?? error?.message ?? String(error)}`);
    process.exit(1);
  }
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity
});

rl.on('line', (line) => {
  requestChain = requestChain
    .then(async () => {
      const raw = line.trim();
      if (!raw) {
        return;
      }

      let request;
      try {
        request = JSON.parse(raw);
      } catch (error) {
        writeResponse(jsonResponse(null, false, `invalid JSON: ${error.message}`));
        return;
      }

      const id = request.id ?? null;
      try {
        console.error(`[driver] request start id=${id} method=${request.method}`);
        const result = await withOperationTimeout(
          `request:${request.method}`,
          dispatch(request.method, request.params ?? {}),
          requestTimeoutMs(request.method, request.params ?? {})
        );
        console.error(`[driver] request done id=${id} method=${request.method}`);
        writeResponse(jsonResponse(id, true, result));
      } catch (error) {
        console.error(`[driver] request failed id=${id} method=${request.method}: ${error?.stack ?? error?.message ?? String(error)}`);
        writeResponse(jsonResponse(id, false, error?.stack ?? error?.message ?? String(error)));
      }
    })
    .catch((error) => {
      writeResponse(jsonResponse(null, false, error?.stack ?? String(error)));
    });
});

rl.on('close', async () => {
  try {
    await shutdownAll();
  } finally {
    process.exit(0);
  }
});

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, async () => {
    try {
      await shutdownAll();
    } finally {
      process.exit(0);
    }
  });
}
