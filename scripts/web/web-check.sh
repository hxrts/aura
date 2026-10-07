#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

source "$script_dir/log-bootstrap.sh"
aura_web_redirect_logs "$repo_root" "$repo_root/artifacts/aura-web/web-check.log"

cd "$repo_root"
cargo clippy -p aura-ui -- -D warnings
cargo clippy -p aura-web --target wasm32-unknown-unknown --features web -- -D warnings
