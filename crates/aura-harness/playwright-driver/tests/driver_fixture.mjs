import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { spawn } from 'node:child_process';

// This fixture owns one direct child and its RPC pipes, not browser descendants.
export function spawnDriver({
  command = process.execPath,
  args = [path.resolve('playwright_driver.mjs')],
  cwd,
} = {}) {
  const child = spawn(command, args, { cwd, stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  const failures = [];
  let requestId = 0;
  let closing = false;
  let exited = false;
  let closed = false;
  let closePromise;
  let terminalError;
  let protocolFailed = false;
  const stderrLimit = 8192;
  let stderrTail = Buffer.alloc(0);
  let stderrBytes = 0n;
  const diagnostics = () => Object.freeze({
    tail: Buffer.from(stderrTail), totalBytes: stderrBytes,
    truncated: stderrBytes > BigInt(stderrTail.length), limitBytes: stderrLimit,
  });
  let resolveExit;
  let resolveSettlement;
  const exit = new Promise((resolve) => { resolveExit = resolve; });
  const settled = new Promise((resolve) => { resolveSettlement = resolve; });
  const rejectPending = (error) => {
    for (const resolver of pending.values()) resolver.reject(error);
    pending.clear();
  };
  const fail = (error) => {
    protocolFailed = true;
    // A stream can deliver the same native Error to its callback and event.
    if (!failures.includes(error)) failures.push(error);
    rejectPending(error);
    closing = true;
    try { child.stdin.end(); } catch (cleanupError) {
      if (!failures.includes(cleanupError)) failures.push(cleanupError);
    }
  };
  const unavailable = () => {
    terminalError ??= new Error('driver fixture is closing or terminated');
    return failures[0] ?? terminalError;
  };
  // Observe from birth, including spawn refusal and exit before close() is called.
  child.on('error', fail);
  child.stdin.on('error', fail);
  child.stdout.on('error', fail);
  child.stderr.on('error', fail);
  // Drain continuously, retaining only a bounded byte tail (not full diagnostics).
  child.stderr.on('data', (chunk) => {
    stderrBytes += BigInt(chunk.length);
    stderrTail = chunk.length >= stderrLimit
      ? Buffer.from(chunk.subarray(chunk.length - stderrLimit))
      : Buffer.from(Buffer.concat([stderrTail, chunk]).subarray(-stderrLimit));
  });
  child.once('exit', (code, signal) => {
    exited = true;
    resolveExit({ code, signal });
    if (code !== 0 || signal !== null) {
      failures.push(Object.assign(new Error(`driver exited: code=${code} signal=${signal}`), { code, signal }));
    }
    // Exit is not stdout settlement: original final replies may still be buffered.
  });
  const rl = readline.createInterface({ input: child.stdout, crlfDelay: Infinity });
  rl.on('error', fail);
  rl.on('line', (line) => {
    if (protocolFailed) return;
    let response;
    try {
      response = JSON.parse(line);
      if (!response || !Number.isSafeInteger(response.id) || typeof response.ok !== 'boolean'
          || !pending.has(response.id)) {
        throw new Error('invalid driver RPC response');
      }
    } catch (error) { fail(error); return; }
    const resolver = pending.get(response.id);
    pending.delete(response.id);
    if (response.ok) resolver.resolve(response.result);
    else resolver.reject(new Error(String(response.error)));
  });
  child.stdout.once('end', () => {
    if (pending.size !== 0) fail(failures[0] ?? new Error('driver stdout ended with pending requests'));
  });
  child.once('close', (code, signal) => {
    closed = true;
    // Spawn refusal need not emit exit. close still acknowledges direct stdio settlement.
    if (!exited) resolveExit({ code, signal });
    if (pending.size !== 0) fail(unavailable());
    rl.close();
    resolveSettlement(Object.freeze({ code, signal, failures: Object.freeze([...failures]), stderr: diagnostics() }));
  });
  const call = (method, params = {}) => {
    if (closing || exited || closed) return Promise.reject(unavailable());
    const id = ++requestId;
    let payload;
    try { payload = JSON.stringify({ id, method, params }); } catch (error) {
      return Promise.reject(error);
    }
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      try {
        child.stdin.write(`${payload}\n`, (error) => { if (error) fail(error); });
      } catch (error) { fail(error); }
    });
  };
  const close = () => {
    if (closePromise) return closePromise;
    closing = true;
    try { child.stdin.end(); } catch (error) { fail(error); }
    closePromise = settled.then(() => {
      if (failures.length === 1) throw failures[0];
      if (failures.length > 1) {
        throw new AggregateError([...failures], 'driver fixture failed during settlement', { cause: failures[0] });
      }
    });
    return closePromise;
  };
  return { child, call, close, exit, settled, diagnostics };
}
