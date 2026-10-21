#!/usr/bin/env bash
# Build and serve the aura-web static site on a configurable port.
set -euo pipefail

port="${1:-4173}"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
web_root="$repo_root/crates/aura-web"
build_profile="${AURA_HARNESS_WEB_BUILD_PROFILE:-release}"
prebuilt_only="${AURA_HARNESS_WEB_PREBUILT_ONLY:-0}"
source "$script_dir/prebuilt-provenance.sh"
source "$script_dir/prebuilt-transfer.sh"
[[ "$prebuilt_only" == 0 || "$prebuilt_only" == 1 ]] || {
    echo 'AURA_HARNESS_WEB_PREBUILT_ONLY must be 0 or 1' >&2; exit 2;
}
dioxus_config="$web_root/Dioxus.toml"
config_backup=""
config_backup_dir=""
config_backup_ready=0
config_mutated=0

source "$script_dir/log-bootstrap.sh"
aura_web_redirect_logs "$repo_root" "$repo_root/artifacts/aura-web/serve-static-${port}.log"

restore_dioxus_config() {
    local stage="" status=0
    if [[ "$config_mutated" == 1 ]]; then
        # Only a completed backup can authorize replacing the live config.
        [[ "$config_backup_ready" == 1 ]] || {
            echo "[serve-web-static] incomplete backup cannot restore $dioxus_config" >&2
            return 1
        }
        stage="$(mktemp "$web_root/.Dioxus.toml.restore.XXXXXXXX")" || return $?
        if cp -p "$config_backup" "$stage"; then
            if mv -f "$stage" "$dioxus_config"; then
                config_mutated=0
            else
                status=$?
            fi
        else
            status=$?
        fi
        if [[ "$status" != 0 ]]; then
            rm -f "$stage" || echo "[serve-web-static] restore staging remains at $stage" >&2
            echo "[serve-web-static] config restoration refused; original backup retained at $config_backup" >&2
            return "$status"
        fi
    fi
    if [[ -n "$config_backup_dir" ]]; then
        if rm -rf "$config_backup_dir"; then
            config_backup_dir=""
            config_backup=""
            config_backup_ready=0
        else
            status=$?
            echo "[serve-web-static] config restored; scratch cleanup refused at $config_backup_dir" >&2
            return "$status"
        fi
    fi
}

finish_config_owner() {
    local primary="$1" cleanup=0
    trap - EXIT INT TERM
    restore_dioxus_config || cleanup=$?
    if [[ "$cleanup" != 0 ]]; then
        echo "[serve-web-static] original status=$primary; config cleanup status=$cleanup" >&2
        [[ "$primary" != 0 ]] || primary="$cleanup"
    fi
    exit "$primary"
}

cd "$web_root"

# Check if any web-relevant source is newer than the build output.
# If so, clear the dx cache so the next build picks up changes.
web_sources_stale() {
    local build_output="$1"
    if [[ ! -f "$build_output" ]]; then
        return 0
    fi
    local newest_src=""
    newest_src="$(find "$repo_root/crates" -path '*/src/*' -name '*.rs' -newer "$build_output" -print -quit 2>/dev/null || true)"
    if [[ -n "$newest_src" ]]; then
        return 0
    fi
    newest_src="$(find "$repo_root/crates" -name 'Cargo.toml' -newer "$build_output" -print -quit 2>/dev/null || true)"
    [[ -n "$newest_src" || "$repo_root/Cargo.lock" -nt "$build_output" || "$repo_root/Cargo.toml" -nt "$build_output" ]]
}

web_build_has_harness_support() {
    local public_dir="$1"
    local asset

    # Dioxus fingerprints current JS/WASM bundle names under assets/. Older
    # releases used wasm/aura-web.js, so inspect emitted bundles rather than
    # coupling cache validation to either layout.
    while IFS= read -r -d '' asset; do
        if grep -a -q "__AURA_HARNESS__" "$asset"; then
            return 0
        fi
    done < <(find "$public_dir" -type f \( -name '*.js' -o -name '*.wasm' \) -print0)

    return 1
}

# LAN validation must never trigger an unguarded Dioxus build or clear a
# serving cache after the harness has started. Build on the idle host first.
if [[ "$prebuilt_only" == 1 ]]; then
    [[ "$build_profile" == release || "$build_profile" == debug ]] || {
        echo "[serve-web-static] unsupported profile: $build_profile" >&2; exit 1;
    }
    prebuilt_dir="$repo_root/target/dx/aura-web/$build_profile/web/public"
    if ! aura_web_admitted_prebuilt_matches "$repo_root" "$prebuilt_dir" ||
         ! web_build_has_harness_support "$prebuilt_dir"; then
        echo '[serve-web-static] missing, stale or non-harness web bundle; stop the harness and run scripts/harness/lan/build.sh web first' >&2
        exit 1
    fi
    public_dir="$prebuilt_dir"
    if [[ ! -d "$web_root/node_modules/ws" ]]; then
        echo '[serve-web-static] missing prebuilt server runtime dependency ws; provision dependencies before starting the harness' >&2
        exit 1
    fi
    echo "[serve-web-static] reusing admitted prebuilt $build_profile web assets at $public_dir"
fi

if [[ "$prebuilt_only" == 0 ]]; then
if [ ! -d node_modules ] || [ ! -d node_modules/ws ]; then
    npm ci
fi
npm run tailwind:build >/dev/null
mkdir -p \
    "$repo_root/target/dx/aura-web/release/web/public/assets" \
    "$repo_root/target/dx/aura-web/release/web/public/fonts" \
    "$repo_root/target/dx/aura-web/debug/web/public/assets" \
    "$repo_root/target/dx/aura-web/debug/web/public/fonts"
# Development rebuilds keep CSS linked to their current generated source.
source_css="$web_root/public/assets/tailwind.css"
for profile in debug release; do
    target_css="$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
    rm -f "$target_css"
    ln -s "$source_css" "$target_css"
done

for profile in debug release; do
    [[ "$prebuilt_only" == 0 ]] || break
    build_output="$repo_root/target/dx/aura-web/$profile/web/public/index.html"
    if web_sources_stale "$build_output"; then
        echo "[serve-web-static] source files changed, clearing $profile dx cache"
        rm -rf "$repo_root/target/dx/aura-web/$profile"
        mkdir -p "$repo_root/target/dx/aura-web/$profile/web/public/assets" \
                 "$repo_root/target/dx/aura-web/$profile/web/public/fonts"
        rm -f "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
        ln -s "$source_css" "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
    fi
    if [[ -f "$build_output" ]] && ! web_build_has_harness_support "$repo_root/target/dx/aura-web/$profile/web/public"; then
        echo "[serve-web-static] cached $profile web assets are missing harness support, clearing dx cache"
        rm -rf "$repo_root/target/dx/aura-web/$profile"
        mkdir -p "$repo_root/target/dx/aura-web/$profile/web/public/assets" \
                 "$repo_root/target/dx/aura-web/$profile/web/public/fonts"
        rm -f "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
        ln -s "$source_css" "$repo_root/target/dx/aura-web/$profile/web/public/assets/tailwind.css"
    fi
done

if [[ -e "$dioxus_config" || -L "$dioxus_config" ]]; then
    # This transaction owns one ordinary config path, not a link's referent.
    # An invocation must have exclusive access until the original is restored.
    [[ -f "$dioxus_config" && ! -L "$dioxus_config" ]] || {
        echo "[serve-web-static] config backup requires a regular non-symlink file: $dioxus_config" >&2
        exit 2
    }
    # Arm ownership before allocation, backup copying or the first live edit.
    trap 'finish_config_owner "$?"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    config_backup_dir="$(mktemp -d "${TMPDIR:-/tmp}/aura-web-config.XXXXXXXX")"
    config_backup="$config_backup_dir/original"
    cp -p "$dioxus_config" "$config_backup"
    config_backup_ready=1
    config_mutated=1
    perl -0pi -e 's/^hot_reload = true$/hot_reload = false/m' "$dioxus_config"
    perl -0pi -e 's/^reload_html = true$/reload_html = false/m' "$dioxus_config"
fi

case "$build_profile" in
    release)
        public_dir="$repo_root/target/dx/aura-web/release/web/public"
        if [[ ! -f "$public_dir/index.html" ]]; then
            NO_COLOR=true ../../scripts/web/dx.sh build --release --profile wasm --platform web --package aura-web --bin aura-web --features web,harness,production-build
        else
            echo "[serve-web-static] reusing prebuilt release web assets at $public_dir"
        fi
        if [[ ! -f "$public_dir/index.html" ]]; then
            public_dir="$repo_root/target/dx/aura-web/debug/web/public"
            if [[ ! -f "$public_dir/index.html" ]]; then
                NO_COLOR=true ../../scripts/web/dx.sh build --platform web --package aura-web --bin aura-web --features web,harness
            else
                echo "[serve-web-static] reusing prebuilt debug web assets at $public_dir"
            fi
        fi
        ;;
    debug)
        public_dir="$repo_root/target/dx/aura-web/debug/web/public"
        if [[ ! -f "$public_dir/index.html" ]]; then
            NO_COLOR=true ../../scripts/web/dx.sh build --platform web --package aura-web --bin aura-web --features web,harness
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
trap - EXIT INT TERM
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
const { WebSocketServer } = require("ws");
const publicDir = process.argv[1];
const port = Number(process.argv[2]);
const host = "127.0.0.1";
// Multi-host LAN runs also expose the transport relay on the LAN
// address so peers on another host can reach browsers on this host. Pages
// keep loading from loopback, which browsers treat as a secure context.
const relayHost = process.env.AURA_HARNESS_WEB_RELAY_HOST || "";
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
if (relayHost && relayHost !== host) {
  const relayServer = http.createServer(server.listeners("request")[0]);
  relayServer.on("upgrade", (req, socket, head) => {
    websocketServer.handleUpgrade(req, socket, head, (ws) => {
      websocketServer.emit("connection", ws, req);
    });
  });
  relayServer.listen(port, relayHost, () => {
    process.stdout.write(`[serve-web-static] transport relay on ${relayHost}:${port}\n`);
  });
}
' "$public_dir" "$port" "${2:-}"
