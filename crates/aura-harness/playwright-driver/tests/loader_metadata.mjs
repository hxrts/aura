import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import childProcess from 'node:child_process';
import { syncBuiltinESMExports } from 'node:module';
import { compiledDriverIsStale, ensureCompiledDriverFresh } from '../driver_loader.mjs';

// Observe the real default spawn, never replace its child, events or result.
function observedDefaultCompile(options) {
  const originalSpawn = childProcess.spawn;
  const observed = { closed: false };
  let promise;
  childProcess.spawn = (...args) => {
    const child = originalSpawn(...args);
    child.once('error', (error) => { observed.nativeError = error; });
    child.once('exit', (code, signal) => { observed.exit = { code, signal }; });
    child.once('close', () => { observed.closed = true; });
    return child;
  };
  syncBuiltinESMExports();
  try { promise = ensureCompiledDriverFresh(options); }
  finally { childProcess.spawn = originalSpawn; syncBuiltinESMExports(); }
  return { promise, observed };
}

async function compilerProducerControls() {
  const parent = fs.mkdtempSync(path.join(os.tmpdir(), 'aura-native-compiler-'));
  try {
    const driverRoot = path.join(parent, 'driver');
    fs.mkdirSync(driverRoot);
    fs.mkdirSync(path.join(driverRoot, 'src'));
    fs.mkdirSync(path.join(driverRoot, 'dist'));
    for (const name of ['package.json', 'tsconfig.json']) {
      const file = path.join(driverRoot, name);
      fs.writeFileSync(file, '{}');
      fs.utimesSync(file, new Date(1000000), new Date(1000000));
    }
    const compilerPath = path.join(driverRoot, 'compiler.cjs');
    const options = { driverRoot, compilerPath, log: () => {} };
    fs.writeFileSync(compilerPath, 'process.exit(7);');
    const failed = observedDefaultCompile(options);
    await assert.rejects(failed.promise, (error) =>
      error.exitCode === 7 && error.signal === null);
    assert.equal(failed.observed.closed, true);
    assert.deepEqual(failed.observed.exit, { code: 7, signal: null });

    fs.writeFileSync(compilerPath, "process.kill(process.pid, 'SIGTERM');");
    const signalled = observedDefaultCompile(options);
    await assert.rejects(signalled.promise, (error) =>
      error.exitCode === null && error.signal === 'SIGTERM');
    assert.equal(signalled.observed.closed, true);
    assert.deepEqual(signalled.observed.exit, { code: null, signal: 'SIGTERM' });

    fs.writeFileSync(compilerPath, `const fs=require('node:fs');
fs.writeFileSync('dist/playwright_driver.js','// original native output');
fs.utimesSync('dist/playwright_driver.js',new Date(4000000),new Date(4000000));`);
    const success = observedDefaultCompile(options);
    assert.equal(await success.promise, path.join(driverRoot, 'dist', 'playwright_driver.js'));
    assert.equal(success.observed.closed, true);
    assert.deepEqual(success.observed.exit, { code: 0, signal: null });
    fs.unlinkSync(path.join(driverRoot, 'dist', 'playwright_driver.js'));

    const refused = observedDefaultCompile({ ...options, log: () => {
      fs.renameSync(driverRoot, path.join(parent, 'withdrawn-original-driver'));
    } });
    await assert.rejects(refused.promise, (error) =>
      error === refused.observed.nativeError && error.code === 'ENOENT');
    assert.equal(refused.observed.closed, true, 'spawn refusal waits for actual close');
    assert.equal(refused.observed.exit, undefined, 'refusal is not an invented process exit');
  } finally { fs.rmSync(parent, { recursive: true }); }
  console.log('default compiler native producer controls passed');
}

export async function loaderMetadataControls() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'aura-loader-metadata-'));
  try {
    const source = path.join(root, 'src');
    const compiled = path.join(root, 'dist', 'playwright_driver.js');
    fs.mkdirSync(path.dirname(compiled));
    fs.writeFileSync(compiled, '// existing artifact\n');
    for (const name of ['package.json', 'tsconfig.json']) {
      fs.writeFileSync(path.join(root, name), '{}');
      fs.utimesSync(path.join(root, name), new Date(1000000), new Date(1000000));
    }
    fs.utimesSync(compiled, new Date(2000000), new Date(2000000));
    fs.writeFileSync(source, 'not a directory');
    const check = () => compiledDriverIsStale({ driverRoot: root });
    assert.throws(check, (error) => error.code === 'ENOTDIR' && error.path === source);
    let compiledCount = 0;
    await assert.rejects(ensureCompiledDriverFresh({
      driverRoot: root, compileDriver: async () => { compiledCount += 1; },
    }), (error) => error.code === 'ENOTDIR' && error.path === source);
    assert.equal(compiledCount, 0, 'metadata refusal precedes compilation');
    fs.unlinkSync(source);
    assert.throws(check, (error) => error.code === 'ENOENT' && error.path === source);
    fs.mkdirSync(source);
    assert.equal(check(), false, 'readable empty source inventory retains freshness semantics');
    fs.unlinkSync(path.join(root, 'tsconfig.json'));
    assert.throws(check, (error) => error.code === 'ENOENT'
      && error.path === path.join(root, 'tsconfig.json'));
    fs.writeFileSync(path.join(root, 'tsconfig.json'), '{}');
    assert.throws(() => compiledDriverIsStale({
      driverRoot: root, compiledDriver: path.join(compiled, 'invalid-child'),
    }), (error) => error.code === 'ENOTDIR');
    fs.unlinkSync(compiled);
    assert.equal(check(), true, 'missing compiled artifact still requests a rebuild');
    const customConfig = path.join(root, 'selected-tsconfig.json');
    const compiler = path.join(root, 'compiler');
    fs.writeFileSync(customConfig, '{}');
    fs.writeFileSync(compiler, '// injected compiler precondition');
    fs.utimesSync(path.join(root, 'tsconfig.json'), new Date(1000000), new Date(1000000));
    await assert.rejects(ensureCompiledDriverFresh({
      driverRoot: root, tsconfigPath: customConfig, compilerPath: compiler,
      log: () => {},
      compileDriver: async () => {
        compiledCount += 1;
        fs.writeFileSync(compiled, '// rebuilt artifact');
        fs.utimesSync(compiled, new Date(2000000), new Date(2000000));
        fs.utimesSync(customConfig, new Date(3000000), new Date(3000000));
      },
    }), /rebuild completed.*still stale/);
    assert.equal(compiledCount, 1, 'post-build verification retains selected config input');
  } finally {
    fs.rmSync(root, { recursive: true });
  }
  await compilerProducerControls();
  console.log('driver loader metadata controls passed');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await loaderMetadataControls();
}
