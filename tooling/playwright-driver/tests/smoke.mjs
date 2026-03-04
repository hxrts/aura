import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import process from 'node:process';
import readline from 'node:readline';
import { spawn } from 'node:child_process';

const fixtureHtml = `<!doctype html>
<html>
  <body>
    <script>
      (() => {
        const state = {
          screen: 'Neighborhood Chat Contacts Notifications Settings\\nContacts (0)',
          clipboard: '',
          logs: []
        };

        const pushLog = (line) => {
          state.logs.push(String(line));
        };

        window.__AURA_HARNESS__ = {
          send_keys(keys) {
            pushLog('send_keys:' + keys);
            if (keys.includes('c')) {
              state.clipboard = 'fixture-clipboard';
            }
            if (keys.includes('2')) {
              state.screen = 'Neighborhood Chat Contacts Notifications Settings\\nChannels';
            }
            if (keys.includes('hello')) {
              state.screen += '\\nhello';
            }
            return true;
          },
          send_key(key, repeat) {
            pushLog('send_key:' + key + ':' + repeat);
            return true;
          },
          snapshot() {
            return {
              screen: state.screen,
              raw_screen: state.screen,
              authoritative_screen: state.screen,
              normalized_screen: state.screen,
              capture_consistency: 'settled'
            };
          },
          read_clipboard() {
            return state.clipboard;
          },
          tail_log(lines) {
            return state.logs.slice(-lines);
          }
        };
      })();
    </script>
  </body>
</html>`;

function createServer() {
  return new Promise((resolve, reject) => {
    const server = http.createServer((req, res) => {
      res.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      res.end(fixtureHtml);
    });

    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (!address || typeof address === 'string') {
        reject(new Error('failed to bind server'));
        return;
      }
      resolve({
        server,
        url: `http://127.0.0.1:${address.port}`
      });
    });
  });
}

function spawnDriver() {
  const driverPath = path.resolve('playwright_driver.mjs');
  const child = spawn(process.execPath, [driverPath], {
    stdio: ['pipe', 'pipe', 'pipe']
  });

  const rl = readline.createInterface({ input: child.stdout, crlfDelay: Infinity });
  let requestId = 0;
  const pending = new Map();

  rl.on('line', (line) => {
    const response = JSON.parse(line);
    const resolver = pending.get(response.id);
    if (resolver) {
      pending.delete(response.id);
      if (response.ok) {
        resolver.resolve(response.result);
      } else {
        resolver.reject(new Error(String(response.error)));
      }
    }
  });

  const call = (method, params = {}) => {
    requestId += 1;
    const id = requestId;
    const payload = JSON.stringify({ id, method, params });
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      child.stdin.write(`${payload}\n`);
    });
  };

  const close = async () => {
    try {
      child.stdin.end();
    } finally {
      await new Promise((resolve) => child.once('exit', resolve));
    }
  };

  return { child, call, close };
}

async function main() {
  const { server, url } = await createServer();
  const driver = spawnDriver();
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'aura-playwright-smoke-'));

  try {
    await driver.call('start_page', {
      instance_id: 'smoke-a',
      app_url: url,
      data_dir: path.join(tempRoot, 'data'),
      artifact_dir: path.join(tempRoot, 'artifacts'),
      headless: true
    });

    await driver.call('send_keys', { instance_id: 'smoke-a', keys: '2hello' });

    const snapshot = await driver.call('snapshot', { instance_id: 'smoke-a', screenshot: false });
    assert.match(snapshot.screen, /Channels/);
    assert.match(snapshot.screen, /hello/);

    await driver.call('send_keys', { instance_id: 'smoke-a', keys: 'c' });
    const clipboard = await driver.call('read_clipboard', { instance_id: 'smoke-a' });
    assert.equal(clipboard.text, 'fixture-clipboard');

    const logs = await driver.call('tail_log', { instance_id: 'smoke-a', lines: 5 });
    assert.ok(Array.isArray(logs.lines));
    assert.ok(logs.lines.some((line) => line.includes('send_keys')));

    const stop = await driver.call('stop', { instance_id: 'smoke-a' });
    assert.equal(stop.status, 'stopped');

    console.log('playwright driver smoke test passed');
  } finally {
    server.close();
    await driver.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
