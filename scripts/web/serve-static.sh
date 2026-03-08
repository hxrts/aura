#!/usr/bin/env bash
set -euo pipefail

port="${1:-4173}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
web_root="$repo_root/crates/aura-web"

cd "$web_root"
if [ ! -d node_modules ]; then
    npm ci
fi
npm run tailwind:build >/dev/null
NO_COLOR=true ../../scripts/web/dx.sh build --release --platform web --package aura-web --bin aura-web --features web

public_dir="$repo_root/target/dx/aura-web/release/web/public"
if [[ ! -f "$public_dir/index.html" ]]; then
    NO_COLOR=true ../../scripts/web/dx.sh build --platform web --package aura-web --bin aura-web --features web
    public_dir="$repo_root/target/dx/aura-web/debug/web/public"
fi

if [[ ! -f "$public_dir/index.html" ]]; then
    echo "[serve-web-static] expected build output at $public_dir/index.html" >&2
    exit 1
fi

node_bin="$(command -v node || true)"
if [[ -z "$node_bin" ]]; then
    echo "[serve-web-static] node not found" >&2
    exit 1
fi

exec "$node_bin" -e '
const http = require("http");
const fs = require("fs");
const path = require("path");
const publicDir = process.argv[1];
const port = Number(process.argv[2]);
const host = "127.0.0.1";
const mime = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "application/javascript; charset=utf-8"],
  [".mjs", "application/javascript; charset=utf-8"],
  [".css", "text/css; charset=utf-8"],
  [".json", "application/json; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".svg", "image/svg+xml"],
  [".png", "image/png"],
  [".jpg", "image/jpeg"],
  [".jpeg", "image/jpeg"],
  [".ico", "image/x-icon"],
  [".txt", "text/plain; charset=utf-8"]
]);
http.createServer((req, res) => {
  const reqPath = new URL(req.url, `http://${host}:${port}`).pathname;
  const resolved = path.normalize(decodeURIComponent(reqPath)).replace(/^(\.\.[/\\])+/, "");
  let filePath = path.join(publicDir, resolved);
  if (reqPath.endsWith("/")) filePath = path.join(filePath, "index.html");
  fs.stat(filePath, (statErr, stats) => {
    if (!statErr && stats.isDirectory()) filePath = path.join(filePath, "index.html");
    fs.readFile(filePath, (readErr, data) => {
      if (readErr) {
        const fallback = path.join(publicDir, "index.html");
        fs.readFile(fallback, (fallbackErr, fallbackData) => {
          if (fallbackErr) {
            res.writeHead(404);
            res.end("Not found");
            return;
          }
          res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
          res.end(fallbackData);
        });
        return;
      }
      const ext = path.extname(filePath).toLowerCase();
      res.writeHead(200, { "Content-Type": mime.get(ext) || "application/octet-stream" });
      res.end(data);
    });
  });
}).listen(port, host, () => {
  process.stdout.write(`[serve-web-static] serving ${publicDir} on http://${host}:${port}\n`);
});
' "$public_dir" "$port"
