import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

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

function withTempDriverRoot(fn) {
  const tempRoot = fs.mkdtempSync(
    path.join(os.tmpdir(), 'aura-playwright-driver-loader-'),
  );
  try {
    return fn(tempRoot);
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
  }
}

async function main() {
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

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
