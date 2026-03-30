#!/usr/bin/env bash
set -euo pipefail

port="${1:-4173}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
web_root="$repo_root/crates/aura-web"
build_profile="${AURA_HARNESS_WEB_BUILD_PROFILE:-release}"
dioxus_config="$web_root/Dioxus.toml"
config_backup=""

restore_dioxus_config() {
    if [[ -n "$config_backup" && -f "$config_backup" ]]; then
        cp "$config_backup" "$dioxus_config"
        rm -f "$config_backup"
        config_backup=""
    fi
}

cd "$web_root"
if [ ! -d node_modules ] || [ ! -d node_modules/ws ]; then
    npm ci
fi
npm run tailwind:build >/dev/null
mkdir -p \
    "$repo_root/target/dx/aura-web/release/web/public/assets" \
    "$repo_root/target/dx/aura-web/release/web/public/fonts" \
    "$repo_root/target/dx/aura-web/debug/web/public/assets" \
    "$repo_root/target/dx/aura-web/debug/web/public/fonts"
# Symlink CSS so changes are immediately visible without rebuild
source_css="$web_root/public/assets/tailwind.css"
for profile in debug release; do
    target_css="$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
    rm -f "$target_css"
    ln -s "$source_css" "$target_css"
done

# Check if any web-relevant source is newer than the build output.
# If so, clear the dx cache so the next build picks up changes.
web_sources_stale() {
    local build_output="$1"
    if [[ ! -f "$build_output" ]]; then
        return 0
    fi
    local newest_src
    newest_src="$(find "$repo_root/crates/aura-web/src" "$repo_root/crates/aura-ui/src" "$repo_root/crates/aura-app/src" -name '*.rs' -newer "$build_output" -print -quit 2>/dev/null || true)"
    [[ -n "$newest_src" ]]
}

for profile in debug release; do
    build_output="$repo_root/target/dx/aura-web/$profile/web/public/index.html"
    if web_sources_stale "$build_output"; then
        echo "[serve-web-static] source files changed, clearing $profile dx cache"
        rm -rf "$repo_root/target/dx/aura-web/$profile"
        mkdir -p "$repo_root/target/dx/aura-web/$profile/web/public/assets" \
                 "$repo_root/target/dx/aura-web/$profile/web/public/fonts"
        rm -f "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
        ln -s "$source_css" "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
    fi
done

if [[ -f "$dioxus_config" ]]; then
    config_backup="$(mktemp)"
    cp "$dioxus_config" "$config_backup"
    perl -0pi -e 's/^hot_reload = true$/hot_reload = false/m' "$dioxus_config"
    perl -0pi -e 's/^reload_html = true$/reload_html = false/m' "$dioxus_config"
    trap restore_dioxus_config EXIT
fi

case "$build_profile" in
    release)
        public_dir="$repo_root/target/dx/aura-web/release/web/public"
        if [[ ! -f "$public_dir/index.html" ]]; then
            NO_COLOR=true ../../scripts/web/dx.sh build --release --platform web --package aura-web --bin aura-web --features web
        else
            echo "[serve-web-static] reusing prebuilt release web assets at $public_dir"
        fi
        if [[ ! -f "$public_dir/index.html" ]]; then
            public_dir="$repo_root/target/dx/aura-web/debug/web/public"
            if [[ ! -f "$public_dir/index.html" ]]; then
                NO_COLOR=true ../../scripts/web/dx.sh build --platform web --package aura-web --bin aura-web --features web
            else
                echo "[serve-web-static] reusing prebuilt debug web assets at $public_dir"
            fi
        fi
        ;;
    debug)
        public_dir="$repo_root/target/dx/aura-web/debug/web/public"
        if [[ ! -f "$public_dir/index.html" ]]; then
            NO_COLOR=true ../../scripts/web/dx.sh build --platform web --package aura-web --bin aura-web --features web
        else
            echo "[serve-web-static] reusing prebuilt debug web assets at $public_dir"
        fi
        ;;
    *)
        echo "[serve-web-static] unsupported AURA_HARNESS_WEB_BUILD_PROFILE=$build_profile" >&2
        exit 1
        ;;
esac

restore_dioxus_config
trap - EXIT

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
const { WebSocketServer } = require("ws");
const publicDir = process.argv[1];
const port = Number(process.argv[2]);
const host = "127.0.0.1";
const TRANSPORT_POLL_PATH = "/__aura_harness_transport__/poll";
const TRANSPORT_ENQUEUE_PATH = "/__aura_harness_transport__/enqueue";
const DEBUG_EVENT_PATH = "/__aura_harness_debug__/event";
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
const transportQueues = new Map();

function queueKey(authority, deviceId) {
  return `${authority}::${deviceId || ""}`;
}

function enqueueTransportEnvelope(authority, deviceId, envelopeB64) {
  const key = queueKey(authority, deviceId);
  const queue = transportQueues.get(key) || [];
  queue.push(envelopeB64);
  transportQueues.set(key, queue);
  process.stdout.write(
    `[serve-web-static] transport enqueue authority=${authority} device=${deviceId || "<any>"} depth=${queue.length}\n`,
  );
}

function drainTransportEnvelopes(authority, deviceId) {
  const drained = [];
  const wildcardKey = queueKey(authority, "");
  const exactKey = queueKey(authority, deviceId || "");
  for (const key of new Set([wildcardKey, exactKey])) {
    const queue = transportQueues.get(key);
    if (!queue || queue.length === 0) {
      continue;
    }
    drained.push(...queue);
    transportQueues.delete(key);
  }
  if (drained.length > 0) {
    process.stdout.write(
      `[serve-web-static] transport drain authority=${authority} device=${deviceId || "<any>"} count=${drained.length}\n`,
    );
  }
  return drained;
}

function handleTransportEnvelopeMessage(message, onSuccess, onError) {
  if (message?.kind !== "transport_envelope") {
    onError(`unsupported harness transport kind: ${message?.kind ?? "<missing>"}`);
    return;
  }

  if (
    typeof message.destination !== "string" ||
    message.destination.length === 0 ||
    typeof message.envelope_b64 !== "string" ||
    message.envelope_b64.length === 0
  ) {
    onError("malformed harness transport envelope");
    return;
  }

  const destinationDeviceId =
    typeof message.destination_device_id === "string" &&
    message.destination_device_id.length > 0
      ? message.destination_device_id
      : "";
  enqueueTransportEnvelope(
    message.destination,
    destinationDeviceId,
    message.envelope_b64,
  );
  onSuccess();
}

const server = http.createServer((req, res) => {
  const reqPath = new URL(req.url, `http://${host}:${port}`).pathname;
  if (req.method === "GET" && reqPath === TRANSPORT_POLL_PATH) {
    const requestUrl = new URL(req.url, `http://${host}:${port}`);
    const authority = requestUrl.searchParams.get("authority");
    const deviceId = requestUrl.searchParams.get("device");
    if (!authority) {
      res.writeHead(400, { "Content-Type": "application/json; charset=utf-8" });
      res.end(JSON.stringify({ error: "missing authority" }));
      return;
    }
    const envelopes = drainTransportEnvelopes(authority, deviceId);
    process.stdout.write(
      `[serve-web-static] transport poll authority=${authority} device=${deviceId || "<any>"} count=${envelopes.length}\n`,
    );
    res.writeHead(200, {
      "Content-Type": "application/json; charset=utf-8",
      "Cache-Control": "no-store",
    });
    res.end(JSON.stringify({ envelopes }));
    return;
  }
  if (req.method === "POST" && reqPath === TRANSPORT_ENQUEUE_PATH) {
    let raw = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      raw += chunk;
    });
    req.on("end", () => {
      let message;
      try {
        message = JSON.parse(raw);
      } catch (error) {
        console.error(`[serve-web-static] invalid harness transport payload: ${error?.message ?? String(error)}`);
        res.writeHead(400, { "Content-Type": "application/json; charset=utf-8" });
        res.end(JSON.stringify({ error: "invalid-json" }));
        return;
      }

      handleTransportEnvelopeMessage(
        message,
        () => {
          res.writeHead(204, { "Cache-Control": "no-store" });
          res.end();
        },
        (error) => {
          console.error(`[serve-web-static] ${error}`);
          res.writeHead(400, { "Content-Type": "application/json; charset=utf-8" });
          res.end(JSON.stringify({ error }));
        },
      );
    });
    return;
  }
  if (req.method === "GET" && reqPath === DEBUG_EVENT_PATH) {
    const requestUrl = new URL(req.url, `http://${host}:${port}`);
    const event = requestUrl.searchParams.get("event") || "<missing>";
    const detail = requestUrl.searchParams.get("detail") || "";
    process.stdout.write(
      `[serve-web-static] debug event=${event} detail=${detail}\n`,
    );
    res.writeHead(204, { "Cache-Control": "no-store" });
    res.end();
    return;
  }
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
});

const websocketServer = new WebSocketServer({ noServer: true });

websocketServer.on("connection", (socket) => {
  socket.on("message", (raw, isBinary) => {
    let payload = raw;
    if (isBinary && Buffer.isBuffer(raw)) {
      payload = raw.toString("utf8");
    } else if (typeof raw !== "string") {
      payload = Buffer.from(raw).toString("utf8");
    }

    let message;
    try {
      message = JSON.parse(payload);
    } catch (error) {
      console.error(`[serve-web-static] invalid harness transport payload: ${error?.message ?? String(error)}`);
      socket.close(1003, "invalid-json");
      return;
    }

    handleTransportEnvelopeMessage(
      message,
      () => {
        socket.send(JSON.stringify({ ok: true }));
        socket.close(1000, "queued");
      },
      (error) => {
        console.error(`[serve-web-static] ${error}`);
        socket.close(1003, "invalid-envelope");
      },
    );
  });
});

server.on("upgrade", (req, socket, head) => {
  websocketServer.handleUpgrade(req, socket, head, (ws) => {
    websocketServer.emit("connection", ws, req);
  });
});

server.listen(port, host, () => {
  process.stdout.write(`[serve-web-static] serving ${publicDir} on http://${host}:${port}\n`);
});
' "$public_dir" "$port" "${2:-}"
