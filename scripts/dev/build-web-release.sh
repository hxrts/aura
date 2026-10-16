#!/usr/bin/env bash
# Build the same release/harness web bundle used by the LAN static server.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
web_root="$repo_root/crates/aura-web"
source "$repo_root/scripts/web/prebuilt-provenance.sh"
start_commit="$(aura_web_clean_commit "$repo_root" || true)"
start_tree="$(aura_web_clean_tree "$repo_root" || true)"
start_config="$(aura_web_build_config_digest "$repo_root" || true)"
tool_inventory=$(mktemp "${TMPDIR:-/tmp}/aura-web-tools.XXXXXX")
chmod 600 "$tool_inventory"
trap 'rm -f "$tool_inventory"' EXIT
export AURA_WEB_TOOL_INVENTORY="$tool_inventory"
aura_web_record_selected_tool "$tool_inventory" node "$(command -v node)"
aura_web_record_selected_tool "$tool_inventory" npm "$(command -v npm)"
cd "$web_root"
if [[ ! -d node_modules || ! -d node_modules/ws ]]; then
  npm ci
fi
npm run tailwind:build
NO_COLOR=true ../../scripts/web/dx.sh "${aura_web_build_args[@]}"

public_dir="$repo_root/target/dx/aura-web/release/web/public"
[[ -f "$public_dir/index.html" ]] || { echo 'web release build produced no index.html' >&2; exit 1; }
mkdir -p "$public_dir/assets"
ln -sfn "$web_root/public/assets/tailwind.css" "$public_dir/assets/tailwind.css"
aura_web_write_prebuilt_provenance "$repo_root" "$public_dir" "$tool_inventory" "$start_commit" "$start_tree" "$start_config"
printf 'Web bundle: %s\n' "$public_dir"
