#!/usr/bin/env node

import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const driverRoot = path.dirname(fileURLToPath(import.meta.url));
const compiledDriver = path.join(driverRoot, 'dist', 'playwright_driver.js');

try {
  await import(pathToFileURL(compiledDriver).href);
} catch (error) {
  console.error(
    `[driver] failed to load compiled TypeScript driver at ${compiledDriver}. Run 'npm run build' in crates/aura-harness/playwright-driver.`
  );
  console.error(error?.stack ?? error?.message ?? String(error));
  process.exitCode = 1;
}
