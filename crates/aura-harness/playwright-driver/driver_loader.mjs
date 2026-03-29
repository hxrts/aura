import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { spawn } from 'node:child_process';

function statMtimeMs(targetPath) {
  try {
    return fs.statSync(targetPath).mtimeMs;
  } catch {
    return null;
  }
}

function collectTypeScriptInputs(rootDir, entries = []) {
  let children = [];
  try {
    children = fs.readdirSync(rootDir, { withFileTypes: true });
  } catch {
    return entries;
  }

  for (const entry of children) {
    const entryPath = path.join(rootDir, entry.name);
    if (entry.isDirectory()) {
      collectTypeScriptInputs(entryPath, entries);
      continue;
    }
    if (entry.isFile() && entry.name.endsWith('.ts')) {
      entries.push(entryPath);
    }
  }

  return entries;
}

export function newestInputMtimeMs({
  driverRoot,
  sourceRoot = path.join(driverRoot, 'src'),
  extraInputs = [],
}) {
  const inputs = [
    ...collectTypeScriptInputs(sourceRoot),
    ...extraInputs.filter(Boolean),
  ];
  let newestMtimeMs = null;
  for (const inputPath of inputs) {
    const mtimeMs = statMtimeMs(inputPath);
    if (mtimeMs == null) {
      continue;
    }
    newestMtimeMs =
      newestMtimeMs == null ? mtimeMs : Math.max(newestMtimeMs, mtimeMs);
  }
  return newestMtimeMs;
}

export function compiledDriverIsStale({
  driverRoot,
  sourceRoot = path.join(driverRoot, 'src'),
  compiledDriver = path.join(driverRoot, 'dist', 'playwright_driver.js'),
  extraInputs = [
    path.join(driverRoot, 'tsconfig.json'),
    path.join(driverRoot, 'package.json'),
  ],
}) {
  const compiledMtimeMs = statMtimeMs(compiledDriver);
  if (compiledMtimeMs == null) {
    return true;
  }

  const newestInput = newestInputMtimeMs({
    driverRoot,
    sourceRoot,
    extraInputs,
  });
  if (newestInput == null) {
    return false;
  }
  return newestInput > compiledMtimeMs;
}

function defaultCompileDriver({
  driverRoot,
  tsconfigPath,
  compilerPath,
}) {
  return new Promise((resolve, reject) => {
    const child = spawn(
      process.execPath,
      [compilerPath, '-p', tsconfigPath],
      {
        cwd: driverRoot,
        stdio: 'inherit',
      },
    );
    child.once('error', reject);
    child.once('exit', (code, signal) => {
      if (code === 0) {
        resolve();
        return;
      }
      reject(
        new Error(
          `TypeScript driver build failed code=${code ?? 'null'} signal=${signal ?? 'null'}`,
        ),
      );
    });
  });
}

export async function ensureCompiledDriverFresh({
  driverRoot,
  sourceRoot = path.join(driverRoot, 'src'),
  compiledDriver = path.join(driverRoot, 'dist', 'playwright_driver.js'),
  tsconfigPath = path.join(driverRoot, 'tsconfig.json'),
  compilerPath = path.join(driverRoot, 'node_modules', 'typescript', 'bin', 'tsc'),
  log = console.error,
  compileDriver = defaultCompileDriver,
}) {
  const stale = compiledDriverIsStale({
    driverRoot,
    sourceRoot,
    compiledDriver,
    extraInputs: [tsconfigPath, path.join(driverRoot, 'package.json')],
  });
  if (!stale) {
    return compiledDriver;
  }

  if (!fs.existsSync(compilerPath)) {
    throw new Error(
      `Playwright driver compiler not found at ${compilerPath}; run 'npm install' in ${driverRoot}`,
    );
  }

  log(
    `[driver] refreshing compiled TypeScript driver at ${compiledDriver} because source inputs are newer or the artifact is missing`,
  );
  await compileDriver({
    driverRoot,
    sourceRoot,
    compiledDriver,
    tsconfigPath,
    compilerPath,
  });

  if (compiledDriverIsStale({ driverRoot, sourceRoot, compiledDriver })) {
    throw new Error(
      `Playwright driver rebuild completed but ${compiledDriver} is still stale`,
    );
  }

  return compiledDriver;
}
