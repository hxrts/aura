#!/usr/bin/env node
// @ts-nocheck

import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import readline from "node:readline";
import type {
  DriverMethod,
  DriverRequest,
  DriverResponse,
  DriverSession,
  DriverSuccessResponse,
  SnapshotResult,
  TailLogResult,
  UiSnapshotPayload,
} from "./contracts.js";
import {
  ACTION_METHODS,
  OBSERVATION_METHODS,
  RECOVERY_METHODS,
} from "./method_sets.js";
import {
  normalizeScreenText,
  normalizeDomState,
  uiSnapshotRenderRevision,
  uiSnapshotRevision,
  uiStateStalenessReason,
} from "./observation.js";

const sessions = new Map<string, DriverSession>();
let requestChain: Promise<void> = Promise.resolve();
let chromiumPromise = null;
const UI_STATE_LOG_PREFIX = "[aura-ui-state]";
const UI_STATE_JSON_LOG_PREFIX = "[aura-ui-json]";
const PLAYWRIGHT_TRACE_ENABLED =
  process.env.AURA_HARNESS_PLAYWRIGHT_TRACE === "1" ||
  process.env.AURA_HARNESS_PLAYWRIGHT_TRACE === "true";
const PLAYWRIGHT_VERBOSE_SUCCESS_LOG_ENABLED =
  process.env.AURA_HARNESS_PLAYWRIGHT_VERBOSE_SUCCESS === "1" ||
  process.env.AURA_HARNESS_PLAYWRIGHT_VERBOSE_SUCCESS === "true";
const DEFAULT_PAGE_GOTO_TIMEOUT_MS = 90000;
const DEFAULT_HARNESS_READY_TIMEOUT_MS = 90000;
const DEFAULT_START_MAX_ATTEMPTS = 3;
const DEFAULT_START_RETRY_BACKOFF_MS = 1200;
const MAX_TIMEOUT_MS = 600000;
const MAX_START_ATTEMPTS = 10;

async function getChromium() {
  if (!chromiumPromise) {
    chromiumPromise = import("playwright").then((module) => module.chromium);
  }
  return chromiumPromise;
}

process.on("uncaughtException", (error) => {
  console.error(
    `[driver] uncaughtException: ${error?.stack ?? error?.message ?? String(error)}`,
  );
  process.exitCode = 1;
});

process.on("unhandledRejection", (reason) => {
  console.error(
    `[driver] unhandledRejection: ${reason?.stack ?? reason?.message ?? String(reason)}`,
  );
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

function resetPersistentProfileDir(dirPath) {
  if (!dirPath) {
    return;
  }
  fs.rmSync(dirPath, { recursive: true, force: true });
  fs.mkdirSync(dirPath, { recursive: true });
}

function jsonResponse(
  id: number | null,
  ok: boolean,
  payload: unknown,
): DriverResponse {
  if (ok) {
    return { id, ok: true, result: payload } as DriverSuccessResponse;
  }
  return { id, ok: false, error: String(payload) };
}

function writeResponse(response: DriverResponse) {
  process.stdout.write(`${JSON.stringify(response)}\n`);
}

function traceDriver(message) {
  if (PLAYWRIGHT_VERBOSE_SUCCESS_LOG_ENABLED) {
    console.error(message);
  }
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

function removeDomStateWaiter(session, waiter) {
  if (!Array.isArray(session.domStateWaiters)) {
    return;
  }
  const index = session.domStateWaiters.indexOf(waiter);
  if (index >= 0) {
    session.domStateWaiters.splice(index, 1);
  }
}

function resolveSemanticResultWaiter(session, payload) {
  const commandId =
    payload && typeof payload === "object" ? payload.command_id : null;
  if (
    typeof commandId !== "string" ||
    !session.semanticResultWaiters ||
    typeof session.semanticResultWaiters !== "object"
  ) {
    return;
  }
  const waiter = session.semanticResultWaiters[commandId];
  if (!waiter) {
    return;
  }
  delete session.semanticResultWaiters[commandId];
  clearTimeout(waiter.timer);
  waiter.resolve(payload);
}

function rejectSemanticResultWaiters(session, reason) {
  if (
    !session.semanticResultWaiters ||
    typeof session.semanticResultWaiters !== "object"
  ) {
    return;
  }
  const waiters = Object.values(session.semanticResultWaiters);
  session.semanticResultWaiters = Object.create(null);
  for (const waiter of waiters) {
    clearTimeout(waiter.timer);
    waiter.reject(new Error(`semantic_result_wait_invalidated:${reason}`));
  }
}

function notifyDomStateWaiters(session) {
  if (!Array.isArray(session.domStateWaiters) || session.domStateWaiters.length === 0) {
    return;
  }
  const waiters = [...session.domStateWaiters];
  for (const waiter of waiters) {
    let matched = false;
    try {
      matched = waiter.predicate(session) === true;
    } catch (error) {
      removeDomStateWaiter(session, waiter);
      clearTimeout(waiter.timer);
      waiter.reject(error instanceof Error ? error : new Error(String(error)));
      continue;
    }
    if (!matched) {
      continue;
    }
    removeDomStateWaiter(session, waiter);
    clearTimeout(waiter.timer);
    waiter.resolve(domSnapshotFromCache(session));
  }
}

function waitForSemanticResult(session, commandId, timeoutMs) {
  if (!session.semanticResultCache || typeof session.semanticResultCache !== "object") {
    session.semanticResultCache = Object.create(null);
  }
  if (!session.semanticResultWaiters || typeof session.semanticResultWaiters !== "object") {
    session.semanticResultWaiters = Object.create(null);
  }
  if (session.semanticResultCache[commandId]) {
    const payload = session.semanticResultCache[commandId];
    delete session.semanticResultCache[commandId];
    return Promise.resolve(payload);
  }
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      if (
        session.semanticResultWaiters &&
        session.semanticResultWaiters[commandId]
      ) {
        delete session.semanticResultWaiters[commandId];
      }
      reject(
        new Error(
          `semantic_result_wait_timeout:${commandId}:${timeoutMs}`,
        ),
      );
    }, timeoutMs);
    session.semanticResultWaiters[commandId] = { resolve, reject, timer };
  });
}

function trySerializeUiState(value) {
  if (!value || typeof value !== "object") {
    return null;
  }
  try {
    return JSON.stringify(value);
  } catch {
    return null;
  }
}

function notifyUiStateWaiters(session) {
  if (
    !session.uiStateCache ||
    !Array.isArray(session.uiStateWaiters) ||
    session.uiStateWaiters.length === 0
  ) {
    return;
  }
  const currentRevision = uiSnapshotRevision(session.uiStateCache);
  const ready = session.uiStateWaiters.filter(
    (waiter) => currentRevision > waiter.afterVersion,
  );
  for (const waiter of ready) {
    removeUiStateWaiter(session, waiter);
    clearTimeout(waiter.timer);
    waiter.resolve({
      snapshot: session.uiStateCache,
      version: currentRevision,
    });
  }
}

function rejectUiStateWaiters(session, reason) {
  if (
    !Array.isArray(session.uiStateWaiters) ||
    session.uiStateWaiters.length === 0
  ) {
    return;
  }
  const waiters = [...session.uiStateWaiters];
  session.uiStateWaiters.length = 0;
  for (const waiter of waiters) {
    clearTimeout(waiter.timer);
    waiter.reject(new Error(`ui_state_wait_invalidated:${reason}`));
  }
}

function rejectDomStateWaiters(session, reason) {
  if (!Array.isArray(session.domStateWaiters) || session.domStateWaiters.length === 0) {
    return;
  }
  const waiters = [...session.domStateWaiters];
  session.domStateWaiters.length = 0;
  for (const waiter of waiters) {
    clearTimeout(waiter.timer);
    waiter.reject(new Error(`dom_state_wait_invalidated:${reason}`));
  }
}

function resetObservationState(session, reason, options = {}) {
  rejectUiStateWaiters(session, reason);
  rejectDomStateWaiters(session, reason);
  rejectSemanticResultWaiters(session, reason);
  session.uiStateCache = null;
  session.uiStateCacheJson = null;
  session.uiStateVersion = 0;
  session.domState = normalizeDomState({ text: "", ids: [] });
  session.renderHeartbeat = null;
  session.requiredUiStateRevision = 0;
  session.lastObservationResetReason = reason;
  session.observationEpoch = (session.observationEpoch ?? 0) + 1;
  if (options.resetClipboard === true) {
    session.clipboardCache = "";
  }
}

function markObservationMutation(session, reason) {
  const currentRevision = uiSnapshotRevision(session.uiStateCache);
  const nextRequiredRevision = Math.max(1, currentRevision + 1);
  session.requiredUiStateRevision = Math.max(
    session.requiredUiStateRevision ?? 0,
    nextRequiredRevision,
  );
  session.lastMutationReason = reason;
}

function clearObservationMutationIfSatisfied(session, snapshot) {
  const requiredRevision = session.requiredUiStateRevision ?? 0;
  if (
    requiredRevision > 0 &&
    uiSnapshotRevision(snapshot) >= requiredRevision
  ) {
    session.requiredUiStateRevision = null;
    session.lastMutationReason = null;
  }
}

function storeUiState(session, payload, source = "unknown") {
  const parsed =
    typeof payload === "string"
      ? (() => {
          try {
            return JSON.parse(payload);
          } catch {
            return null;
          }
        })()
      : payload && typeof payload === "object"
        ? payload
        : null;
  if (!parsed || typeof parsed !== "object") {
    return false;
  }

  const nextJson = trySerializeUiState(parsed);
  const changed = nextJson !== session.uiStateCacheJson;
  session.uiStateCache = parsed;
  session.uiStateCacheJson = nextJson;
  session.lastUiStateSource = source;
  clearObservationMutationIfSatisfied(session, parsed);
  if (changed) {
    session.uiStateVersion = (session.uiStateVersion ?? 0) + 1;
    notifyUiStateWaiters(session);
  }
  return true;
}

function waitForUiStateVersion(session, afterVersion, timeoutMs) {
  const currentRevision = uiSnapshotRevision(session.uiStateCache);
  if (
    session.uiStateCache &&
    typeof session.uiStateCache === "object" &&
    currentRevision > afterVersion
  ) {
    return Promise.resolve({
      snapshot: session.uiStateCache,
      version: currentRevision,
    });
  }

  return new Promise((resolve, reject) => {
    if ((session.uiStateWaiters?.length ?? 0) >= 64) {
      reject(new Error("ui_state_wait_queue_overflow"));
      return;
    }
    const waiter = {
      afterVersion,
      resolve,
      reject,
      timer: null,
    };
    waiter.timer = setTimeout(() => {
      removeUiStateWaiter(session, waiter);
      reject(
        new Error(
          `wait_for_ui_state timed out after ${timeoutMs}ms after_version=${afterVersion} current_revision=${uiSnapshotRevision(
            session.uiStateCache,
          )}`,
        ),
      );
    }, timeoutMs);
    session.uiStateWaiters.push(waiter);
  });
}

function waitForDomState(session, predicate, timeoutMs, label) {
  if (predicate(session) === true) {
    return Promise.resolve(domSnapshotFromCache(session));
  }
  if (!Array.isArray(session.domStateWaiters)) {
    session.domStateWaiters = [];
  }
  return new Promise((resolve, reject) => {
    if (session.domStateWaiters.length >= 64) {
      reject(new Error("dom_state_wait_queue_overflow"));
      return;
    }
    const waiter = {
      predicate,
      resolve,
      reject,
      timer: null,
    };
    waiter.timer = setTimeout(() => {
      removeDomStateWaiter(session, waiter);
      reject(new Error(`${label} timed out after ${timeoutMs}ms`));
    }, timeoutMs);
    session.domStateWaiters.push(waiter);
  });
}

function normalizeInstanceId(params) {
  const instanceId = params?.instance_id;
  if (!instanceId || typeof instanceId !== "string") {
    throw new Error("instance_id is required");
  }
  return instanceId;
}

function isMutatingMethod(method) {
  return ACTION_METHODS.has(method) || RECOVERY_METHODS.has(method);
}

function parseSnapshotPayload(payload): SnapshotResult {
  const fallback = String(payload ?? "");
  if (payload && typeof payload === "object") {
    return {
      screen: String(
        payload.screen ?? payload.authoritative_screen ?? fallback,
      ),
      raw_screen: String(payload.raw_screen ?? payload.screen ?? fallback),
      authoritative_screen: String(
        payload.authoritative_screen ?? payload.screen ?? fallback,
      ),
      normalized_screen: String(
        payload.normalized_screen ?? payload.screen ?? fallback,
      ),
      capture_consistency: String(payload.capture_consistency ?? "settled"),
    };
  }

  return {
    screen: fallback,
    raw_screen: fallback,
    authoritative_screen: fallback,
    normalized_screen: fallback,
    capture_consistency: "settled",
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
        .map((value) => String(value ?? "").trim())
        .filter((value) => value.length > 0),
    );
  }
  return new Set();
}

function domStateHasId(session, id) {
  return domStateIdSet(session).has(String(id ?? "").trim());
}

function domStateIdList(session) {
  return Array.from(domStateIdSet(session));
}

function normalizeRenderHeartbeat(payload) {
  if (!payload || typeof payload !== "object") {
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
    render_seq: renderSeq,
  };
}

const SCREEN_DOM_IDS = Object.freeze({
  onboarding: "aura-onboarding-root",
  neighborhood: "aura-screen-neighborhood",
  chat: "aura-screen-chat",
  contacts: "aura-screen-contacts",
  notifications: "aura-screen-notifications",
  settings: "aura-screen-settings",
});

const MODAL_DOM_IDS = Object.freeze({
  help: "aura-modal-help",
  create_invitation: "aura-modal-create-invitation",
  invitation_code: "aura-modal-invitation-code",
  accept_invitation: "aura-modal-accept-invitation",
  create_home: "aura-modal-create-home",
  create_channel: "aura-modal-create-channel",
  set_channel_topic: "aura-modal-set-channel-topic",
  channel_info: "aura-modal-channel-info",
  edit_nickname: "aura-modal-edit-nickname",
  remove_contact: "aura-modal-remove-contact",
  guardian_setup: "aura-modal-guardian-setup",
  request_recovery: "aura-modal-request-recovery",
  add_device: "aura-modal-add-device",
  import_device_enrollment_code: "aura-modal-import-device-enrollment-code",
  select_device_to_remove: "aura-modal-select-device-to-remove",
  confirm_remove_device: "aura-modal-confirm-remove-device",
  mfa_setup: "aura-modal-mfa-setup",
  assign_moderator: "aura-modal-assign-moderator",
  switch_authority: "aura-modal-switch-authority",
  access_override: "aura-modal-access-override",
  capability_config: "aura-modal-capability-config",
});

function contractEnumKey(value) {
  if (typeof value !== "string") {
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

function domStateIdsByPrefix(session, prefix) {
  return domStateIdList(session).filter((id) => id.startsWith(prefix));
}

function domStateAlignedWithState(session, state) {
  const screenDomId = expectedScreenDomId(state);
  const screenIds = domStateIdsByPrefix(session, "aura-screen-");
  if (screenIds.length > 0 && screenDomId && !screenIds.includes(screenDomId)) {
    return false;
  }

  const modalDomId = expectedModalDomId(state);
  const modalIds = domStateIdsByPrefix(session, "aura-modal-");
  if (modalIds.length > 0) {
    if (modalDomId) {
      if (!modalIds.includes(modalDomId)) {
        return false;
      }
    } else {
      return false;
    }
  }

  return true;
}

async function ensureUiStateRenderConvergence(
  session,
  state,
  reason,
  timeoutMs = 1500,
) {
  const heartbeat = session.renderHeartbeat;
  const domIds = domStateIdList(session);
  if (
    !heartbeat &&
    domIds.length === 0 &&
    !String(session?.domState?.text ?? "").trim()
  ) {
    return;
  }
  if (
    domStateAlignedWithState(session, state) &&
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
        session.page
          .locator(`#${screenDomId}`)
          .first()
          .waitFor({ state: "attached" }),
        timeoutMs,
      );
    } catch (error) {
      throw new Error(
        `semantic screen '${state?.screen ?? "unknown"}' did not converge to DOM id #${screenDomId}: ${
          error?.message ?? String(error)
        } current_ids=${JSON.stringify(domIds)} text_snippet=${JSON.stringify(
          session?.domState?.text ?? "",
        )}`,
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
        session.page
          .locator(`#${modalDomId}`)
          .first()
          .waitFor({ state: "attached" }),
        timeoutMs,
      );
    } catch (error) {
      throw new Error(
        `semantic modal '${state?.open_modal ?? "unknown"}' did not converge to DOM id #${modalDomId}: ${
          error?.message ?? String(error)
        } current_ids=${JSON.stringify(domStateIdList(session))} text_snippet=${JSON.stringify(
          session?.domState?.text ?? "",
        )}`,
      );
    }
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

function runSelfTest() {
  const chatState = { screen: "chat", open_modal: null };
  const modalState = {
    screen: "neighborhood",
    open_modal: "accept_invitation",
  };
  const heartbeatSession = {
    domState: normalizeDomState({ text: "", ids: [] }),
    renderHeartbeat: normalizeRenderHeartbeat({
      screen: "chat",
      open_modal: null,
      render_seq: 4,
    }),
  };
  const staleRevisionSession = {
    requiredUiStateRevision: 7,
    renderHeartbeat: normalizeRenderHeartbeat({
      screen: "chat",
      open_modal: null,
      render_seq: 9,
    }),
  };
  const mutableSession = {
    uiStateCache: {
      screen: "chat",
      revision: { semantic_seq: 3, render_seq: 3 },
    },
    uiStateWaiters: [],
  };
  markObservationMutation(mutableSession, "click_button");

  assert(
    expectedScreenDomId(chatState) === "aura-screen-chat",
    "chat screen id mapping failed",
  );
  assert(
    expectedModalDomId(modalState) === "aura-modal-accept-invitation",
    "accept invitation modal id mapping failed",
  );
  assert(
    uiStateStalenessReason(staleRevisionSession, {
      screen: "chat",
      revision: { semantic_seq: 6, render_seq: 8 },
    }) === "required_revision_not_reached:7",
    "required revision floor should reject stale snapshot",
  );
  assert(
    uiStateStalenessReason(staleRevisionSession, {
      screen: "chat",
      revision: { semantic_seq: 7, render_seq: 8 },
    }) === "heartbeat_ahead:9:8",
    "heartbeat ahead should reject stale render snapshot",
  );
  assert(
    uiStateStalenessReason({}, null) === "missing_snapshot",
    "default observation must fail diagnostically before any recovery path runs",
  );
  assert(
    String(readStructuredUiState).includes("__AURA_UI_PUBLICATION_STATE__") &&
      String(readStructuredUiState).includes(
        "__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__",
      ),
    "structured observation must surface explicit browser publication diagnostics",
  );
  assert(
    String(readStructuredUiState).includes("ui_state_publication_unavailable:"),
    "structured observation must fail closed with explicit publication-state detail",
  );
  assert(
    OBSERVATION_METHODS.has("ui_state") &&
      !ACTION_METHODS.has("ui_state") &&
      !RECOVERY_METHODS.has("ui_state"),
    "ui_state must remain an observation-only method",
  );
  assert(
    RECOVERY_METHODS.has("recover_ui_state") &&
      !OBSERVATION_METHODS.has("recover_ui_state"),
    "recover_ui_state must remain an explicit recovery-only method",
  );
  assert(
    !String(uiState).includes("readStructuredUiStateWithNavigationRecovery") &&
      !String(uiState).includes("resetObservationState("),
    "ui_state must not perform implicit recovery",
  );
  assert(
    String(recoverUiState).includes(
      "readStructuredUiStateWithNavigationRecovery",
    ),
    "explicit browser recovery path must use the dedicated recovery helper",
  );
  assert(
    mutableSession.requiredUiStateRevision === 4,
    "mutating actions should require a newer semantic revision",
  );
  storeUiState(
    mutableSession,
    {
      screen: "chat",
      revision: { semantic_seq: 4, render_seq: 4 },
    },
    "selftest",
  );
  assert(
    mutableSession.requiredUiStateRevision == null,
    "fresh snapshot should clear mutation floor",
  );
  const staleAfterMutationSession = {
    uiStateCache: {
      screen: "chat",
      revision: { semantic_seq: 11, render_seq: 11 },
    },
    renderHeartbeat: normalizeRenderHeartbeat({
      screen: "chat",
      open_modal: null,
      render_seq: 11,
    }),
    requiredUiStateRevision: null,
    uiStateWaiters: [],
  };
  markObservationMutation(staleAfterMutationSession, "submit_form");
  assert(
    uiStateStalenessReason(staleAfterMutationSession, {
      screen: "chat",
      revision: { semantic_seq: 11, render_seq: 11 },
    }) === "required_revision_not_reached:12",
    "post-action polling must reject a snapshot that is not newer than the pre-action baseline",
  );
  console.error("[driver] selftest ok");
}

function consoleTailText(session, lines = 40) {
  const tail = session.consoleLog.slice(-lines);
  return tail.length > 0 ? tail.join("\n") : "none";
}

async function ensureHarnessWithTimeout(page, timeoutMs) {
  await page.waitForFunction(
    () => {
      const bridge = window.__AURA_HARNESS__;
      const observe = window.__AURA_HARNESS_OBSERVE__;
      return bridge && observe && typeof observe.snapshot === "function";
    },
    null,
    { timeout: timeoutMs },
  );
}

async function ensurePageInteractive(page, timeoutMs) {
  await page.waitForFunction(
    () => {
      const title = document.title || "";
      const bodyText = document.body?.innerText || "";
      const buildScreenVisible =
        title.includes("Dioxus Build") ||
        bodyText.includes("We're building your app now") ||
        bodyText.includes("Starting the build...");
      const mainRoot = document.getElementById("main");
      return !buildScreenVisible && !!mainRoot;
    },
    null,
    { timeout: timeoutMs },
  );
}

async function installDomObserver(page, session) {
  await page.exposeBinding("__AURA_DRIVER_PUSH_STATE", (_source, payload) => {
    session.domState = normalizeDomState(payload);
    notifyDomStateWaiters(session);
  });
  await page.evaluate(() => {
    const pushState = () => {
      const root =
        document.getElementById("aura-app-root") ??
        document.querySelector("main:last-of-type") ??
        document.body;
      const ids = Array.from(document.querySelectorAll("[id]"))
        .map((element) => element.id)
        .filter((id) => id.startsWith("aura-"));
      return window.__AURA_DRIVER_PUSH_STATE({
        text: root?.textContent ?? "",
        ids,
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
      attributeFilter: ["id", "class", "aria-hidden", "open", "data-state"],
    });

    window.addEventListener("load", schedulePush, { once: true });
    window.__AURA_DRIVER_OBSERVER_INSTALLED = true;
    schedulePush();
  });
}

async function installUiStateObserver(page, session) {
  if (!session.uiStateBindingsInstalled) {
    await page.exposeFunction("__AURA_DRIVER_PUSH_UI_STATE", (payload) => {
      storeUiState(session, payload);
    });
    await page.exposeFunction(
      "__AURA_DRIVER_PUSH_RENDER_HEARTBEAT",
      (payload) => {
        if (typeof payload === "string") {
          try {
            session.renderHeartbeat = normalizeRenderHeartbeat(
              JSON.parse(payload),
            );
          } catch {
            session.renderHeartbeat = null;
          }
          return;
        }
        session.renderHeartbeat = normalizeRenderHeartbeat(payload);
      },
    );
    await page.exposeFunction("__AURA_DRIVER_PUSH_CLIPBOARD", (payload) => {
      session.clipboardCache = String(payload ?? "");
    });
    session.uiStateBindingsInstalled = true;
  }
}

async function installHarnessMutationQueue(page, session) {
  await page.exposeFunction("__AURA_DRIVER_PUSH_SEMANTIC_RESULT", (payload) => {
    if (!session.semanticResultCache || typeof session.semanticResultCache !== "object") {
      session.semanticResultCache = Object.create(null);
    }
    if (payload && typeof payload === "object" && typeof payload.command_id === "string") {
      session.semanticResultCache[payload.command_id] = payload;
      resolveSemanticResultWaiter(session, payload);
    }
  });
  await page.evaluate(() => {
    if (window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED) {
      return;
    }

    window.__AURA_DRIVER_PENDING_NAV_SCREEN__ = null;
    window.__AURA_DRIVER_SEMANTIC_QUEUE__ = [];
    window.__AURA_DRIVER_SEMANTIC_RESULTS__ = Object.create(null);
    window.__AURA_DRIVER_SEMANTIC_BUSY__ = false;
    window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ = false;
    window.__AURA_DRIVER_SEMANTIC_DEBUG__ = {
      last_event: "installed",
      active_command_id: null,
      last_error: null,
      queue_depth: 0,
      last_progress_at: Date.now(),
    };
    window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__ = () => {
      const harness = window.__AURA_HARNESS__;
      const semanticQueue = window.__AURA_DRIVER_SEMANTIC_QUEUE__;
      const semanticResults = window.__AURA_DRIVER_SEMANTIC_RESULTS__;
      const semanticDebug = window.__AURA_DRIVER_SEMANTIC_DEBUG__;
      if (semanticDebug) {
        semanticDebug.queue_depth = Array.isArray(semanticQueue)
          ? semanticQueue.length
          : 0;
      }
      if (
        window.__AURA_DRIVER_SEMANTIC_BUSY__ ||
        !Array.isArray(semanticQueue) ||
        semanticQueue.length === 0
      ) {
        return;
      }
      const nextJson = semanticQueue.shift();
      if (typeof nextJson !== "string" || nextJson.length === 0) {
        queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
        return;
      }
      let next = null;
      try {
        next = JSON.parse(nextJson);
      } catch (error) {
        if (semanticDebug) {
          semanticDebug.last_event = "queue_parse_error";
          semanticDebug.last_error = error?.message ?? String(error);
          semanticDebug.active_command_id = null;
          semanticDebug.last_progress_at = Date.now();
        }
        queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
        return;
      }
      if (!next || typeof next.command_id !== "string") {
        queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
        return;
      }
      if (semanticDebug) {
        semanticDebug.last_event = "queue_start";
        semanticDebug.active_command_id = next.command_id;
        semanticDebug.last_error = null;
        semanticDebug.last_progress_at = Date.now();
      }
      if (
        !harness ||
        typeof harness.submit_semantic_command !== "function"
      ) {
        semanticResults[next.command_id] = {
          ok: false,
          error:
            "window.__AURA_HARNESS__.submit_semantic_command is unavailable",
        };
        if (semanticDebug) {
          semanticDebug.last_event = "queue_missing_bridge";
          semanticDebug.last_error =
            "window.__AURA_HARNESS__.submit_semantic_command is unavailable";
          semanticDebug.active_command_id = null;
          semanticDebug.last_progress_at = Date.now();
        }
        queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
        return;
      }
      window.__AURA_DRIVER_SEMANTIC_BUSY__ = true;
      Promise.resolve()
        .then(() => {
          console.log(
            `[driver-page] semantic queue start id=${next.command_id}`,
          );
          const requestObject = JSON.parse(next.request_json);
          if (semanticDebug) {
            semanticDebug.last_event = "queue_invoke";
            semanticDebug.last_progress_at = Date.now();
          }
          return harness.submit_semantic_command(requestObject);
        })
        .then((result) => {
          console.log(
            `[driver-page] semantic queue ok id=${next.command_id}`,
          );
          semanticResults[next.command_id] = { ok: true, result };
          Promise.resolve(
            window.__AURA_DRIVER_PUSH_SEMANTIC_RESULT?.({
              command_id: next.command_id,
              ok: true,
              result,
            }),
          ).catch(() => {});
          if (semanticDebug) {
            semanticDebug.last_event = "queue_ok";
            semanticDebug.active_command_id = null;
            semanticDebug.last_progress_at = Date.now();
          }
        })
        .catch((error) => {
          console.error(
            `[driver-page] semantic queue error id=${next.command_id}: ${error?.message ?? String(error)}`,
          );
          semanticResults[next.command_id] = {
            ok: false,
            error: error?.message ?? String(error),
          };
          Promise.resolve(
            window.__AURA_DRIVER_PUSH_SEMANTIC_RESULT?.({
              command_id: next.command_id,
              ok: false,
              error: error?.message ?? String(error),
            }),
          ).catch(() => {});
          if (semanticDebug) {
            semanticDebug.last_event = "queue_error";
            semanticDebug.last_error = error?.message ?? String(error);
            semanticDebug.active_command_id = null;
            semanticDebug.last_progress_at = Date.now();
          }
        })
        .finally(() => {
          window.__AURA_DRIVER_SEMANTIC_BUSY__ = false;
          if (semanticDebug) {
            semanticDebug.queue_depth = Array.isArray(semanticQueue)
              ? semanticQueue.length
              : 0;
          }
          queueMicrotask(window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__);
        });
    };

    window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__ = () => {
      if (window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__) {
        return;
      }
      window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ = true;
      setTimeout(() => {
        window.__AURA_DRIVER_SEMANTIC_WAKE_SCHEDULED__ = false;
        window.__AURA_DRIVER_RUN_SEMANTIC_QUEUE__?.();
      }, 0);
    };

    const enqueueSemanticCommand = (payloadJson) => {
      if (typeof payloadJson !== "string") {
        return {
          queue_depth: Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)
            ? window.__AURA_DRIVER_SEMANTIC_QUEUE__.length
            : null,
          debug: window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? null,
        };
      }
      if (!Array.isArray(window.__AURA_DRIVER_SEMANTIC_QUEUE__)) {
        throw new Error("window.__AURA_DRIVER_SEMANTIC_QUEUE__ is unavailable");
      }
      window.__AURA_DRIVER_SEMANTIC_QUEUE__.push(payloadJson);
      if (window.__AURA_DRIVER_SEMANTIC_DEBUG__) {
        window.__AURA_DRIVER_SEMANTIC_DEBUG__.last_event = "enqueued";
        window.__AURA_DRIVER_SEMANTIC_DEBUG__.active_command_id = null;
        window.__AURA_DRIVER_SEMANTIC_DEBUG__.queue_depth =
          window.__AURA_DRIVER_SEMANTIC_QUEUE__.length;
        window.__AURA_DRIVER_SEMANTIC_DEBUG__.last_progress_at = Date.now();
      }
      window.__AURA_DRIVER_WAKE_SEMANTIC_QUEUE__?.();
      return {
        queue_depth: window.__AURA_DRIVER_SEMANTIC_QUEUE__.length,
        debug: window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? null,
      };
    };

    window.__AURA_DRIVER_SEMANTIC_ENQUEUE__ = enqueueSemanticCommand;

    const runPendingNav = () => {
      const pendingNav = window.__AURA_DRIVER_PENDING_NAV_SCREEN__;
      const harness = window.__AURA_HARNESS__;
      if (
        pendingNav &&
        harness &&
        typeof harness.navigate_screen === "function"
      ) {
        window.__AURA_DRIVER_PENDING_NAV_SCREEN__ = null;
        try {
          harness.navigate_screen(pendingNav);
        } catch {
          window.__AURA_DRIVER_PENDING_NAV_SCREEN__ = pendingNav;
          window.setTimeout(() => {
            window.__AURA_DRIVER_RUN_PENDING_NAV__?.();
          }, 16);
        }
      }
    };

    window.__AURA_DRIVER_RUN_PENDING_NAV__ = runPendingNav;
    window.__AURA_DRIVER_WAKE_PENDING_NAV__ = () => {
      window.setTimeout(() => {
        window.__AURA_DRIVER_RUN_PENDING_NAV__?.();
      }, 0);
    };

    window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED = true;
  });
}

function installPageNavigationReset(session) {
  const onNavigation = () => {
    resetObservationState(session, "frame_navigation");
    console.error(`[driver] navigation_cache_clear instance=${session.id}`);
  };
  session.page.on("framenavigated", onNavigation);
  session.page.on("domcontentloaded", onNavigation);
  session.page.on("load", onNavigation);
}

async function assertRootStructure(session, reason) {
  const deadlineMs = Date.now() + 2000;
  let lastError = null;

  while (Date.now() < deadlineMs) {
    try {
      const structure = await withOperationTimeout(
        `root_structure_${reason}`,
        session.page.evaluate(() => {
          if (
            typeof window.__AURA_HARNESS_OBSERVE__?.root_structure === "function"
          ) {
            return window.__AURA_HARNESS_OBSERVE__.root_structure();
          }

          const document = window.document;
          if (!document) {
            return null;
          }

          const screenIds = [
            "aura-screen-onboarding",
            "aura-screen-neighborhood",
            "aura-screen-chat",
            "aura-screen-contacts",
            "aura-screen-notifications",
            "aura-screen-settings",
          ];
          const presentScreenIds = screenIds.filter((id) =>
            document.getElementById(id),
          );

          return {
            screen: presentScreenIds[0]?.replace("aura-screen-", "") ?? null,
            app_root_count: document.getElementById("aura-app-root") ? 1 : 0,
            modal_region_count: document.getElementById("aura-modal-region")
              ? 1
              : 0,
            onboarding_root_count: document.getElementById(
              "aura-onboarding-root",
            )
              ? 1
              : 0,
            toast_region_count: document.getElementById("aura-toast-region")
              ? 1
              : 0,
            active_screen_root_count: presentScreenIds.length,
          };
        }),
        2000,
      );

      if (!structure || typeof structure !== "object") {
        lastError = new Error(`root structure export unavailable during ${reason}`);
      } else {
        const appRootCount = Number(structure.app_root_count ?? 0);
        const modalRegionCount = Number(structure.modal_region_count ?? 0);
        const onboardingRootCount = Number(structure.onboarding_root_count ?? 0);
        const toastRegionCount = Number(structure.toast_region_count ?? 0);
        const activeScreenRootCount = Number(structure.active_screen_root_count ?? 0);
        const onboardingShell =
          onboardingRootCount === 1 &&
          appRootCount === 0 &&
          modalRegionCount === 0 &&
          toastRegionCount === 0 &&
          activeScreenRootCount === 0;
        const appShell =
          onboardingRootCount === 0 &&
          appRootCount === 1 &&
          modalRegionCount === 1 &&
          toastRegionCount === 1 &&
          activeScreenRootCount === 1;
        if (onboardingShell || appShell) {
          return;
        }
        lastError = new Error(
          `invalid root structure during ${reason}: ${JSON.stringify(structure)}`,
        );
      }
    } catch (error) {
      if (isNavigationTransitionError(error)) {
        throw error;
      }
      lastError = error;
    }
    try {
      await waitForDomState(
        session,
        (activeSession) => domStateIdList(activeSession).length > 0,
        Math.max(1, deadlineMs - Date.now()),
        `root_structure_${reason}`,
      );
    } catch {
      await delay(25);
    }
  }

  throw (
    lastError ?? new Error(`root structure export unavailable during ${reason}`)
  );
}

function isNavigationTransitionError(error) {
  const message = String(error?.message ?? error ?? "");
  return (
    message.includes("Execution context was destroyed") ||
    message.includes("most likely because of a navigation") ||
    message.includes("Target page, context or browser has been closed")
  );
}

function isUiStateWaitInvalidation(error) {
  const message = String(error?.message ?? error ?? "");
  return message.startsWith("ui_state_wait_invalidated:");
}

function isUiStateRecoveryCandidate(error) {
  const message = String(error?.message ?? error ?? "");
  return (
    isUiStateWaitInvalidation(error) ||
    isNavigationTransitionError(error) ||
    message.includes("root structure export unavailable during ui_state") ||
    (message.includes("root_structure_") && message.includes("timed out"))
  );
}

async function waitForPageNavigationStabilization(session, reason) {
  traceDriver(
    `[driver] navigation_wait start instance=${session.id} reason=${reason}`,
  );
  try {
    await withOperationTimeout(
      `navigation_wait_load_${reason}`,
      session.page.waitForLoadState("load", { timeout: 5000 }),
      6000,
    );
  } catch {}
  try {
    await withOperationTimeout(
      `navigation_wait_domcontentloaded_${reason}`,
      session.page.waitForLoadState("domcontentloaded", { timeout: 5000 }),
      6000,
    );
  } catch {}
  await waitForDomState(
    session,
    (activeSession) => domStateIdList(activeSession).length > 0,
    300,
    `navigation_dom_state_${reason}`,
  ).catch(() => {});
  traceDriver(
    `[driver] navigation_wait done instance=${session.id} reason=${reason}`,
  );
}

async function focusAuraPage(page) {
  try {
    await withOperationTimeout(
      "focus_page_bring_to_front",
      page.bringToFront(),
      1500,
    );
  } catch (error) {
    console.error(
      `[driver] focus_page_bring_to_front_skipped reason=${normalizeClickError(error)}`,
    );
  }
  return true;
}

async function focusAuraPageSafe(page, instanceId, context) {
  try {
    return await focusAuraPage(page);
  } catch (error) {
    console.error(
      `[driver] focus_page skipped instance=${instanceId ?? "unknown"} context=${context} reason=${normalizeClickError(
        error,
      )}`,
    );
    return false;
  }
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function normalizeClickError(error) {
  return error?.message || String(error || "unknown");
}

async function dispatchHarnessKey(page, rawKey, repeat = 1) {
  return page.evaluate(
    ({ key, repeatCount }) => {
      const harness = window.__AURA_HARNESS__;
      if (!harness) {
        return { ok: false, reason: "harness_missing" };
      }
      if (key.length === 1 && typeof harness.send_keys === "function") {
        for (let index = 0; index < repeatCount; index += 1) {
          harness.send_keys(key);
        }
        return { ok: true, mode: "send_keys" };
      }
      if (typeof harness.send_key === "function") {
        harness.send_key(key, repeatCount);
        return { ok: true, mode: "send_key" };
      }
      return { ok: false, reason: "harness_send_key_missing" };
    },
    { key: rawKey, repeatCount: repeat },
  );
}

async function dispatchHarnessKeysText(page, text) {
  return page.evaluate((value) => {
    const harness = window.__AURA_HARNESS__;
    if (!harness || typeof harness.send_keys !== "function") {
      return { ok: false, reason: "harness_send_keys_missing" };
    }
    harness.send_keys(value);
    return { ok: true, mode: "send_keys" };
  }, text);
}

async function clickLocatorWithDiagnostics(locator, context) {
  const actionTimeoutMs = 2800;
  const result = await locator.evaluate((element) => {
    if (!(element instanceof HTMLElement)) {
      return { ok: false, reason: "not_html_element" };
    }
    const style = window.getComputedStyle(element);
    if (style.display === "none" || style.visibility === "hidden") {
      return { ok: false, reason: "not_visible" };
    }
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) {
      return { ok: false, reason: "zero_size" };
    }
    if (
      element.hasAttribute("disabled") ||
      element.getAttribute("aria-disabled") === "true"
    ) {
      return { ok: false, reason: "disabled" };
    }
    return {
      ok: true,
      id: element.id || null,
      text: String(element.textContent ?? "")
        .replace(/\s+/g, " ")
        .trim(),
    };
  });
  if (!result || result.ok !== true) {
    throw new Error(
      `${context} precheck_failed ${JSON.stringify(result ?? {})}`,
    );
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
        noWaitAfter: true,
      }),
      actionTimeoutMs + 200,
    );
  } catch (error) {
    throw error;
  }

  return result;
}

async function clickByCssSelector(page, selector, session) {
  const normalizedSelector = String(selector || "").trim();
  const maxAttempts = 2;
  let lastError = null;

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    const attemptContext = `css:${normalizedSelector}:attempt${attempt}`;
    try {
      const locator = page.locator(normalizedSelector).first();

      await withOperationTimeout(
        `click_wait_${attemptContext}`,
        locator.waitFor({ state: "attached", timeout: 600 }),
        800,
      );

      return await clickLocatorWithDiagnostics(locator, attemptContext);
    } catch (error) {
      lastError = error;
      const message = normalizeClickError(error);
      console.error(
        `[driver] click_button css attempt_failed instance=${session.id} selector=${normalizedSelector} attempt=${attempt} error=${message}`,
      );
      if (isNavigationTransitionError(error)) {
        await waitForPageNavigationStabilization(session, attemptContext);
      }
      if (attempt + 1 < maxAttempts) {
        await waitForDomState(
          session,
          (activeSession) => domStateIdList(activeSession).length > 0,
          80,
          `css_click_retry_${normalizedSelector}`,
        ).catch(() => {});
        continue;
      }
    }
  }

  throw new Error(
    `css_click_retries_exhausted selector=${normalizedSelector} ${normalizeClickError(lastError)}`,
  );
}

async function clickByLabelText(page, label, session) {
  const normalizedLabel = normalizeClickTarget(label);
  const candidates = [
    page.getByRole("button", { name: normalizedLabel, exact: true }),
    page.getByRole("link", { name: normalizedLabel, exact: true }),
    page.locator('button, a, [role="button"], [role="link"]').filter({
      hasText: new RegExp(`^${escapeRegex(normalizedLabel)}$`, "i"),
    }),
  ];
  const maxAttempts = 2;
  let lastError = null;

  for (let attempt = 0; attempt < maxAttempts; attempt += 1) {
    for (const candidate of candidates) {
      const context = `label:${normalizedLabel}:attempt${attempt}`;
      try {
        await withOperationTimeout(
          `click_label_wait_${context}`,
          candidate.first().waitFor({ state: "visible", timeout: 900 }),
          1200,
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
      await waitForDomState(
        session,
        (activeSession) => domStateIdList(activeSession).length > 0,
        175,
        `label_click_retry_${normalizedLabel}`,
      ).catch(() => {});
    }
  }

  throw new Error(
    `label_click_failed label=${normalizedLabel} ${normalizeClickError(lastError)}`,
  );
}

function isNavigationSelector(selector) {
  return String(selector ?? "")
    .trim()
    .startsWith("#aura-nav-");
}

function mapPlaywrightKey(key) {
  if (key.length === 1) {
    return key;
  }

  switch (
    String(key ?? "")
      .trim()
      .toLowerCase()
  ) {
    case "enter":
      return "Enter";
    case "esc":
    case "escape":
      return "Escape";
    case "tab":
      return "Tab";
    case "backtab":
      return "Shift+Tab";
    case "up":
      return "ArrowUp";
    case "down":
      return "ArrowDown";
    case "left":
      return "ArrowLeft";
    case "right":
      return "ArrowRight";
    case "home":
      return "Home";
    case "end":
      return "End";
    case "pageup":
      return "PageUp";
    case "pagedown":
      return "PageDown";
    case "backspace":
      return "Backspace";
    case "delete":
      return "Delete";
    default:
      throw new Error(`unsupported key: ${key}`);
  }
}

async function pressMappedKey(page, key) {
  const mapped = mapPlaywrightKey(key);
  const actionTimeoutMs = 1200;
  console.error(`[driver] key_press start key=${key} mapped=${mapped}`);
  try {
    await withOperationTimeout(
      `key_press:${mapped}`,
      page.keyboard.press(mapped),
      actionTimeoutMs,
    );
    console.error(`[driver] key_press done key=${key} mapped=${mapped}`);
    return;
  } catch (error) {
    console.error(
      `[driver] key_press_failed key=${key} mapped=${mapped} error=${normalizeClickError(error)}`,
    );
  }

  const fallback = await withOperationTimeout(
    `key_press_fallback_${mapped}`,
    dispatchHarnessKey(page, key, 1),
    700,
  );
  if (!fallback?.ok) {
    throw new Error(
      `keyboard press failed for ${mapped}: harness=${JSON.stringify(fallback)}`,
    );
  }
  console.error(`[driver] key_press done key=${key} mapped=${mapped}`);
}

async function flushTypedBuffer(page, buffer) {
  if (!buffer) {
    return "";
  }
  const actionTimeoutMs = 5000;
  const preview = JSON.stringify(
    buffer.length > 80 ? `${buffer.slice(0, 80)}…` : buffer,
  );
  console.error(
    `[driver] key_type start bytes=${buffer.length} preview=${preview}`,
  );
  for (const ch of buffer) {
    const mapped = ch === " " ? "Space" : ch;
    await withOperationTimeout(
      `keyboard_type:${JSON.stringify(mapped)}`,
      page.keyboard.press(mapped),
      actionTimeoutMs,
    );
  }
  console.error(`[driver] key_type done bytes=${buffer.length}`);
  return "";
}

function decodeEscapeSequence(value, startIndex) {
  if (value[startIndex] !== "\u001b") {
    return null;
  }
  const next = value[startIndex + 1];
  if (next !== "[") {
    return { consumed: 1, key: "esc" };
  }
  let cursor = startIndex + 2;
  let body = "";
  while (cursor < value.length) {
    const ch = value[cursor];
    body += ch;
    if ((ch >= "A" && ch <= "Z") || ch === "~") {
      break;
    }
    cursor += 1;
  }

  switch (body) {
    case "A":
      return { consumed: 3, key: "up" };
    case "B":
      return { consumed: 3, key: "down" };
    case "C":
      return { consumed: 3, key: "right" };
    case "D":
      return { consumed: 3, key: "left" };
    case "H":
      return { consumed: 3, key: "home" };
    case "F":
      return { consumed: 3, key: "end" };
    case "Z":
      return { consumed: 3, key: "backtab" };
    case "5~":
      return { consumed: 4, key: "pageup" };
    case "6~":
      return { consumed: 4, key: "pagedown" };
    case "3~":
      return { consumed: 4, key: "delete" };
    default:
      return { consumed: 1, key: "esc" };
  }
}

async function typeKeyStream(page, rawKeys) {
  const value = String(rawKeys ?? "");
  let buffer = "";

  for (let index = 0; index < value.length; index += 1) {
    const ch = value[index];
    if (ch === "\r") {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, "enter");
      if (value[index + 1] === "\n") {
        index += 1;
      }
      continue;
    }
    if (ch === "\n") {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, "enter");
      continue;
    }
    if (ch === "\t") {
      buffer = await flushTypedBuffer(page, buffer);
      await pressMappedKey(page, "tab");
      continue;
    }
    if (ch === "\u001b") {
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
    "page_liveness_probe",
    page.evaluate(() => {
      const active = document.activeElement;
      return {
        title: document.title ?? "",
        readyState: document.readyState ?? "",
        visibilityState: document.visibilityState ?? "",
        hasFocus:
          typeof document.hasFocus === "function" ? document.hasFocus() : false,
        activeTag: active?.tagName ?? null,
        activeId: active?.id ?? null,
        activeClass: active?.className ?? null,
      };
    }),
    3000,
  );
}

async function readDomSnapshot(page) {
  return withOperationTimeout(
    "dom_snapshot",
    page.evaluate(() => {
      const root =
        document.getElementById("aura-app-root") ??
        document.querySelector("main:last-of-type") ??
        document.body;
      const text = root?.textContent ?? "";
      return {
        screen: text,
        raw_screen: text,
        authoritative_screen: text,
        normalized_screen: text,
        capture_consistency: "settled",
      };
    }),
    15000,
  ).then((payload) => ({
    ...payload,
    screen: normalizeScreenText(payload.screen),
    raw_screen: normalizeScreenText(payload.raw_screen),
    authoritative_screen: normalizeScreenText(payload.authoritative_screen),
    normalized_screen: normalizeScreenText(payload.normalized_screen),
  }));
}

function domSnapshotFromCache(session) {
  const text = session.domState?.text ?? "";
  return {
    screen: text,
    raw_screen: text,
    authoritative_screen: text,
    normalized_screen: text,
    capture_consistency: "settled",
  };
}

async function waitForDomPatterns(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const patterns = Array.isArray(params?.patterns)
    ? params.patterns
        .map((value) => normalizeScreenText(String(value)))
        .filter(Boolean)
    : [];
  if (patterns.length === 0) {
    throw new Error("patterns is required");
  }
  const timeoutMs = Number(params?.timeout_ms ?? 30000);
  let lastText = "";
  if (session.domState) {
    const text = session.domState?.text ?? "";
    lastText = text;
    if (patterns.some((pattern) => text.includes(pattern))) {
      return parseSnapshotPayload(domSnapshotFromCache(session));
    }
    traceDriver(
      `[driver] wait_for_dom_patterns cache_miss instance=${instanceId} patterns=${JSON.stringify(patterns)}; falling back to playwright`,
    );
    try {
      const snapshot = await waitForDomState(
        session,
        (activeSession) => {
          const currentText = activeSession.domState?.text ?? "";
          lastText = currentText || lastText;
          return patterns.some((pattern) => currentText.includes(pattern));
        },
        timeoutMs,
        "wait_for_dom_patterns",
      );
      return parseSnapshotPayload(snapshot);
    } catch {
      // Fall through to one final structured DOM read before failing.
    }
  }

  try {
    const snapshot = await withOperationTimeout(
      "wait_for_dom_patterns_snapshot_final",
      readDomSnapshot(session.page),
      2000,
    );
    const text = normalizeScreenText(
      snapshot?.authoritative_screen ?? snapshot?.screen ?? "",
    );
    lastText = text || lastText;
    if (patterns.some((pattern) => text.includes(pattern))) {
      return parseSnapshotPayload(snapshot);
    }
  } catch (error) {
    lastText = `${lastText}\n[dom-read-error] ${error.message}`.trim();
  }
  throw new Error(
    `wait_for_dom_patterns timed out after ${timeoutMs}ms patterns=${JSON.stringify(
      patterns,
    )} text_snippet=${JSON.stringify(lastText.slice(0, 1600))} console_tail=${JSON.stringify(
      consoleTailText(session),
    )}`,
  );
}

async function waitForSelector(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const selector = String(params?.selector ?? "").trim();
  if (!selector) {
    throw new Error("selector is required");
  }
  const timeoutMs = Number(params?.timeout_ms ?? 30000);
  traceDriver(
    `[driver] wait_for_selector start instance=${instanceId} selector=${selector} cache=${selector.startsWith("#") && !!session.domState}`,
  );
  if (
    selector.startsWith("#") &&
    session.domState?.ids?.has(selector.slice(1))
  ) {
    traceDriver(
      `[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=cache`,
    );
    return parseSnapshotPayload(domSnapshotFromCache(session));
  }
  if (selector.startsWith("#") && session.domState) {
    traceDriver(
      `[driver] wait_for_selector cache_miss instance=${instanceId} selector=${selector}; falling back to playwright`,
    );
    try {
      const snapshot = await waitForDomState(
        session,
        (activeSession) => activeSession.domState?.ids?.has(selector.slice(1)),
        timeoutMs,
        `wait_for_selector:${selector}`,
      );
      traceDriver(
        `[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=cache_waiter`,
      );
      return parseSnapshotPayload(snapshot);
    } catch {
      // Fall through to Playwright locator diagnostics.
    }
  }
  try {
    await withOperationTimeout(
      `wait_for_selector:${selector}`,
      session.page
        .locator(selector)
        .first()
        .waitFor({ state: "visible", timeout: timeoutMs }),
      timeoutMs + 1000,
    );
  } catch (error) {
    const diagnostics = await session.page
      .evaluate(() => {
        const ids = Array.from(document.querySelectorAll("[id]"))
          .map((element) => element.id)
          .filter((id) => id.startsWith("aura-contact-item-"))
          .slice(0, 50);
        const root =
          document.getElementById("aura-app-root") ??
          document.querySelector("main:last-of-type") ??
          document.body;
        const text = String(root?.textContent ?? "")
          .replace(/\s+/g, " ")
          .trim()
          .slice(0, 1200);
        return { ids, text };
      })
      .catch(() => ({ ids: [], text: "" }));
    throw new Error(
      `${error.message} current_contact_ids=${JSON.stringify(diagnostics.ids)} text_snippet=${JSON.stringify(diagnostics.text)}`,
    );
  }
  traceDriver(
    `[driver] wait_for_selector done instance=${instanceId} selector=${selector} source=playwright`,
  );
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
  return page.locator("main").last();
}

function normalizeClickTarget(label) {
  return String(label || "")
    .trim()
    .replace(/^\(|^\"|^'|^\[|^\{|^</g, "")
    .replace(/\)|\"|\'|\]|\}|>$|:$|\.$/g, "")
    .trim();
}

function navigationShortcutForSelector(selector) {
  if (!selector) {
    return null;
  }
  if (selector.includes("aura-nav-neighborhood")) {
    return "1";
  }
  if (selector.includes("aura-nav-chat")) {
    return "2";
  }
  if (selector.includes("aura-nav-contacts")) {
    return "3";
  }
  if (selector.includes("aura-nav-notifications")) {
    return "4";
  }
  if (selector.includes("aura-nav-settings")) {
    return "5";
  }
  return null;
}

function parseBoundedInt(params, key, fallback, min, max) {
  const raw = params?.[key];
  if (raw == null) {
    return fallback;
  }
  if (
    typeof raw !== "number" ||
    !Number.isFinite(raw) ||
    !Number.isInteger(raw)
  ) {
    throw new Error(`${key} must be an integer number`);
  }
  if (raw < min || raw > max) {
    throw new Error(`${key} must be between ${min} and ${max}, got ${raw}`);
  }
  return raw;
}

function parseStartOptions(params) {
  const instanceId = normalizeInstanceId(params);
  const appUrl = String(params?.app_url ?? "http://127.0.0.1:4173");
  const scenarioSeed = params?.scenario_seed ?? null;
  const dataDir = String(
    params?.data_dir ?? path.join(".tmp", "harness", instanceId),
  );
  const headless = params?.headless !== false;
  const artifactDir = params?.artifact_dir ? String(params.artifact_dir) : null;
  const pageGotoTimeoutMs = parseBoundedInt(
    params,
    "page_goto_timeout_ms",
    DEFAULT_PAGE_GOTO_TIMEOUT_MS,
    1,
    MAX_TIMEOUT_MS,
  );
  const harnessReadyTimeoutMs = parseBoundedInt(
    params,
    "harness_ready_timeout_ms",
    DEFAULT_HARNESS_READY_TIMEOUT_MS,
    1,
    MAX_TIMEOUT_MS,
  );
  const startMaxAttempts = parseBoundedInt(
    params,
    "start_max_attempts",
    DEFAULT_START_MAX_ATTEMPTS,
    1,
    MAX_START_ATTEMPTS,
  );
  const startRetryBackoffMs = parseBoundedInt(
    params,
    "start_retry_backoff_ms",
    DEFAULT_START_RETRY_BACKOFF_MS,
    0,
    MAX_TIMEOUT_MS,
  );
  const resetStorage = params?.reset_storage === true;
  const requireSemanticReady = params?.require_semantic_ready !== false;

  return {
    instanceId,
    appUrl,
    scenarioSeed,
    dataDir,
    headless,
    artifactDir,
    pageGotoTimeoutMs,
    harnessReadyTimeoutMs,
    startMaxAttempts,
    startRetryBackoffMs,
    resetStorage,
    requireSemanticReady,
  };
}

function requestTimeoutMs(method, params) {
  switch (method) {
    case "wait_for_dom_patterns":
    case "wait_for_selector": {
      const timeoutMs = Number(params?.timeout_ms ?? 30000);
      return Math.max(1000, timeoutMs + 5000);
    }
    case "wait_for_ui_state": {
      const timeoutMs = Number(params?.timeout_ms ?? UI_STATE_TIMEOUT_MS);
      return Math.max(1000, timeoutMs + 5000);
    }
    case "click_button":
    case "fill_input":
    case "submit_semantic_command":
      return 30000;
    case "reload_page":
    case "recover_ui_state":
    case "restart_page_session":
      return 60000;
    case "start_page": {
      const pageGotoTimeoutMs = Number(
        params?.page_goto_timeout_ms ?? DEFAULT_PAGE_GOTO_TIMEOUT_MS,
      );
      const harnessReadyTimeoutMs = Number(
        params?.harness_ready_timeout_ms ?? DEFAULT_HARNESS_READY_TIMEOUT_MS,
      );
      return Math.max(1000, pageGotoTimeoutMs + harnessReadyTimeoutMs + 10000);
    }
    default:
      return 15000;
  }
}

function withHarnessHarnessQuery(appUrl, instanceId, scenarioSeed) {
  const url = new URL(appUrl);
  url.searchParams.set("__aura_harness_instance", instanceId);
  if (
    scenarioSeed !== undefined &&
    scenarioSeed !== null &&
    scenarioSeed !== ""
  ) {
    url.searchParams.set("__aura_harness_scenario_seed", String(scenarioSeed));
  }
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
    scenarioSeed,
    dataDir,
    headless,
    artifactDir,
    pageGotoTimeoutMs,
    harnessReadyTimeoutMs,
    startMaxAttempts,
    startRetryBackoffMs,
    resetStorage,
    requireSemanticReady,
  } = options;
  const targetUrl = withHarnessHarnessQuery(appUrl, instanceId, scenarioSeed);

  if (sessions.has(instanceId)) {
    await stop({ instance_id: instanceId });
  }

  if (resetStorage) {
    resetPersistentProfileDir(dataDir);
  } else {
    ensureDir(dataDir);
  }
  ensureDir(artifactDir);

  const consoleLog = [];
  let lastError = null;

  for (let attempt = 1; attempt <= startMaxAttempts; attempt += 1) {
    let context = null;
    try {
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} launchPersistentContext start`,
      );
      const chromium = await getChromium();
      context = await chromium.launchPersistentContext(dataDir, {
        headless,
        viewport: { width: 1280, height: 900 },
        ignoreHTTPSErrors: true,
      });
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} launchPersistentContext done`,
      );

      const page = context.pages()[0] ?? (await context.newPage());
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} page acquired`,
      );
      const session = {
        id: instanceId,
        context,
        page,
        startOptions: { ...options, resetStorage: false },
        headless,
        appUrl: targetUrl,
        dataDir,
        artifactDir,
        consoleLog,
        tracePath:
          artifactDir && PLAYWRIGHT_TRACE_ENABLED
            ? path.join(artifactDir, `${instanceId}-trace.zip`)
            : null,
        domState: normalizeDomState({ text: "", ids: [] }),
        uiStateCache: null,
        uiStateCacheJson: null,
        uiStateVersion: 0,
        uiStateWaiters: [],
        requiredUiStateRevision: 0,
        observationEpoch: 0,
        lastObservationResetReason: null,
        lastUiStateSource: null,
        lastMutationReason: null,
        renderHeartbeat: null,
        clipboardCache: "",
        semanticResultCache: Object.create(null),
        semanticResultWaiters: Object.create(null),
        domStateWaiters: [],
      };
      sessions.set(instanceId, session);

      page.on("console", (message) => {
        const text = message.text();
        if (text.startsWith(UI_STATE_JSON_LOG_PREFIX)) {
          const payload = text.slice(UI_STATE_JSON_LOG_PREFIX.length);
          storeUiState(session, payload, "console_push");
          consoleLog.push(
            `[${nowIso()}] ${message.type()}: ${UI_STATE_JSON_LOG_PREFIX}<json>`,
          );
          return;
        }
        if (text.startsWith(UI_STATE_LOG_PREFIX)) {
          consoleLog.push(`[${nowIso()}] ${message.type()}: ${text}`);
          return;
        }
        consoleLog.push(`[${nowIso()}] ${message.type()}: ${text}`);
      });

      if (artifactDir && PLAYWRIGHT_TRACE_ENABLED) {
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing start`,
        );
        await context.tracing.start({
          screenshots: true,
          snapshots: true,
          sources: true,
        });
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing done`,
        );
      }

      await installUiStateObserver(page, session);
      installPageNavigationReset(session);

      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} goto start url=${targetUrl}`,
      );
      await page.goto(targetUrl, {
        waitUntil: "commit",
        timeout: pageGotoTimeoutMs,
      });
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} goto done`,
      );
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} profile_reset_mode=${
          resetStorage ? "prelaunch_profile_reset" : "preserve_profile"
        }`,
      );
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensurePageInteractive start`,
      );
      await ensurePageInteractive(page, harnessReadyTimeoutMs);
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensurePageInteractive done`,
      );
      try {
        const bindingType = await page.evaluate(
          () => typeof window.__AURA_DRIVER_PUSH_UI_STATE,
        );
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} uiStateBinding type=${bindingType}`,
        );
      } catch (error) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} uiStateBinding probe failed: ${
            error?.message ?? String(error)
          }`,
        );
      }
      try {
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout start`,
        );
        await ensureHarnessWithTimeout(
          page,
          Math.min(harnessReadyTimeoutMs, 5000),
        );
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout done`,
        );
        await installUiStateObserver(page, session);
        await assertRootStructure({ page }, "startup");
      } catch (error) {
        consoleLog.push(
          `[${nowIso()}] harness bridge not ready after startup: ${error?.message ?? String(error)}`,
        );
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout optional failure: ${
            error?.message ?? String(error)
          }`,
        );
      }
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installHarnessMutationQueue start`,
      );
      await installHarnessMutationQueue(page, session);
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installHarnessMutationQueue done`,
      );
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installDomObserver start`,
      );
      await installDomObserver(page, session);
      traceDriver(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installDomObserver done`,
      );
      try {
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready start`,
        );
        await uiState({ instance_id: instanceId });
        traceDriver(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready done`,
        );
      } catch (error) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} semantic_ready failed: ${
            error?.message ?? String(error)
          }`,
        );
        if (requireSemanticReady) {
          throw error;
        }
        consoleLog.push(
          `[${nowIso()}] semantic_ready deferred after startup: ${
            error?.message ?? String(error)
          }`,
        );
      }

      return {
        instance_id: instanceId,
        app_url: targetUrl,
        data_dir: dataDir,
        headless,
      };
    } catch (error) {
      sessions.delete(instanceId);
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} failed: ${
          error?.stack ?? error?.message ?? String(error)
        }`,
      );
      lastError = error;
      consoleLog.push(
        `[${nowIso()}] start_page attempt ${attempt}/${startMaxAttempts} failed: ${
          error?.message ?? String(error)
        }`,
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
    }`,
  );
}

async function restartPageSession(session, reason) {
  const options = session.startOptions;
  if (!options) {
    throw new Error(
      `restart_page_session missing start options for ${session.id}`,
    );
  }
  console.error(
    `[driver] restart_page_session instance=${session.id} reason=${reason} data_dir=${options.dataDir}`,
  );
  await stop({ instance_id: session.id });
  await startPage({
    instance_id: options.instanceId,
    app_url: options.appUrl,
    scenario_seed: options.scenarioSeed,
    data_dir: options.dataDir,
    headless: options.headless,
    artifact_dir: options.artifactDir,
    page_goto_timeout_ms: options.pageGotoTimeoutMs,
    harness_ready_timeout_ms: options.harnessReadyTimeoutMs,
    start_max_attempts: options.startMaxAttempts,
    start_retry_backoff_ms: options.startRetryBackoffMs,
    require_semantic_ready:
      reason !== "create_account_bootstrap" &&
      reason !== "stage_runtime_identity_bootstrap",
  });
  return getSession(options.instanceId);
}

function getSession(instanceId) {
  const session = sessions.get(instanceId);
  if (!session) {
    throw new Error(`unknown session: ${instanceId}`);
  }
  return session;
}

function tryParseUiStateValue(value) {
  if (typeof value === "string") {
    try {
      return JSON.parse(value);
    } catch {
      return null;
    }
  }
  return value && typeof value === "object" ? value : null;
}

async function readStructuredUiState(
  session,
  instanceId,
  reason,
  timeoutMs = 1000,
  { storeResult = false } = {},
) {
  traceDriver(
    `[driver] ui_state structured_read start instance=${instanceId} reason=${reason} timeout_ms=${timeoutMs}`,
  );
  const pushTimeoutMs =
    timeoutMs <= 1 ? timeoutMs : Math.max(1, Math.min(timeoutMs - 1, 1500));
  const currentRevision =
    session.uiStateCache && typeof session.uiStateCache === "object"
      ? uiSnapshotRevision(session.uiStateCache)
      : -1;
  try {
    const pushed = await waitForUiStateVersion(
      session,
      currentRevision,
      pushTimeoutMs,
    );
    await ensureUiStateRenderConvergence(
      session,
      pushed.snapshot,
      `${reason}:driver_push`,
    );
    if (storeResult) {
      storeUiState(session, pushed.snapshot, `driver_push:${reason}`);
    }
    traceDriver(
      `[driver] ui_state structured_read pushed instance=${instanceId} reason=${reason} version=${pushed.version}`,
    );
    return pushed.snapshot;
  } catch (error) {
    traceDriver(
      `[driver] ui_state structured_read pushed_unavailable instance=${instanceId} reason=${reason} error=${error?.message ?? String(error)}`,
    );
  }

  const fallbackTimeoutMs = Math.max(1, timeoutMs - pushTimeoutMs);
  const payload = await withOperationTimeout(
    `ui_state_structured_${reason}`,
    (async () => {
      return session.page.evaluate(() => {
        if (typeof window.__AURA_UI_STATE_JSON__ === "string") {
          return window.__AURA_UI_STATE_JSON__;
        }
        if (
          window.__AURA_UI_STATE_CACHE__ &&
          typeof window.__AURA_UI_STATE_CACHE__ === "object"
        ) {
          return window.__AURA_UI_STATE_CACHE__;
        }
        if (typeof window.__AURA_HARNESS_OBSERVE__?.ui_state === "function") {
          return window.__AURA_HARNESS_OBSERVE__.ui_state();
        }
        if (typeof window.__AURA_UI_STATE__ === "function") {
          const payload = window.__AURA_UI_STATE__();
          if (payload != null) {
            return payload;
          }
        }
        throw new Error(
          `ui_state_publication_unavailable:${JSON.stringify({
            ui_state: window.__AURA_UI_PUBLICATION_STATE__ ?? null,
            render_heartbeat:
              window.__AURA_RENDER_HEARTBEAT_PUBLICATION_STATE__ ?? null,
          })}`,
        );
      });
    })(),
    fallbackTimeoutMs,
  );
  const parsed = tryParseUiStateValue(payload);
  if (parsed && typeof parsed === "object") {
    await ensureUiStateRenderConvergence(session, parsed, reason);
    if (storeResult) {
      storeUiState(session, parsed, `structured:${reason}`);
    }
    traceDriver(
      `[driver] ui_state structured_read done instance=${instanceId} reason=${reason}`,
    );
    return parsed;
  }
  traceDriver(
    `[driver] ui_state structured_read unavailable instance=${instanceId} reason=${reason}`,
  );
  return null;
}

async function readStructuredUiStateWithNavigationRecovery(
  session,
  instanceId,
  reason,
  timeoutMs = 1000,
) {
  try {
    return await readStructuredUiState(session, instanceId, reason, timeoutMs, {
      storeResult: false,
    });
  } catch (error) {
    if (!isUiStateRecoveryCandidate(error)) {
      throw error;
    }
    resetObservationState(session, `structured_navigation_recovery:${reason}`);
  traceDriver(
    `[driver] ui_state structured_navigation_recovery instance=${instanceId} reason=${reason}`,
  );
    await waitForPageNavigationStabilization(
      session,
      `structured_navigation_${reason}`,
    );
    await ensureHarnessWithTimeout(session.page, UI_STATE_TIMEOUT_MS);
    await installUiStateObserver(session.page, session);
    await installHarnessMutationQueue(session.page, session);
    await installDomObserver(session.page, session);
    await assertRootStructure(session, `ui_state_after_navigation_${reason}`);
    return readStructuredUiState(
      session,
      instanceId,
      `post_navigation_${reason}`,
      UI_STATE_TIMEOUT_MS,
      {
        storeResult: false,
      },
    );
  }
}

async function sendKeys(params) {
  const instanceId = normalizeInstanceId(params);
  const keys = String(params?.keys ?? "");
  const session = getSession(instanceId);

  console.error(
    `[driver] send_keys start instance=${instanceId} bytes=${keys.length}`,
  );
  try {
    const harnessResult = await withOperationTimeout(
      `send_keys_harness:${instanceId}`,
      dispatchHarnessKeysText(session.page, keys),
      2000,
    );
    if (harnessResult?.ok) {
      console.error(
        `[driver] send_keys done instance=${instanceId} mode=${harnessResult.mode ?? "harness"}`,
      );
      return {
        status: "sent",
        bytes: keys.length,
        mode: harnessResult.mode ?? "harness",
      };
    }
  } catch (error) {
    console.error(
      `[driver] send_keys harness_path_failed instance=${instanceId} error=${normalizeClickError(error)}`,
    );
  }
  console.error(`[driver] send_keys focus start instance=${instanceId}`);
  await focusAuraPageSafe(session.page, instanceId, "send_keys");
  console.error(`[driver] send_keys focus done instance=${instanceId}`);
  console.error(`[driver] send_keys type start instance=${instanceId}`);
  await withOperationTimeout(
    `type_keys:${instanceId}`,
    typeKeyStream(session.page, keys),
    8000,
  );
  console.error(`[driver] send_keys type done instance=${instanceId}`);

  console.error(`[driver] send_keys done instance=${instanceId}`);
  return { status: "sent", bytes: keys.length };
}

async function sendKey(params) {
  const instanceId = normalizeInstanceId(params);
  const key = String(params?.key ?? "");
  const repeat = Number(params?.repeat ?? 1);
  const session = getSession(instanceId);
  const count = Number.isFinite(repeat) ? Math.max(1, Math.floor(repeat)) : 1;

  try {
    const harnessResult = await withOperationTimeout(
      `send_key_harness:${instanceId}`,
      dispatchHarnessKey(session.page, key, count),
      2000,
    );
    if (harnessResult?.ok) {
      return { status: "sent", mode: harnessResult.mode ?? "harness" };
    }
  } catch (error) {
    console.error(
      `[driver] send_key harness_path_failed instance=${instanceId} key=${key} error=${normalizeClickError(error)}`,
    );
  }

  await focusAuraPageSafe(session.page, instanceId, "send_key");
  for (let index = 0; index < count; index += 1) {
    await pressMappedKey(session.page, key);
  }

  return { status: "sent" };
}

async function navigateScreen(params) {
  const instanceId = normalizeInstanceId(params);
  const screen = String(params?.screen ?? "").trim();
  if (!screen) {
    throw new Error("screen is required");
  }
  const session = getSession(instanceId);
  let result;
  try {
    result = await withOperationTimeout(
      `navigate_screen:${instanceId}:${screen}`,
      session.page.evaluate((targetScreen) => {
        if (!window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED) {
          return { ok: false, reason: "mutation_queue_missing" };
        }
        window.__AURA_DRIVER_PENDING_NAV_SCREEN__ = targetScreen;
        window.__AURA_DRIVER_WAKE_PENDING_NAV__?.();
        return { ok: true };
      }, screen),
      1000,
    );
  } catch (error) {
    console.error(
      `[driver] navigate_screen restart_retry instance=${instanceId} screen=${screen} error=${error?.message ?? String(error)}`,
    );
    const restartedSession = await restartPageSession(
      session,
      `navigate_screen:${screen}`,
    );
    result = await withOperationTimeout(
      `navigate_screen_restart:${instanceId}:${screen}`,
      restartedSession.page.evaluate((targetScreen) => {
        if (!window.__AURA_DRIVER_MUTATION_QUEUE_INSTALLED) {
          return { ok: false, reason: "mutation_queue_missing" };
        }
        window.__AURA_DRIVER_PENDING_NAV_SCREEN__ = targetScreen;
        window.__AURA_DRIVER_WAKE_PENDING_NAV__?.();
        return { ok: true };
      }, screen),
      2000,
    );
  }
  if (!result?.ok) {
    throw new Error(
      `navigate_screen_failed screen=${screen} reason=${result?.reason ?? "unknown"}`,
    );
  }
  return { status: "navigated", screen };
}

async function openSettingsSection(params) {
  const instanceId = normalizeInstanceId(params);
  const section = String(params?.section ?? "").trim();
  if (!section) {
    throw new Error("section is required");
  }
  const session = getSession(instanceId);
  let result;
  try {
    result = await withOperationTimeout(
      `open_settings_section:${instanceId}:${section}`,
      session.page.evaluate((targetSection) => {
        const harness = window.__AURA_HARNESS__;
        if (typeof harness?.open_settings_section !== "function") {
          return { ok: false, reason: "open_settings_section_missing" };
        }
        const accepted = harness.open_settings_section(targetSection);
        return { ok: accepted === true };
      }, section),
      1000,
    );
  } catch (error) {
    console.error(
      `[driver] open_settings_section restart_retry instance=${instanceId} section=${section} error=${error?.message ?? String(error)}`,
    );
    const restartedSession = await restartPageSession(
      session,
      `open_settings_section:${section}`,
    );
    result = await withOperationTimeout(
      `open_settings_section_restart:${instanceId}:${section}`,
      restartedSession.page.evaluate((targetSection) => {
        const harness = window.__AURA_HARNESS__;
        if (typeof harness?.open_settings_section !== "function") {
          return { ok: false, reason: "open_settings_section_missing" };
        }
        const accepted = harness.open_settings_section(targetSection);
        return { ok: accepted === true };
      }, section),
      2000,
    );
  }
  if (!result?.ok) {
    throw new Error(
      `open_settings_section_failed section=${section} reason=${result?.reason ?? "unknown"}`,
    );
  }
  return { status: "opened", section };
}

async function snapshot(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const screenshot = params?.screenshot !== false;
  let usedDomFallback = false;

  let payload = null;
  try {
    payload = await withOperationTimeout(
      "snapshot",
      session.page.evaluate(() => {
        if (window.__AURA_HARNESS_OBSERVE__?.snapshot) {
          return window.__AURA_HARNESS_OBSERVE__.snapshot();
        }
        return null;
      }),
    );
  } catch (error) {
    console.error(
      `[driver] snapshot observer timeout instance=${instanceId} falling back to dom snapshot error=${
        error?.message ?? String(error)
      }`,
    );
  }

  if (payload == null) {
    usedDomFallback = true;
    const cachedDomSnapshot = domSnapshotFromCache(session);
    if (
      cachedDomSnapshot.screen ||
      cachedDomSnapshot.raw_screen ||
      cachedDomSnapshot.authoritative_screen
    ) {
      payload = cachedDomSnapshot;
    } else {
      try {
        payload = await readDomSnapshot(session.page);
      } catch (error) {
        throw new Error(
          `${error}\nBrowser console tail:\n${consoleTailText(session)}`,
        );
      }
    }
  }
  const normalized = parseSnapshotPayload(payload);

  let screenshotPath = null;
  if (screenshot && session.artifactDir && !usedDomFallback) {
    screenshotPath = path.join(
      session.artifactDir,
      `${instanceId}-${Date.now()}.png`,
    );
    await session.page.screenshot({ path: screenshotPath, fullPage: true });
  }

  return {
    ...normalized,
    screenshot_path: screenshotPath,
  };
}

async function uiState(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const recoveryTimeoutMs = Math.min(UI_STATE_TIMEOUT_MS, 4000);
  const recentConsole = consoleTailText(session, 8).replace(/\n/g, " | ");
  traceDriver(
    `[driver] ui_state start instance=${instanceId} cache_type=${typeof session.uiStateCache} cache_json=${typeof session.uiStateCacheJson} heartbeat_seq=${session.renderHeartbeat?.render_seq ?? "none"} console_tail=${recentConsole}`,
  );

  if (session.uiStateCache && typeof session.uiStateCache === "object") {
    const cached =
      typeof session.uiStateCacheJson === "string"
        ? tryParseUiStateValue(session.uiStateCacheJson)
        : session.uiStateCache;
    const staleReason = uiStateStalenessReason(session, cached);
    if (!staleReason) {
      traceDriver(`[driver] ui_state cache_hit instance=${instanceId}`);
      return cached;
    }
    traceDriver(
      `[driver] ui_state stale_cache instance=${instanceId} reason=${staleReason} source=${session.lastUiStateSource ?? "unknown"} mutation=${session.lastMutationReason ?? "none"}`,
    );
  }

  traceDriver(`[driver] ui_state cache_miss instance=${instanceId}`);
  const observed = await readStructuredUiState(
    session,
    instanceId,
    "observation",
    recoveryTimeoutMs,
  ).catch((error) => {
    throw new Error(
      `structured ui_state observation failed for instance ${instanceId}: ${error}\nBrowser console tail:\n${consoleTailText(session)}`,
    );
  });
  if (observed) {
    const staleReason = uiStateStalenessReason(session, observed);
    if (staleReason) {
      throw new Error(`structured_ui_state_stale:${staleReason}`);
    }
    return observed;
  }

  throw new Error(
    `browser UI state unavailable for instance ${instanceId}; primary_observation=driver_push_cache secondary_observation=structured_ui_state heartbeat=${JSON.stringify(
      session.renderHeartbeat,
    )}\nBrowser console tail:\n${consoleTailText(session)}`,
  );
}

async function waitForUiState(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const timeoutMs = Number(params?.timeout_ms ?? UI_STATE_TIMEOUT_MS);
  const rawAfterVersion = params?.after_version;
  const afterVersion =
    rawAfterVersion == null
      ? null
      : Number.isFinite(rawAfterVersion)
        ? Number(rawAfterVersion)
        : 0;

  if (afterVersion == null) {
    const snapshot = await uiState({ instance_id: instanceId });
    return {
      snapshot,
      version: uiSnapshotRevision(snapshot),
    };
  }

  const deadlineMs = Date.now() + timeoutMs;

  while (true) {
    const remainingMs = Math.max(1, deadlineMs - Date.now());
    if (!session.uiStateCache || typeof session.uiStateCache !== "object") {
      try {
        const snapshot = await uiState({ instance_id: instanceId });
        const version = uiSnapshotRevision(snapshot);
        if (version > afterVersion) {
          await ensureUiStateRenderConvergence(session, snapshot, "event_wait");
          return { snapshot, version };
        }
      } catch (error) {
        if (!isUiStateRecoveryCandidate(error)) {
          throw error;
        }

        const recovered = await readStructuredUiStateWithNavigationRecovery(
          session,
          instanceId,
          "wait_for_ui_state_navigation",
          remainingMs,
        );
        const staleReason = uiStateStalenessReason(session, recovered);
        if (!staleReason) {
          storeUiState(session, recovered, "wait_for_ui_state_recovery");
          const recoveredVersion = uiSnapshotRevision(recovered);
          if (recoveredVersion > afterVersion) {
            await ensureUiStateRenderConvergence(
              session,
              recovered,
              "event_wait_recovery",
            );
            return {
              snapshot: recovered,
              version: recoveredVersion,
            };
          }
        }
      }
    }

    try {
      const result = await waitForUiStateVersion(
        session,
        afterVersion,
        remainingMs,
      );
      await ensureUiStateRenderConvergence(
        session,
        result.snapshot,
        "event_wait",
      );
      return result;
    } catch (error) {
      if (!isUiStateRecoveryCandidate(error)) {
        throw error;
      }

      if (Date.now() >= deadlineMs) {
        throw new Error(
          `wait_for_ui_state timed out after ${timeoutMs}ms after_version=${afterVersion} current_revision=${uiSnapshotRevision(
            session.uiStateCache,
          )} invalidation=${String(error?.message ?? error)}`,
        );
      }

      const recovered = await readStructuredUiStateWithNavigationRecovery(
        session,
        instanceId,
        "wait_for_ui_state_navigation",
        Math.max(1, deadlineMs - Date.now()),
      );
      const staleReason = uiStateStalenessReason(session, recovered);
      if (staleReason) {
        continue;
      }

      storeUiState(session, recovered, "wait_for_ui_state_recovery");
      const recoveredVersion = uiSnapshotRevision(recovered);
      if (recoveredVersion > afterVersion) {
        await ensureUiStateRenderConvergence(
          session,
          recovered,
          "event_wait_recovery",
        );
        return {
          snapshot: recovered,
          version: recoveredVersion,
        };
      }
    }
  }
}

async function recoverUiState(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const reason = String(params?.reason ?? "explicit");
  console.error(
    `[driver] recover_ui_state start instance=${instanceId} reason=${reason}`,
  );
  const recovered = await readStructuredUiStateWithNavigationRecovery(
    session,
    instanceId,
    `explicit_recovery:${reason}`,
    UI_STATE_TIMEOUT_MS,
  );
  if (!recovered) {
    throw new Error(
      `recover_ui_state failed instance=${instanceId} reason=${reason}`,
    );
  }
  const staleReason = uiStateStalenessReason(session, recovered);
  if (staleReason) {
    throw new Error(
      `recover_ui_state returned stale snapshot instance=${instanceId} reason=${reason} stale=${staleReason}`,
    );
  }
  storeUiState(session, recovered, `recovery:${reason}`);
  console.error(
    `[driver] recover_ui_state done instance=${instanceId} reason=${reason}`,
  );
  return recovered;
}

async function stageRuntimeIdentity(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const runtimeIdentityJson = String(params?.runtime_identity_json ?? "").trim();
  if (!runtimeIdentityJson) {
    throw new Error("runtime_identity_json is required");
  }

  const rebootstrapResult = await session.page.evaluate(async (serializedIdentity) => {
    const stageRuntimeIdentity = window.__AURA_HARNESS__?.stage_runtime_identity;
    if (typeof stageRuntimeIdentity !== "function") {
      return { ok: false, reason: "stage_runtime_identity_missing" };
    }
    await stageRuntimeIdentity(serializedIdentity);
    return { ok: true };
  }, runtimeIdentityJson);
  if (!rebootstrapResult?.ok) {
    throw new Error(
      `stage_runtime_identity_rebootstrap_failed:${rebootstrapResult?.reason ?? "unknown"}`,
    );
  }

  return {
    status: "staged",
    runtime_identity_json: runtimeIdentityJson,
    storage_key: null,
  };
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
      `${error}\nBrowser console tail:\n${consoleTailText(session)}`,
    );
  }
  return parseSnapshotPayload(payload);
}

async function clickButton(params) {
  const instanceId = normalizeInstanceId(params);
  const selector = String(params?.selector ?? "").trim();
  const label = String(params?.label ?? "").trim();
  const effectiveLabel = label;
  const session = getSession(instanceId);
  console.error(
    `[driver] click_button start instance=${instanceId} selector=${selector || "-"} label=${label || "-"}`,
  );

  if (selector) {
    let selectorError = null;
    try {
      await clickByCssSelector(session.page, selector, session);
      console.error(
        `[driver] click_button done instance=${instanceId} selector=${selector} via=css`,
      );
      return { status: "clicked" };
    } catch (selectorError) {
      console.error(
        `[driver] click_button selector_failed instance=${instanceId} selector=${selector} error=${selectorError?.message ?? String(selectorError)}`,
      );
    }

    throw new Error(
      `click_button failed selector=${selector} label=${effectiveLabel || "-"} dom_error=${selectorError?.message ?? "unknown"}`,
    );
  }

  if (!label && !effectiveLabel) {
    throw new Error("label is required");
  }
  const activeLabel = effectiveLabel || label;

  await clickByLabelText(session.page, activeLabel, session);

  console.error(
    `[driver] click_button done instance=${instanceId} label=${activeLabel}`,
  );

  return { status: "clicked" };
}

async function fillInput(params) {
  const instanceId = normalizeInstanceId(params);
  const selector = String(params?.selector ?? "").trim();
  const value = String(params?.value ?? "");
  if (!selector) {
    throw new Error("selector is required");
  }
  const session = getSession(instanceId);
  console.error(
    `[driver] fill_input start instance=${instanceId} selector=${selector}`,
  );
  const locator = session.page.locator(selector).first();
  const domCacheHasSelector =
    selector.startsWith("#") && domStateHasId(session, selector.slice(1));
  const attachTimeoutMs = domCacheHasSelector ? 2000 : 8000;
  const focusTimeoutMs = domCacheHasSelector ? 1500 : 3000;
  const fillTimeoutMs = domCacheHasSelector ? 2000 : 3000;
  console.error(
    `[driver] fill_input dom_cache instance=${instanceId} selector=${selector} present=${domCacheHasSelector}`,
  );
  try {
    await focusAuraPageSafe(
      session.page,
      instanceId,
      `fill_input_start:${selector}`,
    );
    console.error(
      `[driver] fill_input attach_wait start instance=${instanceId} selector=${selector}`,
    );
    await withOperationTimeout(
      `fill_input_attach:${selector}`,
      locator.waitFor({ state: "attached", timeout: attachTimeoutMs }),
      attachTimeoutMs + 1000,
    );
    console.error(
      `[driver] fill_input attach_wait done instance=${instanceId} selector=${selector}`,
    );
    console.error(
      `[driver] fill_input focus start instance=${instanceId} selector=${selector}`,
    );
    await withOperationTimeout(
      `fill_input_focus:${selector}`,
      locator.focus({ timeout: focusTimeoutMs }),
      focusTimeoutMs + 1000,
    );
    console.error(
      `[driver] fill_input focus done instance=${instanceId} selector=${selector}`,
    );
    console.error(
      `[driver] fill_input playwright_fill start instance=${instanceId} selector=${selector}`,
    );
    await withOperationTimeout(
      `fill_input_fill:${selector}`,
      locator.fill(value, { timeout: fillTimeoutMs }),
      fillTimeoutMs + 2000,
    );
    console.error(
      `[driver] fill_input playwright_fill done instance=${instanceId} selector=${selector}`,
    );
    await withOperationTimeout(
      `fill_input_commit:${selector}`,
      session.page.evaluate(
        ({ targetSelector, nextValue }) => {
          const element = document.querySelector(targetSelector);
          if (
            !(
              element instanceof HTMLInputElement ||
              element instanceof HTMLTextAreaElement
            )
          ) {
            return { ok: false, reason: "field_not_found" };
          }
          element.focus();
          element.value = nextValue;
          element.dispatchEvent(
            new InputEvent("input", { bubbles: true, data: nextValue }),
          );
          element.dispatchEvent(new Event("change", { bubbles: true }));
          element.dispatchEvent(new FocusEvent("blur", { bubbles: true }));
          return {
            ok: true,
            disabled: element.disabled,
            readOnly: element.readOnly,
            value: element.value,
          };
        },
        { targetSelector: selector, nextValue: value },
      ),
      fillTimeoutMs + 2000,
    );
    await withOperationTimeout(
      `fill_input_value_ack:${selector}`,
      session.page.waitForFunction(
        ({ targetSelector, nextValue }) => {
          const element = document.querySelector(targetSelector);
          return (
            (element instanceof HTMLInputElement ||
              element instanceof HTMLTextAreaElement) &&
            element.value === nextValue
          );
        },
        { targetSelector: selector, nextValue: value },
        { timeout: fillTimeoutMs },
      ),
      fillTimeoutMs + 1000,
    );
  } catch (error) {
    console.error(
      `[driver] fill_input playwright_path_failed instance=${instanceId} selector=${selector} error=${error?.message ?? String(error)}`,
    );
    const diagnostics = {
      ids: domStateIdList(session)
        .filter(
          (id) =>
            id.startsWith("aura-screen-") ||
            id.startsWith("aura-field-") ||
            id.startsWith("aura-chat-"),
        )
        .slice(0, 100),
      text: session.domState.text.slice(0, 1200),
    };
    throw new Error(
      `${error.message} current_ids=${JSON.stringify(diagnostics.ids)} text_snippet=${JSON.stringify(
        diagnostics.text,
      )}`,
    );
  }
  console.error(
    `[driver] fill_input done instance=${instanceId} selector=${selector}`,
  );
  return { status: "filled", bytes: value.length };
}

async function readClipboard(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  let lastText = String(session.clipboardCache ?? "");
  if (lastText.trim().length > 0) {
    return { text: lastText };
  }
  for (let attempt = 0; attempt < 20; attempt += 1) {
    await new Promise((resolve) => setTimeout(resolve, 100));
    lastText = String(session.clipboardCache ?? "");
    if (lastText.trim().length > 0) {
      return { text: lastText };
    }
  }
  return { text: lastText };
}

async function submitSemanticCommand(params) {
  const instanceId = normalizeInstanceId(params);
  let session = getSession(instanceId);
  const request = params?.request;
  if (!request || typeof request !== "object") {
    throw new Error("semantic command request is required");
  }
  const requestJson = JSON.stringify(request);
  const commandId = `${instanceId}:${Date.now()}:${Math.random().toString(16).slice(2)}`;
  if (!session.semanticResultCache || typeof session.semanticResultCache !== "object") {
    session.semanticResultCache = Object.create(null);
  }
  if (!session.semanticResultWaiters || typeof session.semanticResultWaiters !== "object") {
    session.semanticResultWaiters = Object.create(null);
  }
  delete session.semanticResultCache[commandId];
  delete session.semanticResultWaiters[commandId];
  const responsePromise = waitForSemanticResult(session, commandId, 10000);
  traceDriver(
    `[driver] submit_semantic_command preflight instance=${instanceId}`,
  );
  traceDriver(
    `[driver] submit_semantic_command invoke_start instance=${instanceId}`,
  );
  const enqueuePayload = JSON.stringify({
    command_id: commandId,
    request_json: requestJson,
  });
  const enqueueCommand = async (activeSession, label, timeoutMs) =>
    withOperationTimeout(
      label,
      activeSession.page.evaluate((payloadJson) => {
        if (typeof window.__AURA_DRIVER_SEMANTIC_ENQUEUE__ !== "function") {
          throw new Error("window.__AURA_DRIVER_SEMANTIC_ENQUEUE__ is unavailable");
        }
        window.__AURA_DRIVER_SEMANTIC_ENQUEUE__(payloadJson);
        return null;
      }, enqueuePayload),
      timeoutMs,
    );
  try {
    await enqueueCommand(
      session,
      `submit_semantic_command_enqueue_${instanceId}`,
      2000,
    );
  } catch (error) {
    console.error(
      `[driver] submit_semantic_command enqueue_retry instance=${instanceId} error=${error?.message ?? String(error)}`,
    );
    try {
      await waitForPageNavigationStabilization(
        session,
        `submit_semantic_command_enqueue:${instanceId}`,
      );
      await enqueueCommand(
        session,
        `submit_semantic_command_enqueue_retry_${instanceId}`,
        4000,
      );
    } catch (retryError) {
      console.error(
        `[driver] submit_semantic_command enqueue_restart instance=${instanceId} error=${retryError?.message ?? String(retryError)}`,
      );
      const restartedSession = await restartPageSession(
        session,
        `submit_semantic_command_enqueue:${instanceId}`,
      );
      session = restartedSession;
      await enqueueCommand(
        restartedSession,
        `submit_semantic_command_enqueue_restart_${instanceId}`,
        4000,
      );
    }
  }

  let lastDebug = null;
  let response = null;
  try {
    response = await responsePromise;
  } catch (error) {
    const pageStatus = await withOperationTimeout(
      `submit_semantic_command_result_${instanceId}`,
      session.page.evaluate((activeCommandId) => {
        const semanticResults = window.__AURA_DRIVER_SEMANTIC_RESULTS__;
        const debug = window.__AURA_DRIVER_SEMANTIC_DEBUG__ ?? null;
        let result = null;
        if (
          semanticResults &&
          Object.prototype.hasOwnProperty.call(semanticResults, activeCommandId)
        ) {
          result = semanticResults[activeCommandId];
          delete semanticResults[activeCommandId];
        }
        return JSON.stringify({ result, debug });
      }, commandId),
      2000,
    ).catch(() => null);
    if (typeof pageStatus === "string") {
      try {
        const parsedStatus = JSON.parse(pageStatus);
        lastDebug = parsedStatus?.debug ?? lastDebug;
        if (parsedStatus?.result) {
          response = parsedStatus.result;
        }
      } catch {
        lastDebug = lastDebug;
      }
    }
    if (!response) {
      throw new Error(
        `submit_semantic_command timed out instance=${instanceId} command_id=${commandId} last_debug=${JSON.stringify(lastDebug)} cause=${error?.message ?? String(error)}`,
      );
    }
  }
  if (!response.ok) {
    throw new Error(
      response.error ?? `semantic command ${commandId} failed without error`,
    );
  }
  traceDriver(
    `[driver] submit_semantic_command resolved instance=${instanceId}`,
  );
  resetObservationState(session, "submit_semantic_command");
  if (typeof response.result === "string") {
    return JSON.parse(response.result);
  }
  return response.result;
}

async function getAuthorityId(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const authorityId = await session.page.evaluate(() => {
    if (
      typeof window.__AURA_HARNESS_OBSERVE__?.get_authority_id === "function"
    ) {
      return window.__AURA_HARNESS_OBSERVE__.get_authority_id();
    }
    return null;
  });
  if (authorityId == null) {
    return {};
  }
  return { authority_id: String(authorityId) };
}

async function reloadPage(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const reason = String(params?.reason ?? "manual_reload");
  try {
    console.error(
      `[driver] reload_page soft_reload start instance=${instanceId} reason=${reason}`,
    );
    await withOperationTimeout(
      `reload_page_soft:${instanceId}`,
      (async () => {
        resetObservationState(session, `reload_page:${reason}`);
        await session.page.reload({
          waitUntil: "commit",
          timeout:
            session.startOptions?.pageGotoTimeoutMs ??
            DEFAULT_PAGE_GOTO_TIMEOUT_MS,
        });
        await ensurePageInteractive(
          session.page,
          session.startOptions?.harnessReadyTimeoutMs ??
            DEFAULT_HARNESS_READY_TIMEOUT_MS,
        );
        await ensureHarnessWithTimeout(
          session.page,
          session.startOptions?.harnessReadyTimeoutMs ??
            DEFAULT_HARNESS_READY_TIMEOUT_MS,
        );
        await installUiStateObserver(session.page, session);
        await installHarnessMutationQueue(session.page, session);
        await installDomObserver(session.page, session);
        await uiState({ instance_id: instanceId });
      })(),
      Math.min(session.startOptions?.harnessReadyTimeoutMs ?? 30000, 30000),
    );
    console.error(
      `[driver] reload_page soft_reload done instance=${instanceId} reason=${reason}`,
    );
  } catch (error) {
    console.error(
      `[driver] reload_page soft_reload_failed instance=${instanceId} reason=${reason} error=${
        error?.stack ?? error?.message ?? String(error)
      }`,
    );
    await restartPageSession(session, reason);
  }
  return { status: "reloaded" };
}

async function tailLog(params): Promise<TailLogResult> {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const lines = Number(params?.lines ?? 20);
  const requested = Number.isFinite(lines)
    ? Math.max(1, Math.floor(lines))
    : 20;

  let harnessLines = [];
  try {
    const result = await withOperationTimeout(
      "tail_log",
      session.page.evaluate((count) => {
        return window.__AURA_HARNESS_OBSERVE__.tail_log(count);
      }, requested),
      1000,
    );
    if (Array.isArray(result)) {
      harnessLines = result;
    }
  } catch (error) {
    console.error(
      `[driver] tail_log fallback_to_console instance=${instanceId} error=${
        error?.message ?? String(error)
      }`,
    );
  }

  const merged = [
    ...(Array.isArray(harnessLines) ? harnessLines.map(String) : []),
    ...session.consoleLog,
  ].filter((line) => {
    const text = String(line);
    return !(
      text.includes("[driver] request start") ||
      text.includes("[driver] request done") ||
      text.includes("method=ui_state") ||
      text.includes("method=snapshot") ||
      text.includes("method=tail_log")
    );
  });

  return {
    lines: merged.slice(-requested),
  };
}

async function injectMessage(params) {
  const instanceId = normalizeInstanceId(params);
  const message = String(params?.message ?? "");
  const session = getSession(instanceId);

  await session.page.evaluate((value) => {
    if (window.__AURA_HARNESS__?.inject_message) {
      window.__AURA_HARNESS__.inject_message(value);
    }
  }, message);

  return { status: "injected" };
}

async function stop(params) {
  const instanceId = normalizeInstanceId(params);
  const session = sessions.get(instanceId);
  if (!session) {
    return { status: "already_stopped" };
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
    status: "stopped",
    trace_path: session.tracePath,
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

async function dispatch(method: DriverMethod, params: DriverRequest["params"]) {
  const instanceId =
    params &&
    typeof params === "object" &&
    typeof params.instance_id === "string"
      ? params.instance_id
      : null;
  const shouldMarkActionMutation =
    instanceId &&
    ACTION_METHODS.has(method) &&
    sessions.has(instanceId);
  if (shouldMarkActionMutation) {
    markObservationMutation(getSession(instanceId), method);
  }
  let result;
  switch (method) {
    case "start_page":
      result = await startPage(params);
      break;
    case "send_keys":
      result = await sendKeys(params);
      break;
    case "send_key":
      result = await sendKey(params);
      break;
    case "navigate_screen":
      result = await navigateScreen(params);
      break;
    case "open_settings_section":
      result = await openSettingsSection(params);
      break;
    case "click_button":
      result = await clickButton(params);
      break;
    case "fill_input":
      result = await fillInput(params);
      break;
    case "snapshot":
      result = await snapshot(params);
      break;
    case "ui_state":
      result = await uiState(params);
      break;
    case "wait_for_ui_state":
      result = await waitForUiState(params);
      break;
    case "dom_snapshot":
      result = await domSnapshot(params);
      break;
    case "wait_for_dom_patterns":
      result = await waitForDomPatterns(params);
      break;
    case "wait_for_selector":
      result = await waitForSelector(params);
      break;
    case "read_clipboard":
      result = await readClipboard(params);
      break;
    case "submit_semantic_command":
      result = await submitSemanticCommand(params);
      break;
    case "get_authority_id":
      result = await getAuthorityId(params);
      break;
    case "reload_page":
      result = await reloadPage(params);
      break;
    case "recover_ui_state":
      result = await recoverUiState(params);
      break;
    case "stage_runtime_identity":
      result = await stageRuntimeIdentity(params);
      break;
    case "restart_page_session":
      result = await restartPageSession(
        getSession(normalizeInstanceId(params)),
        "explicit_recovery",
      );
      break;
    case "tail_log":
      result = await tailLog(params);
      break;
    case "inject_message":
      result = await injectMessage(params);
      break;
    case "stop":
      result = await stop(params);
      break;
    default:
      throw new Error(`unsupported method: ${method}`);
  }
  if (
    instanceId &&
    !ACTION_METHODS.has(method) &&
    isMutatingMethod(method) &&
    sessions.has(instanceId)
  ) {
    markObservationMutation(getSession(instanceId), method);
  }
  return result;
}

if (process.argv.includes("--selftest")) {
  try {
    runSelfTest();
    process.exit(0);
  } catch (error) {
    console.error(
      `[driver] selftest failed: ${error?.stack ?? error?.message ?? String(error)}`,
    );
    process.exit(1);
  }
}

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

rl.on("line", (line) => {
  requestChain = requestChain
    .then(async () => {
      const raw = line.trim();
      if (!raw) {
        return;
      }

      let request: DriverRequest;
      try {
        request = JSON.parse(raw) as DriverRequest;
      } catch (error) {
        writeResponse(
          jsonResponse(null, false, `invalid JSON: ${error.message}`),
        );
        return;
      }

      const id = request.id ?? null;
      try {
        traceDriver(
          `[driver] request start id=${id} method=${request.method}`,
        );
        const result = await withOperationTimeout(
          `request:${request.method}`,
          dispatch(request.method, request.params ?? {}),
          requestTimeoutMs(request.method, request.params ?? {}),
        );
        traceDriver(
          `[driver] request done id=${id} method=${request.method}`,
        );
        writeResponse(jsonResponse(id, true, result));
      } catch (error) {
        console.error(
          `[driver] request failed id=${id} method=${request.method}: ${error?.stack ?? error?.message ?? String(error)}`,
        );
        writeResponse(
          jsonResponse(
            id,
            false,
            error?.stack ?? error?.message ?? String(error),
          ),
        );
      }
    })
    .catch((error) => {
      writeResponse(jsonResponse(null, false, error?.stack ?? String(error)));
    });
});

rl.on("close", async () => {
  try {
    await shutdownAll();
  } finally {
    process.exit(0);
  }
});

for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, async () => {
    try {
      await shutdownAll();
    } finally {
      process.exit(0);
    }
  });
}
