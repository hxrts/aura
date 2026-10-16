#!/usr/bin/env bash
# Execute one sequential build phase and record its actual outcome.
set -euo pipefail
phase="${1:?usage: build-phase.sh compile|discovery|execution|disposal [--expected-tests FILE] -- COMMAND}"
shift
case "$phase" in compile|discovery|execution|disposal) ;; *) exit 2 ;; esac
expected=''
if [[ "${1:-}" == --expected-tests ]]; then expected="${2:?missing inventory}"; shift 2; fi
[[ "${1:-}" == -- && $# -gt 1 ]] || exit 2
shift
[[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || {
  echo 'build-phase: requires an original build-budget receipt' >&2; exit 2;
}
jq -e '.final_exit_status==null and (.wrapper.pid|type)=="number"' "$AURA_BUILD_RECEIPT" >/dev/null
kill -0 "$(jq -r '.wrapper.pid' "$AURA_BUILD_RECEIPT")" 2>/dev/null || {
  echo 'build-phase: original receipt owner is no longer live' >&2; exit 2;
}
owner_identity="$(jq -r '.wrapper.start_identity' "$AURA_BUILD_RECEIPT")"
[[ -n "$owner_identity" && "$owner_identity" == "$(ps -p "$(jq -r '.wrapper.pid' "$AURA_BUILD_RECEIPT")" -o lstart= 2>/dev/null)" ]] || {
  echo 'build-phase: original receipt process identity changed' >&2; exit 2;
}
if [[ -n "$expected" ]]; then
  [[ "$phase" == discovery || "$phase" == execution ]] || exit 2
  [[ -s "$expected" ]] || { echo 'build-phase: empty/missing expected test inventory' >&2; exit 2; }
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source "$repo_root/scripts/dev/build-receipt.sh"
aura_receipt_cargo_metadata "$@"
phase_packages="$receipt_packages"
phase_features="$receipt_features"
phase_target="$receipt_test_target"
phase_filter="$receipt_test_filter"
phase_lock="${AURA_BUILD_RECEIPT}.phase-lock"
mkdir "$phase_lock" 2>/dev/null || {
  echo 'build-phase: another phase owns this receipt' >&2; exit 2;
}
phase_log="$(mktemp "$(dirname "$AURA_BUILD_RECEIPT")/$phase.XXXXXX")"
argv="$(jq -cn --args '$ARGS.positional' -- "$@")"
started="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
expected_json='null'
if [[ -n "$expected" ]]; then
  expected_json="$(jq -Rs 'split("\n") | map(select(length>0)) | sort' "$expected")"
fi
runner_identity="$(ps -p "$$" -o lstart= 2>/dev/null || true)"
child_identity=''
observed_child='null'
recorded=0
child=''
finish_phase() {
  local status=$?
  if [[ -n "$child" ]] && kill -0 "$child" 2>/dev/null; then
    kill -TERM "$child" 2>/dev/null || true
    wait "$child" 2>/dev/null || true
  fi
  if (( recorded == 0 )); then
    aura_receipt_update --arg phase "$phase" --arg log "$phase_log" \
      --argjson status "$status" --argjson argv "$argv" --argjson child "$observed_child" \
      '.phases += [{phase:$phase,command_argv:$argv,log:$log,
        child:$child,command_exit_status:null,exit_status:$status,inventory_result:"unobserved"}]
        | .active_phase=null'
  fi
  rmdir "$phase_lock" 2>/dev/null || true
}
trap finish_phase EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
aura_receipt_update --arg phase "$phase" --arg log "$phase_log" --arg started "$started" \
  --argjson argv "$argv" --argjson pid "$$" --arg identity "$runner_identity" \
  --arg packages "$phase_packages" --arg features "$phase_features" \
  --arg target "$phase_target" --arg filter "$phase_filter" \
  --argjson expected "$expected_json" \
  '.current_phase=$phase | .active_phase={phase:$phase,command_argv:$argv,log:$log,
    started_utc:$started,runner:{pid:$pid,start_identity:$identity},child:null} |
    .metadata.phase_commands += [{phase:$phase,
      packages:(if $packages=="" then null else $packages end),
      features:(if $features=="" then null else $features end),
      test_target:(if $target=="" then null else $target end),
      test_filter:(if $filter=="" then null else $filter end),
      expected_tests:$expected}]'
# Capture output as an artifact; do not infer a successful command from output.
"$@" > "$phase_log" 2>&1 &
child=$!
child_identity="$(ps -p "$child" -o lstart= 2>/dev/null || true)"
observed_child="$(jq -cn --argjson pid "$child" --arg identity "$child_identity" \
  '{pid:$pid,start_identity:(if $identity=="" then null else $identity end)}')"
aura_receipt_update --argjson child "$observed_child" '.active_phase.child=$child'
command_status=0
wait "$child" || command_status=$?
child=''
cat "$phase_log"
status="$command_status"
inventory_result=unknown
observed='null'
passed='null'
failed='null'
ignored='null'
if [[ "$phase" == execution && -n "$expected" ]]; then
  # --nocapture may put diagnostics between a test name and its terminal status.
  parsed="$(awk -v expected_file="$expected" -f "$repo_root/scripts/dev/libtest-inventory.awk" "$phase_log")" || parse_status=$?
  passed="$(printf '%s\n' "$parsed" | awk -F '\t' '$1=="passed" {print $2}' | jq -Rs 'split("\n") | map(select(length>0)) | sort')"
  failed="$(printf '%s\n' "$parsed" | awk -F '\t' '$1=="failed" {print $2}' | jq -Rs 'split("\n") | map(select(length>0)) | sort')"
  ignored="$(printf '%s\n' "$parsed" | awk -F '\t' '$1=="ignored" {print $2}' | jq -Rs 'split("\n") | map(select(length>0)) | sort')"
  # A command failure with only apparent successes (or a successful command
  # with a failure token) cannot certify those identities as terminal results.
  if { (( command_status != 0 )) && [[ "$failed" == '[]' ]]; } ||
     { (( command_status == 0 )) && [[ "$failed" != '[]' ]]; }; then
    parse_status=78
    passed='[]'; failed='[]'; ignored='[]'
  fi
  observed="$(jq -cn --argjson passed "$passed" --argjson failed "$failed" '$passed+$failed|sort')"
fi
if [[ -n "$expected" ]]; then
  if [[ "$phase" == discovery ]]; then
    observed="$(sed -n 's/: test$//p' "$phase_log" | jq -Rs 'split("\n") | map(select(length>0)) | sort')"
  fi
  if [[ "${parse_status:-0}" == 0 ]] && jq -en --argjson observed "$observed" --argjson expected "$expected_json" \
    '$observed==$expected and ($observed|length)>0' >/dev/null; then
    inventory_result=matched
  else
    inventory_result=mismatch
    (( status != 0 )) || status=78
    echo "build-phase: $phase required inventory mismatch; see $phase_log" >&2
  fi
fi
aura_receipt_update --arg phase "$phase" --arg log "$phase_log" --arg started "$started" \
  --arg finished "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --arg inventory "$inventory_result" \
  --argjson command_status "$command_status" --argjson status "$status" \
  --argjson argv "$argv" --argjson expected "$expected_json" --argjson observed "$observed" \
  --argjson child "$observed_child" --argjson runner_pid "$$" --arg identity "$runner_identity" \
  --argjson passed "$passed" --argjson failed "$failed" --argjson ignored "$ignored" \
  '.phases += [{phase:$phase,command_argv:$argv,log:$log,started_utc:$started,
    runner:{pid:$runner_pid,start_identity:$identity},child:$child,
    finished_utc:$finished,command_exit_status:$command_status,exit_status:$status,
    inventory_result:$inventory,expected_tests:$expected,observed_tests:$observed,
    passed_tests:$passed,failed_tests:$failed,ignored_tests:$ignored,
    passed_count:(if $passed==null then null else ($passed|length) end),
    failed_count:(if $failed==null then null else ($failed|length) end),
    ignored_count:(if $ignored==null then null else ($ignored|length) end),
    expected_count:(if $expected==null then null else ($expected|length) end),
    observed_count:(if $observed==null then null else ($observed|length) end)}]
    | .active_phase=null'
recorded=1
if (( status != 0 )); then aura_receipt_first_failure || true; fi
exit "$status"
