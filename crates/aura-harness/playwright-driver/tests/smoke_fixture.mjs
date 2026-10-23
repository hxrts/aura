import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { spawnDriver } from './driver_fixture.mjs';

// Fixture-only listener custody. close() observes the actual native close callback.
export function createSmokeServer(html, { port = 0, host = '127.0.0.1' } = {}, recordFailure = () => {}) {
  const server = http.createServer((_request, response) => {
    response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
    response.end(html);
  });
  const failures = [];
  let everListening = false;
  let bindFinished = false;
  let rejectBind;
  let closePromise;
  const record = (error) => {
    if (!failures.includes(error)) { failures.push(error); recordFailure(error); }
  };
  server.on('error', (error) => {
    record(error);
    if (!bindFinished) { bindFinished = true; rejectBind(error); }
  });
  const ready = new Promise((resolve, reject) => {
    rejectBind = reject;
    try {
      server.listen(port, host, () => {
        everListening = true;
        bindFinished = true;
        const address = server.address();
        if (!address || typeof address === 'string') {
          const error = new Error('failed to bind smoke HTTP server address');
          record(error);
          reject(error);
        } else resolve(`http://${host}:${address.port}`);
      });
    } catch (error) { record(error); bindFinished = true; reject(error); }
  });
  const close = () => {
    closePromise ??= ready.catch(() => {}).then(() => new Promise((resolve, reject) => {
      try {
        server.close((error) => {
          // A refused listen acquired no listener; the callback explicitly confirms it.
          if (error && !(error.code === 'ERR_SERVER_NOT_RUNNING' && !everListening)) {
            record(error);
            reject(error);
          } else resolve();
        });
      } catch (error) { record(error); reject(error); }
    }));
    return closePromise;
  };
  return { server, ready, close, failures: () => [...failures] };
}

// One smoke acquisition/operation/retirement sequence, not a general resource manager.
export async function withSmokeFixture(html, operation, {
  serverOptions,
  driverOptions,
  scratchParent = os.tmpdir(),
} = {}) {
  let listener;
  let driver;
  let tempRoot;
  let scratchIdentity;
  let driverSettled = false;
  let listenerSettled = false;
  let result;
  const failures = [];
  const record = (error) => { if (!failures.includes(error)) failures.push(error); };
  try {
    listener = createSmokeServer(html, serverOptions, record);
    const url = await listener.ready;
    driver = spawnDriver(driverOptions);
    tempRoot = fs.mkdtempSync(path.join(scratchParent, 'aura-playwright-smoke-'));
    scratchIdentity = fs.lstatSync(tempRoot);
    result = await operation({ server: listener.server, url, driver, tempRoot });
  } catch (error) { record(error); }
  finally {
    if (driver) {
      try { await driver.close(); } catch (error) {
        const terminal = await driver.settled;
        if (terminal.failures.length) {
          for (const original of terminal.failures) record(original);
        } else record(error);
      }
      await driver.settled;
      driverSettled = true;
    }
    if (listener) {
      try { await listener.close(); listenerSettled = true; } catch (error) { record(error); }
      for (const error of listener.failures()) record(error);
    }
    if (tempRoot && (!driverSettled || !listenerSettled)) {
      record(new Error(`smoke scratch retained without child/listener settlement: ${tempRoot}`));
    } else if (tempRoot) {
      try {
        const current = fs.lstatSync(tempRoot);
        if (!scratchIdentity || !current.isDirectory()
            || current.dev !== scratchIdentity.dev || current.ino !== scratchIdentity.ino) {
          throw new Error('smoke scratch identity changed; refusing cleanup');
        }
        fs.rmSync(tempRoot, { recursive: true });
      } catch (error) { record(error); }
    }
  }
  if (failures.length) {
    if (driver) console.error('driver fixture stderr (bounded tail):', driver.diagnostics());
    if (failures.length === 1) throw failures[0];
    throw new AggregateError(failures, 'smoke fixture operation and retirement failed', { cause: failures[0] });
  }
  return result;
}
