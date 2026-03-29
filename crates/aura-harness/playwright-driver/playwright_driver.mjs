#!/usr/bin/env node

import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { ensureCompiledDriverFresh } from './driver_loader.mjs';

const driverRoot = path.dirname(fileURLToPath(import.meta.url));

try {
  const compiledDriver = await ensureCompiledDriverFresh({ driverRoot });
  await import(pathToFileURL(compiledDriver).href);
} catch (error) {
  console.error(
    `[driver] failed to load Playwright TypeScript driver from ${driverRoot}`
  );
  console.error(error?.stack ?? error?.message ?? String(error));
  process.exitCode = 1;
}
