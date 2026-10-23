import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { spawnDriver } from './driver_fixture.mjs';
import { createSmokeServer, withSmokeFixture } from './smoke_fixture.mjs';
import { loaderMetadataControls } from './loader_metadata.mjs';
import http from 'node:http';

import {
  compiledDriverIsStale,
  ensureCompiledDriverFresh,
} from '../driver_loader.mjs';

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function writeFile(targetPath, contents) {
  fs.mkdirSync(path.dirname(targetPath), { recursive: true });
  fs.writeFileSync(targetPath, contents);
}

async function withTempDriverRoot(fn) {
  const tempRoot = fs.mkdtempSync(
    path.join(os.tmpdir(), 'aura-playwright-driver-loader-'),
  );
  try {
    return await fn(tempRoot);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
}

const fixtureProgram = `
const readline = require('node:readline');
let held = false;
readline.createInterface({ input: process.stdin }).on('line', (line) => {
  const request = JSON.parse(line);
  const reply = (result) => process.stdout.write(JSON.stringify({id:request.id,ok:true,result})+'\\n');
  if (request.method === 'hold') held = true;
  else if (request.method === 'ack') reply(held);
  else if (request.method === 'fail') process.stderr.write('original-child-failure', () => process.exit(7));
  else if (request.method === 'final') process.stdout.write(JSON.stringify({id:request.id,ok:true,result:'final-result'})+'\\n', () => process.exit(0));
  else if (request.method === 'malformed') process.stdout.write('{invalid\\n');
  else if (request.method === 'stderr') process.stderr.write('x'.repeat(131064)+'TAIL-END', () => reply('drained'));
  else reply(request.params);
});
`;

async function withPausedStdout(driver, observe) {
  driver.child.stdout.pause();
  try { return await observe(); }
  finally { driver.child.stdout.resume(); }
}

async function lifecycleControls() {
  const detached = spawnSync(process.execPath, ['--input-type=module', '-e', 'async function main(){await new Promise(()=>{});} main().catch(()=>{});']);
  assert.equal(detached.error, undefined);
  assert.equal(detached.signal, null);
  assert.equal(detached.status, 0, 'detached unresolved entry can falsely exit clean');
  const structured = spawnSync(process.execPath, ['--input-type=module', '-e', 'async function main(){await new Promise(()=>{});} await main().catch(()=>{});']);
  assert.equal(structured.error, undefined);
  assert.equal(structured.signal, null);
  assert.equal(structured.status, 13, 'unresolved structured entry refuses clean exit');
  const drivers = [];
  const own = (driver) => { drivers.push(driver); return driver; };
  const start = () => own(spawnDriver({ args: ['-e', fixtureProgram] }));
  let failure;
  try {
    const early = own(spawnDriver({ args: ['-e', 'process.exit(0)'] }));
    await early.exit;
    const earlyClose = early.close();
    assert.equal(early.close(), earlyClose, 'close retains same original promise');
    await earlyClose;
    await assert.rejects(early.call('late'), /closing or terminated/);

    const buffered = start();
    let replied = false;
    let finalReply;
    await withPausedStdout(buffered, async () => {
      finalReply = buffered.call('final').then((value) => { replied = true; return value; });
      assert.equal((await buffered.exit).code, 0);
      assert.equal(replied, false, 'exit occurs while original stdout reply is still paused');
    });
    assert.equal(await finalReply, 'final-result');
    await buffered.close();

    const assertionFailure = new Error('controlled pre-resume assertion failure');
    const withdrawn = start();
    let withdrawnReply;
    await assert.rejects(withPausedStdout(withdrawn, async () => {
      withdrawnReply = withdrawn.call('final');
      await withdrawn.exit;
      throw assertionFailure;
    }), (error) => error === assertionFailure);
    assert.equal(await withdrawnReply, 'final-result', 'failed observation still resumes original pipe');
    await withdrawn.close();

    const normal = start();
    assert.deepEqual(await normal.call('echo', { typed: 'payload' }), { typed: 'payload' });
    assert.equal(await normal.call('stderr'), 'drained');
    await normal.close();
    const stderr = (await normal.settled).stderr;
    assert.equal(stderr.totalBytes, 131072n);
    assert.equal(stderr.tail.length, 8192);
    assert.equal(stderr.tail.toString(), 'x'.repeat(8184)+'TAIL-END');
    assert.equal(stderr.truncated, true);
    assert.equal(normal.close(), normal.close());

    const failed = start();
    const pending = failed.call('hold').catch((error) => error);
    assert.equal(await failed.call('ack'), true, 'actual child acknowledged pending request');
    const trigger = failed.call('fail').catch((error) => error);
    const original = await pending;
    assert.equal((await failed.exit).code, 7);
    const failedSettlement = await failed.settled;
    assert.equal(failedSettlement.stderr.tail.toString(), 'original-child-failure');
    assert.equal(failedSettlement.stderr.truncated, false);
    const causes = failedSettlement.failures;
    assert.ok(causes.some((cause) => cause.code === 7));
    assert.ok(original instanceof Error);
    assert.equal(await trigger, original, 'same failure settles every pending call');
    await assert.rejects(failed.close(), (error) => causes.length === 1
      ? error === original
      : error.cause === original && error.errors.length === causes.length
        && error.errors.every((cause, index) => cause === causes[index]));
    await assert.rejects(failed.call('late'), (error) => error === original);

    let refusedRoot;
    await withTempDriverRoot(async (scratch) => {
      refusedRoot = scratch;
      const missing = path.join(scratch, 'missing-driver');
      assert.equal(fs.existsSync(missing), false);
      const refused = own(spawnDriver({ command: missing }));
      try {
        const nativeError = new Promise((resolve) => refused.child.once('error', resolve));
        const refusedCall = refused.call('request').catch((error) => error);
        const native = await nativeError;
        assert.equal(native.code, 'ENOENT');
        assert.equal(await refusedCall, native, 'original spawn Error identity retained');
        await assert.rejects(refused.close(), (error) => error === native || error.cause === native);
      } finally { await refused.settled; }
    });
    assert.equal(fs.existsSync(refusedRoot), false, 'private refusal scratch retires after child settlement');

    const malformed = start();
    const parseError = await malformed.call('malformed').catch((error) => error);
    assert.ok(parseError instanceof SyntaxError);
    await assert.rejects(malformed.close(), (error) => error === parseError || error.cause === parseError);
    assert.equal(malformed.close(), malformed.close());
  } catch (error) { failure = error; }
  // Even an assertion failure closes every same original child before returning.
  const cleanup = await Promise.allSettled(drivers.map((driver) => driver.close()));
  if (failure) {
    const independent = cleanup.filter((result) => result.status === 'rejected')
      .map((result) => result.reason).filter((error) => error !== failure);
    if (independent.length) throw new AggregateError([failure, ...independent], 'fixture assertion and cleanup failures', { cause: failure });
    throw failure;
  }
  console.log('driver fixture lifecycle controls passed');
}

async function smokeResourceControls() {
  const options = { args: ['-e', fixtureProgram] };
  const blocker = createSmokeServer('occupied');
  const blockerUrl = await blocker.ready;
  try {
    const port = Number(new URL(blockerUrl).port);
    let observedFailure;
    const refused = createSmokeServer('refused', { port }, (error) => { observedFailure = error; });
    const native = new Promise((resolve) => refused.server.once('error', resolve));
    try {
      const original = await native;
      assert.equal(original.code, 'EADDRINUSE');
      assert.equal(observedFailure, original, 'listener failure is reported at actual native observation');
      await assert.rejects(refused.ready, (error) => error === original);
    } finally { await refused.close(); }
    assert.equal(refused.close(), refused.close());
    assert.equal(refused.server.listening, false);
  } finally { await blocker.close(); }

  await withTempDriverRoot(async (scratchParent) => {
    let scratch;
    let child;
    let listenerClosed = false;
    let closeAfterDriver;
    let scratchPresentAtListenerClose;
    const value = await withSmokeFixture('fixture-body', async ({ server, url, driver, tempRoot }) => {
      scratch = tempRoot;
      child = driver;
      server.once('close', () => {
        listenerClosed = true;
        closeAfterDriver = driver.child.stdout.closed && driver.child.stderr.closed;
        scratchPresentAtListenerClose = fs.existsSync(tempRoot);
      });
      const body = await new Promise((resolve, reject) => {
        http.get(url, (response) => {
          let text = '';
          response.on('data', (chunk) => { text += chunk; });
          response.on('error', reject);
          response.on('end', () => resolve(text));
        }).on('error', reject);
      });
      assert.equal(body, 'fixture-body');
      assert.deepEqual(await driver.call('echo', { value: 42 }), { value: 42 });
      writeFile(path.join(tempRoot, 'artifact'), 'owned scratch');
      return 42;
    }, { driverOptions: options, scratchParent });
    assert.equal(value, 42);
    assert.equal(listenerClosed, true, 'native server close event precedes retirement return');
    assert.equal(closeAfterDriver, true);
    assert.equal(scratchPresentAtListenerClose, true, 'scratch remains until native listener closes');
    assert.equal((await child.settled).code, 0);
    assert.equal(fs.existsSync(scratch), false);

    const socketPath = path.join(scratchParent, 'address-refusal.sock');
    const wrongAddress = createSmokeServer('unix-address', { port: socketPath });
    try {
      await assert.rejects(wrongAddress.ready, (error) =>
        error === wrongAddress.failures()[0] && /HTTP server address/.test(error.message));
    } finally { await wrongAddress.close(); }
    assert.equal(wrongAddress.server.listening, false);
    assert.equal(fs.existsSync(socketPath), false);

    const parentFile = path.join(scratchParent, 'not-a-directory');
    writeFile(parentFile, 'native acquisition refusal');
    const childProof = path.join(scratchParent, 'partial-child-ended');
    const partialProgram = "process.stdin.resume();process.stdin.on('end',()=>require('node:fs').writeFileSync(process.argv[1],'ended'));";
    const available = createSmokeServer('selected-port');
    const selectedPort = Number(new URL(await available.ready).port);
    await available.close();
    await assert.rejects(withSmokeFixture('partial', () => {
      assert.fail('operation cannot run after scratch acquisition refusal');
    }, {
      serverOptions: { port: selectedPort },
      driverOptions: { args: ['-e', partialProgram, childProof] },
      scratchParent: parentFile,
    }), (error) => error.code === 'ENOTDIR');
    assert.equal(fs.readFileSync(childProof, 'utf8'), 'ended', 'partial acquisition still settles original child');
    const reopened = createSmokeServer('released-port', { port: selectedPort });
    try { await reopened.ready; } finally { await reopened.close(); }

    const operationOnly = new Error('original operation-only failure');
    let failedScratch;
    await assert.rejects(withSmokeFixture('operation-failure', async ({ tempRoot }) => {
      failedScratch = tempRoot;
      throw operationOnly;
    }, { driverOptions: options, scratchParent }), (error) => error === operationOnly);
    assert.equal(fs.existsSync(failedScratch), false);

    const primary = new Error('original smoke operation failure');
    let withdrawnScratch;
    await assert.rejects(withSmokeFixture('failure', async ({ tempRoot }) => {
      // Actual namespace withdrawal makes the owner's original lstat fail natively.
      withdrawnScratch = tempRoot;
      fs.rmSync(tempRoot, { recursive: true });
      throw primary;
    }, { driverOptions: options, scratchParent }), (error) =>
      error instanceof AggregateError && error.cause === primary
        && error.errors[0] === primary && error.errors[1].code === 'ENOENT'
        && error.errors[1].path === withdrawnScratch);
  });
  console.log('smoke HTTP/scratch ownership controls passed');
}

async function main() {
  await loaderMetadataControls();
  await smokeResourceControls();
  await lifecycleControls();
  let pendingRoot;
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const pending = withTempDriverRoot(async (driverRoot) => {
    pendingRoot = driverRoot;
    await gate;
    assert.equal(fs.existsSync(driverRoot), true);
    writeFile(path.join(driverRoot, 'completed'), 'settled\n');
    return 42;
  });
  assert.equal(fs.existsSync(pendingRoot), true, 'pending callback retains its scratch root');
  release();
  assert.equal(await pending, 42);
  assert.equal(fs.existsSync(pendingRoot), false, 'settled callback retires its scratch root');

  let failedRoot;
  const originalError = new Error('controlled asynchronous callback failure');
  await assert.rejects(withTempDriverRoot(async (driverRoot) => {
    failedRoot = driverRoot;
    await Promise.resolve();
    assert.equal(fs.existsSync(driverRoot), true);
    throw originalError;
  }), (error) => error === originalError);
  assert.equal(fs.existsSync(failedRoot), false, 'failed callback retires its scratch root');

  await withTempDriverRoot(async (driverRoot) => {
    const sourcePath = path.join(driverRoot, 'src', 'playwright_driver.ts');
    const compiledPath = path.join(driverRoot, 'dist', 'playwright_driver.js');
    const tsconfigPath = path.join(driverRoot, 'tsconfig.json');
    const packageJsonPath = path.join(driverRoot, 'package.json');
    const compilerPath = path.join(driverRoot, 'node_modules', 'typescript', 'bin', 'tsc');

    writeFile(compiledPath, 'export const version = 1;\n');
    await delay(20);
    writeFile(sourcePath, 'export const version = 2;\n');
    writeFile(tsconfigPath, '{}\n');
    writeFile(packageJsonPath, '{}\n');
    writeFile(compilerPath, '#!/usr/bin/env node\n');

    assert.equal(
      compiledDriverIsStale({ driverRoot, compiledDriver: compiledPath }),
      true,
      'newer TypeScript sources should make the compiled driver stale',
    );

    let compileCount = 0;
    await ensureCompiledDriverFresh({
      driverRoot,
      compiledDriver: compiledPath,
      compilerPath,
      compileDriver: async ({ compiledDriver }) => {
        compileCount += 1;
        await delay(20);
        writeFile(compiledDriver, 'export const version = 2;\n');
      },
      log: () => {},
    });
    assert.equal(
      compileCount,
      1,
      'stale driver artifacts should force a rebuild before launch',
    );
    assert.equal(
      compiledDriverIsStale({ driverRoot, compiledDriver: compiledPath }),
      false,
      'a rebuilt driver artifact should no longer be stale',
    );
  });

  await withTempDriverRoot(async (driverRoot) => {
    const sourcePath = path.join(driverRoot, 'src', 'playwright_driver.ts');
    const compiledPath = path.join(driverRoot, 'dist', 'playwright_driver.js');
    const tsconfigPath = path.join(driverRoot, 'tsconfig.json');
    const packageJsonPath = path.join(driverRoot, 'package.json');
    const compilerPath = path.join(driverRoot, 'node_modules', 'typescript', 'bin', 'tsc');

    writeFile(sourcePath, 'export const version = 2;\n');
    writeFile(tsconfigPath, '{}\n');
    writeFile(packageJsonPath, '{}\n');
    writeFile(compilerPath, '#!/usr/bin/env node\n');
    await delay(20);
    writeFile(compiledPath, 'export const version = 2;\n');

    let compileCount = 0;
    await ensureCompiledDriverFresh({
      driverRoot,
      compiledDriver: compiledPath,
      compilerPath,
      compileDriver: async () => {
        compileCount += 1;
      },
      log: () => {},
    });
    assert.equal(
      compileCount,
      0,
      'fresh compiled driver artifacts should launch without an unnecessary rebuild',
    );
  });

  console.log('playwright driver launcher freshness test passed');
}

await main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
