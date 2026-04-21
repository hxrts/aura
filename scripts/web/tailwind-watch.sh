#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

source "$script_dir/log-bootstrap.sh"
aura_web_redirect_logs "$repo_root" "$repo_root/artifacts/aura-web/tailwind-watch.log"

cd "$repo_root/crates/aura-web"
npm run tailwind:watch
