#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

policy_allowlist="scripts/check/timeout-policy-boundary.allowlist"
backoff_allowlist="scripts/check/timeout-backoff-discipline.allowlist"

fail() {
  echo "timeout-policy-boundary: $*" >&2
  exit 1
}

[[ -f "$policy_allowlist" ]] || fail "missing allowlist: $policy_allowlist"
[[ -f "$backoff_allowlist" ]] || fail "missing allowlist: $backoff_allowlist"

exit_code=0

# --- Pass 1: direct timeout/sleep primitives ---

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done < "$policy_allowlist"

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  {
    rg -n \
      -e 'tokio::time::timeout\(' \
      -e 'tokio::time::sleep\(' \
      -e 'std::thread::sleep\(' \
      -e 'thread::sleep\(' \
      crates/aura-app/src/workflows \
      crates/aura-agent/src/handlers/invitation \
      crates/aura-agent/src/runtime_bridge \
      crates/aura-agent/src/runtime/effects \
      crates/aura-terminal/src/tui \
      crates/aura-harness/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  echo "timeout-policy-boundary: direct timeout/sleep primitives found outside sanctioned wrappers" >&2
  exit_code=1
fi

# --- Pass 2: hand-rolled retry/backoff loops ---

backoff_violations=()
backoff_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      backoff_exemptions=$((backoff_exemptions + 1))
      break
    fi
  done < "$backoff_allowlist"

  if (( allowed == 0 )); then
    backoff_violations+=("$match")
  fi
done < <(
  {
    rg -n \
      -e 'backoff\s*=\s*\(backoff \* 2\)\.min' \
      -e 'sleep\(backoff\)' \
      -e 'retry_interval_ms' \
      -e 'for attempt in ' \
      -e 'attempts \+=' \
      crates/aura-app/src/workflows \
      crates/aura-agent/src/handlers/invitation \
      crates/aura-agent/src/runtime_bridge \
      crates/aura-terminal/src/tui \
      crates/aura-harness/src \
      -g '*.rs'
  } | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#backoff_violations[@]} > 0 )); then
  printf '%s\n' "${backoff_violations[@]}" >&2
  echo "timeout-policy-boundary: hand-rolled retry/backoff logic found outside shared timeout model" >&2
  exit_code=1
fi

if (( exit_code != 0 )); then
  exit 1
fi

echo "timeout policy boundary: clean (${legacy_exemptions} policy + ${backoff_exemptions} backoff temporary exemptions)"
