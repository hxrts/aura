#!/usr/bin/env node

import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { chromium } from 'playwright';

const sessions = new Map();
let requestChain = Promise.resolve();
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

async function focusAuraPage(page) {
  await page.evaluate(() => {
    window.focus();
    document.body.setAttribute('tabindex', '-1');
    document.body.focus();
  });
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function mapPlaywrightKey(key) {
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
  await page.keyboard.press(mapPlaywrightKey(key));
}

async function flushTypedBuffer(page, buffer) {
  if (!buffer) {
    return '';
  }
  await page.keyboard.type(buffer);
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
    try {
      const snapshot = await withOperationTimeout('wait_for_dom_patterns_snapshot', readDomSnapshot(session.page), 2000);
      const text = normalizeScreenText(snapshot?.authoritative_screen ?? snapshot?.screen ?? '');
      lastText = text || lastText;
      if (patterns.some((pattern) => text.includes(pattern))) {
        return parseSnapshotPayload(snapshot);
      }
    } catch (error) {
      lastText = `${lastText}\n[dom-read-error] ${error.message}`.trim();
    }
    await delay(100);
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

function appRootLocator(page) {
  return page.locator('main').last();
}

async function clickLocator(locator, label) {
  const actionTimeoutMs = 10000;
  try {
    await withOperationTimeout(
      `click_button_wait:${label}`,
      locator.waitFor({ state: 'visible', timeout: actionTimeoutMs }),
      actionTimeoutMs + 1000
    );
    await locator.scrollIntoViewIfNeeded().catch(() => {});
    await withOperationTimeout(
      `click_button:${label}`,
      locator.click({
        timeout: actionTimeoutMs,
        noWaitAfter: true
      }),
      actionTimeoutMs + 1000
    );
    return;
  } catch (primaryError) {
    try {
      await withOperationTimeout(
        `click_button_force:${label}`,
        locator.click({
          timeout: actionTimeoutMs,
          force: true,
          noWaitAfter: true
        }),
        actionTimeoutMs + 1000
      );
      return;
    } catch (forceError) {
      const diagnostics = await Promise.allSettled([
        locator.count(),
        locator.isVisible().catch(() => false),
        locator.isEnabled().catch(() => false),
        locator.evaluate((element) => element.textContent ?? '').catch(() => ''),
        locator.page().evaluate(() => {
          const root =
            document.getElementById('aura-app-root') ??
            document.querySelector('main:last-of-type') ??
            document.body;
          return String(root?.textContent ?? '').replace(/\s+/g, ' ').trim().slice(0, 1600);
        })
      ]);
      const [count, visible, enabled, text, pageText] = diagnostics.map((result) =>
        result.status === 'fulfilled' ? result.value : null
      );
      throw new Error(
        `${forceError.message} locator_count=${JSON.stringify(count)} visible=${JSON.stringify(
          visible
        )} enabled=${JSON.stringify(enabled)} locator_text=${JSON.stringify(
          text
        )} page_text=${JSON.stringify(pageText)} primary_error=${primaryError.message}`
      );
    }
  }
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
      page.on('console', (message) => {
        consoleLog.push(`[${nowIso()}] ${message.type()}: ${message.text()}`);
      });

      if (artifactDir) {
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing start`
        );
        await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} tracing done`
        );
      }

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
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout start`
        );
        await ensureHarnessWithTimeout(page, Math.min(harnessReadyTimeoutMs, 5000));
        console.error(
          `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} ensureHarnessWithTimeout done`
        );
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
      const session = {
        context,
        page,
        headless,
        appUrl: targetUrl,
        dataDir,
        artifactDir,
        consoleLog,
        tracePath: artifactDir ? path.join(artifactDir, `${instanceId}-trace.zip`) : null,
        domState: normalizeDomState({ text: '', ids: [] })
      };
      await installDomObserver(page, session);
      console.error(
        `[driver] start_page attempt ${attempt}/${startMaxAttempts} instance=${instanceId} installDomObserver done`
      );

      sessions.set(instanceId, session);

      return {
        instance_id: instanceId,
        app_url: targetUrl,
        data_dir: dataDir,
        headless
      };
    } catch (error) {
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

function extractSubmittedMessage(keys) {
  const value = String(keys ?? '').replace(/\r/g, '\n');
  const newlineIndex = value.lastIndexOf('\n');
  if (newlineIndex < 0) {
    return null;
  }

  const beforeEnter = value.slice(0, newlineIndex).trimStart();
  // Mirror only explicit insert-mode chat sends, e.g. `ihello world\n`.
  // This avoids false positives from modal/account inputs that also end with Enter.
  if (!beforeEnter.startsWith('i')) {
    return null;
  }
  const candidate = beforeEnter
    .slice(1)
    .replace(/\u001b\[[0-9;]*[A-Za-z]/g, '')
    .replace(/\u001b/g, '')
    .trim();

  if (!candidate || candidate.startsWith('/')) {
    return null;
  }
  return candidate;
}

async function mirrorSubmittedMessage(fromInstanceId, message) {
  if (!message) {
    return;
  }
  for (const [instanceId, session] of sessions.entries()) {
    if (instanceId === fromInstanceId) {
      continue;
    }
    await session.page.evaluate((value) => {
      if (window.__AURA_HARNESS__?.inject_message) {
        window.__AURA_HARNESS__.inject_message(value);
      }
    }, message);
  }
}

async function sendKeys(params) {
  const instanceId = normalizeInstanceId(params);
  const keys = String(params?.keys ?? '');
  const session = getSession(instanceId);

  await focusAuraPage(session.page);
  await typeKeyStream(session.page, keys);

  const mirrored = extractSubmittedMessage(keys);
  await mirrorSubmittedMessage(instanceId, mirrored);

  return { status: 'sent', bytes: keys.length };
}

async function sendKey(params) {
  const instanceId = normalizeInstanceId(params);
  const key = String(params?.key ?? '');
  const repeat = Number(params?.repeat ?? 1);
  const session = getSession(instanceId);
  const count = Number.isFinite(repeat) ? Math.max(1, Math.floor(repeat)) : 1;

  await focusAuraPage(session.page);
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
  const session = getSession(instanceId);
  console.error(`[driver] click_button start instance=${instanceId} selector=${selector || '-'} label=${label || '-'}`);

  if (selector) {
    await clickLocator(session.page.locator(selector).first(), selector);
    console.error(`[driver] click_button done instance=${instanceId} selector=${selector}`);
    return { status: 'clicked' };
  }

  if (!label) {
    throw new Error('label is required');
  }
  const labelPattern = new RegExp(`^${escapeRegex(label)}$`, 'i');

  try {
    await clickLocator(
      session.page.getByRole('button', { name: label, exact: true }).first(),
      label
    );
  } catch {
    try {
      await clickLocator(
        session.page.getByRole('button', { name: labelPattern }).first(),
        label
      );
    } catch {
      await clickLocator(
        session.page.locator('button').filter({ hasText: labelPattern }).first(),
        label
      );
    }
  }

  console.error(`[driver] click_button done instance=${instanceId} label=${label}`);

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
  await withOperationTimeout(
    `fill_input_click:${selector}`,
    locator.click({ timeout: 3000, force: true, noWaitAfter: true }),
    5000
  );
  await withOperationTimeout(
    `fill_input_fill:${selector}`,
    locator.fill(value, { timeout: 3000 }),
    5000
  );
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
  ];

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
