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

async function ensureHarnessWithTimeout(page, timeoutMs) {
  await page.waitForFunction(() => {
    const bridge = window.__AURA_HARNESS__;
    return bridge && typeof bridge.snapshot === 'function';
  }, null, { timeout: timeoutMs });
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
      context = await chromium.launchPersistentContext(dataDir, {
        headless,
        viewport: { width: 1280, height: 900 },
        ignoreHTTPSErrors: true
      });

      const page = context.pages()[0] ?? (await context.newPage());
      page.on('console', (message) => {
        consoleLog.push(`[${nowIso()}] ${message.type()}: ${message.text()}`);
      });

      if (artifactDir) {
        await context.tracing.start({ screenshots: true, snapshots: true, sources: true });
      }

      await page.goto(targetUrl, { waitUntil: 'domcontentloaded', timeout: pageGotoTimeoutMs });
      await ensureHarnessWithTimeout(page, harnessReadyTimeoutMs);

      sessions.set(instanceId, {
        context,
        page,
        headless,
        appUrl: targetUrl,
        dataDir,
        artifactDir,
        consoleLog,
        tracePath: artifactDir ? path.join(artifactDir, `${instanceId}-trace.zip`) : null
      });

      return {
        instance_id: instanceId,
        app_url: targetUrl,
        data_dir: dataDir,
        headless
      };
    } catch (error) {
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

  await session.page.evaluate((value) => {
    window.__AURA_HARNESS__.send_keys(value);
  }, keys);

  const mirrored = extractSubmittedMessage(keys);
  await mirrorSubmittedMessage(instanceId, mirrored);

  return { status: 'sent', bytes: keys.length };
}

async function sendKey(params) {
  const instanceId = normalizeInstanceId(params);
  const key = String(params?.key ?? '');
  const repeat = Number(params?.repeat ?? 1);
  const session = getSession(instanceId);

  await session.page.evaluate(
    (payload) => {
      window.__AURA_HARNESS__.send_key(payload.key, payload.repeat);
    },
    {
      key,
      repeat: Number.isFinite(repeat) ? Math.max(1, Math.floor(repeat)) : 1
    }
  );

  return { status: 'sent' };
}

async function snapshot(params) {
  const instanceId = normalizeInstanceId(params);
  const session = getSession(instanceId);
  const screenshot = params?.screenshot !== false;

  const payload = await session.page.evaluate(() => window.__AURA_HARNESS__.snapshot());
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
    case 'snapshot':
      return snapshot(params);
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
        const result = await dispatch(request.method, request.params ?? {});
        writeResponse(jsonResponse(id, true, result));
      } catch (error) {
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
