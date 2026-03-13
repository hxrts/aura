#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/async-service-actor-ownership.allowlist"

fail() {
  echo "async-service-actor-ownership: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done < "$allowlist_file"

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  {
    rg -n \
      -e 'pub async fn start\(' \
      -e 'pub async fn stop\(' \
      -e 'watch::Sender<bool>' \
      crates/aura-agent/src/runtime/services/rendezvous_manager.rs \
      crates/aura-agent/src/runtime/services/sync_manager.rs
    rg -n 'tokio::spawn|spawn_local' \
      crates/aura-agent/src/runtime/services/rendezvous_manager.rs \
      crates/aura-agent/src/runtime/services/sync_manager.rs \
      | rg -v ':\s*//!|:\s*//|:\s*/\*'
  } | sort -u
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "migrated services bypass actor-owned lifecycle or task ownership"
fi

echo "async service actor ownership: clean (${legacy_exemptions} temporary exemptions)"
