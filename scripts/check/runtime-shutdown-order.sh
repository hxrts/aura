#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

target="crates/aura-agent/src/runtime/system.rs"

fail() {
  echo "runtime-shutdown-order: $*" >&2
  exit 1
}

[[ -f "$target" ]] || fail "missing target: $target"

reactive_line="$(rg -n "self\\.reactive_pipeline\\.take\\(\\)" "$target" | cut -d: -f1 | head -n1)"
task_tree_line="$(rg -n "shutdown_with_timeout\\(Duration::from_secs\\(5\\)\\)" "$target" | cut -d: -f1 | head -n1)"
stop_services_line="$(rg -n "self\\.stop_services\\(\\)\\.await" "$target" | cut -d: -f1 | head -n1)"
lifecycle_line="$(rg -n "lifecycle_manager\\.shutdown\\(ctx\\)\\.await" "$target" | cut -d: -f1 | head -n1)"

[[ -n "$reactive_line" ]] || fail "missing reactive pipeline shutdown step"
[[ -n "$task_tree_line" ]] || fail "missing runtime task tree shutdown step"
[[ -n "$stop_services_line" ]] || fail "missing stop_services step"
[[ -n "$lifecycle_line" ]] || fail "missing lifecycle shutdown step"

(( reactive_line < task_tree_line )) || fail "reactive pipeline must shut down before task tree cancellation"
(( task_tree_line < stop_services_line )) || fail "runtime task tree must cancel before service teardown"
(( stop_services_line < lifecycle_line )) || fail "services must stop before lifecycle manager shutdown"

if rg -n "runtime_tasks\\.shutdown\\(\\);" "$target" > /dev/null; then
  fail "found legacy unbounded runtime_tasks.shutdown() call"
fi

echo "runtime shutdown order: clean"
