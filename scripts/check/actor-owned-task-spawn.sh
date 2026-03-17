#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

# Temporary exemptions (owner: architecture, doc: work/ownership.md)
allowlist=(
  '^crates/aura-terminal/src/demo/signal_coordinator\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/demo/simulator\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/handlers/scenarios/simulation\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/handlers/tui\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/tui/context/io_context\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/tui/effects/operational/messaging\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/tui/harness_state\.rs:.*tokio::spawn'
  '^crates/aura-terminal/src/tui/screens/app/subscriptions\.rs:.*tokio::spawn'
)

fail() {
  echo "actor-owned-task-spawn: $*" >&2
  exit 1
}

approved_patterns=(
  '^crates/aura-agent/src/task_registry\.rs:'
  '^crates/aura-effects/src/reactive/handler\.rs:'
  '^crates/aura-effects/src/reactive/graph\.rs:'
  '^crates/aura-harness/src/backend/local_pty\.rs:.*thread::spawn'
  '^crates/aura-harness/src/backend/playwright_browser\.rs:.*thread::spawn'
  '^crates/aura-harness/src/bin/tool_repl\.rs:.*thread::spawn'
  '^crates/aura-harness/src/coordinator\.rs:.*thread::spawn'
  '^crates/aura-harness/src/executor\.rs:.*std::thread::spawn'
  '^crates/aura-terminal/src/tui/tasks\.rs:'
  '^crates/aura-testkit/src/infrastructure/time\.rs:.*thread::spawn'
  '^crates/aura-ui/src/app\.rs:'
  '^crates/aura-web/src/harness_bridge\.rs:.*spawn_local'
  '^crates/aura-web/src/main\.rs:'
  '^crates/aura-web/src/web_clipboard\.rs:.*spawn_local'
)

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  approved=0
  for pattern in "${approved_patterns[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      approved=1
      break
    fi
  done
  if (( approved == 1 )); then
    continue
  fi

  allowed=0
  for pattern in "${allowlist[@]}"; do
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  rg -n 'tokio::spawn\(|spawn_local\(async move|std::thread::spawn\(|thread::spawn\(' crates/*/src -g '*.rs' \
    | rg -v ':\s*//!|:\s*//|:\s*/\*'
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "raw task spawning appears outside approved actor/supervisor modules"
fi

echo "actor-owned task spawn: clean (${legacy_exemptions} temporary exemptions)"
