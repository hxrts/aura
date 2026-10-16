#!/usr/bin/env bash
# Fixed-commit pre-ship smoke with independently resumable, guarded steps.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$root"
source scripts/dev/build-receipt.sh
source scripts/dev/policy-dependency-inputs.sh
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-1}"
export CARGO_BUILD_JOBS=4
export AURA_BUILD_WAIT_SECONDS="${AURA_BUILD_WAIT_SECONDS:-1800}"

commands=(
  'just ci-clippy'
  'just web-check'
  'cargo test -p hxrts-aura-app --lib --features native,app-internals,web-js'
  'cargo test -p aura-ui --lib'
  'cargo test -p hxrts-aura-agent --test home_flows --test runtime_integration --test telltale_machine'
  'cargo test -p aura-terminal --lib --test cli_rpc --test cli_production --test e2e_terminal_state --test cli_socket'
)
mode="${1:-run}"
[[ "$mode" == run || "$mode" == --verify ]] || {
  echo 'usage: scripts/ci/main-smoke.sh [--verify]' >&2; exit 2;
}
[[ -z "$(git status --porcelain --untracked-files=normal)" ]] || {
  echo 'main smoke requires a clean checkout' >&2; exit 1;
}
commit="$(git rev-parse --verify HEAD)"
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || exit 1
smoke_dir="$root/.tmp/main-smoke-$commit"
validated="$smoke_dir/validated-commit"
private_file() {
  [[ -f "$1" && ! -L "$1" && -O "$1" &&
     "$(aura_receipt_stat_mode "$1")" == 600 ]]
}
budget_path_valid() {
  local receipt=$1 run suffix
  [[ "$receipt" == "$root/artifacts/disk-budget/receipts/"*/receipt.json ]] || return 1
  run="${receipt%/receipt.json}"
  suffix="${run#"$root/artifacts/disk-budget/receipts/run."}"
  [[ "$run" == "$root/artifacts/disk-budget/receipts/run.$suffix" &&
     "$suffix" =~ ^[A-Za-z0-9]{6}$ && -d "$run" && ! -L "$run" &&
     -O "$run" && "$(aura_receipt_stat_mode "$run")" == 700 ]]
}
command_log_valid() {
  local log=$1 receipt=$2 suffix
  suffix="${log#"${receipt%/receipt.json}/command."}"
  suffix="${suffix%.log}"
  [[ "$log" == "${receipt%/receipt.json}/command.$suffix.log" &&
     "$suffix" =~ ^[A-Za-z0-9]{6}$ ]] && private_file "$log"
}
[[ ! -L "$root/.tmp" && ( ! -e "$root/.tmp" || ( -d "$root/.tmp" && -O "$root/.tmp" ) ) ]] || exit 1
if [[ ! -e "$root/.tmp" ]]; then mkdir -m 700 "$root/.tmp"; fi
[[ ! -L "$smoke_dir" && ( ! -e "$smoke_dir" ||
   ( -d "$smoke_dir" && -O "$smoke_dir" && "$(aura_receipt_stat_mode "$smoke_dir")" == 700 ) ) ]] || {
  echo 'main smoke evidence directory is unprotected' >&2; exit 1;
}
[[ ! -e "$validated" && ! -L "$validated" ]] || private_file "$validated" || exit 1
if [[ "$mode" == run ]]; then
  umask 077
  [[ -e "$smoke_dir" ]] || mkdir -m 700 "$smoke_dir"
  rm -f "$validated"
fi
[[ -d "$smoke_dir" && ! -L "$smoke_dir" && -O "$smoke_dir" &&
   "$(aura_receipt_stat_mode "$smoke_dir")" == 700 ]] || {
  echo 'main smoke evidence directory is unavailable or unprotected' >&2; exit 1;
}

tool_identity() {
  # Evaluate actual selected executables inside the same pinned shell that
  # runs the steps. Hash values only; never put environment or key material in
  # shared receipts. A missing executable cannot yield a reusable proof.
  nix develop -c bash -c '
    set -euo pipefail
    source scripts/dev/policy-dependency-inputs.sh
    policy_tool_identity "$PWD" "$PWD"
    policy_ownership_native_identity
    for name in just cargo rustc node npm; do
      selected="$(command -v "$name")"
      [[ -f "$selected" && -x "$selected" ]] || exit 2
      printf "%s\0%s\0" "$name" "$selected"
      sha256sum "$selected"
    done
  ' | sha256sum | awk '{print $1}'
}

selector_identity() {
  local key
  for key in RUST_TEST_THREADS CARGO_BUILD_JOBS AURA_BUILD_WAIT_SECONDS \
    AURA_TEST_STORAGE_PROVIDER AURA_TEST_CLOCK_PROVIDER AURA_TEST_KEYRING_PROVIDER \
    AURA_HARNESS_MODE AURA_HARNESS_WEB_BUILD_PROFILE RUSTFLAGS CARGO_ENCODED_RUSTFLAGS \
    CARGO_HOME CARGO_TARGET_DIR CARGO_BUILD_TARGET CARGO_INCREMENTAL \
    RUSTC RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER SDKROOT DEVELOPER_DIR; do
    printf '%s\0%s\0' "$key" "${!key-}"
  done
  local exported key
  exported="$(policy_exported_names)" || return 2
  while IFS= read -r key; do
    case "$key" in
      AURA_TEST_*|AURA_HARNESS_*|CARGO_PROFILE_*|CARGO_TARGET_*|CC|CXX|AR|LD|HOST_CC|TARGET_CC|HOST_AR|TARGET_AR)
        [[ "$key" != AURA_TEST_KEYRING_DIR ]] || continue
        printf '%s\0%s\0' "$key" "${!key-}";;
    esac
  done < <(printf '%s\n' "$exported" | LC_ALL=C sort -u)
}

refresh_inputs() {
  tool_hash="$(tool_identity)" || return 1
  selector_hash="$(selector_identity | sha256sum | awk '{print $1}')" || return 1
}
refresh_inputs || { echo 'main smoke input identity unavailable' >&2; exit 1; }
flake_hash="$(sha256sum flake.lock | awk '{print $1}')"
owner_hash="$(sha256sum scripts/ci/main-smoke.sh | awk '{print $1}')"
budget_hash="$(sha256sum scripts/dev/build-budget.sh | awk '{print $1}')"

step_input() {
  local index="$1" command_text="$2"
  printf '%s\0%s\0%s\0%s\0%s\0%s\0%s\0%s\0' \
    "$commit" "$index" "$command_text" "$tool_hash" "$selector_hash" \
    "$flake_hash" "$owner_hash" "$budget_hash" | sha256sum | awk '{print $1}'
}

valid_step() {
  local index="$1" command_text="$2" input="$3" receipt log budget_receipt command_log
  receipt="$smoke_dir/step-$index.json"
  private_file "$receipt" || return 1
  log="$(jq -r '.log // empty' "$receipt")"
  local suffix="${log#"$smoke_dir/step-$index.attempt-"}"
  [[ "$log" == "$smoke_dir/step-$index.attempt-$suffix" &&
     "$suffix" =~ ^[A-Za-z0-9]{6}$ ]] || return 1
  private_file "$log" || return 1
  [[ ! -e "$smoke_dir/keyring-$index" && ! -L "$smoke_dir/keyring-$index" ]] || return 1
  budget_receipt="$(jq -r '.budget_receipt // empty' "$receipt")"
  budget_path_valid "$budget_receipt" || return 1
  private_file "$budget_receipt" || return 1
  jq -e --arg root "$root" --arg commit "$commit" --arg command "$command_text" \
    '.root==$root and .source.revision==$commit and .admission.admitted==true and
     .final_exit_status==0 and .lane=="handoff-smoke" and
     .command_argv==["bash","-c",$command]' "$budget_receipt" >/dev/null || return 1
  command_log="$(jq -r '.artifacts.command_log // empty' "$budget_receipt")"
  command_log_valid "$command_log" "$budget_receipt" || return 1
  jq -e --argjson index "$index" --arg commit "$commit" \
    --arg command "$command_text" --arg input "$input" \
    --arg log "$log" --arg log_hash "$(sha256sum "$log" | awk '{print $1}')" \
    --arg budget "$budget_receipt" \
    --arg budget_hash "$(sha256sum "$budget_receipt" | awk '{print $1}')" \
    --arg command_log "$command_log" \
    --arg command_log_hash "$(sha256sum "$command_log" | awk '{print $1}')" \
    '.schema==1 and .index==$index and .commit==$commit and
     .command==$command and .input_sha256==$input and .log==$log and .log_sha256==$log_hash and
     .budget_receipt==$budget and .budget_receipt_sha256==$budget_hash and
     .command_log==$command_log and .command_log_sha256==$command_log_hash and
     .keyring_cleanup=="acknowledged" and .result=="passed"' \
    "$receipt" >/dev/null
}

run_step() {
  local index="$1" command_text="$2" input="$3" receipt log keyring status budget_receipt command_log scratch attempt
  receipt="$smoke_dir/step-$index.json"
  keyring="$smoke_dir/keyring-$index"
  [[ ! -e "$receipt" && ! -L "$receipt" ]] || private_file "$receipt" || return 1
  [[ -z "$(find "$smoke_dir" -maxdepth 1 -name "step-$index.attempt-*" -type l -print -quit)" ]] || return 1
  rm -f "$receipt"
  [[ ! -e "$keyring" && ! -L "$keyring" ]] || {
    echo "main smoke step $index has retained keyring state" >&2; return 1;
  }
  mkdir -m 700 "$keyring"
  log="$(mktemp "$smoke_dir/step-$index.attempt-XXXXXX")"
  chmod 600 "$log"
  echo "Main smoke step $index: $command_text"
  if AURA_TEST_KEYRING_DIR="$keyring" nix develop -c bash -c '
    unset CARGO_TARGET_DIR
    exec nice -n 10 bash scripts/dev/build-budget.sh --lane handoff-smoke -- bash -c "$1"
  ' aura-smoke "$command_text" > "$log" 2>&1; then
    status=0
  else
    status=$?
  fi
  # The owned command is terminal before keyring disposal. Never retain test
  # keys or promote a failed step into reusable execution evidence.
  rm -rf "$keyring"
  [[ ! -e "$keyring" ]] || return 1
  budget_receipt="$(sed -n 's/^Build receipt: //p' "$log" | tail -1)"
  attempt="$log.json"
  scratch="$(mktemp "$smoke_dir/attempt.XXXXXX")"
  jq -n --argjson index "$index" --arg commit "$commit" --arg input "$input" \
    --arg log "$log" --arg log_hash "$(sha256sum "$log" | awk '{print $1}')" \
    --arg budget "$budget_receipt" --argjson status "$status" \
    '{schema:1,index:$index,commit:$commit,input_sha256:$input,log:$log,
      log_sha256:$log_hash,budget_receipt:$budget,exit_status:$status,
      keyring_cleanup:"acknowledged"}' > "$scratch"
  chmod 600 "$scratch"
  mv "$scratch" "$attempt"
  if (( status != 0 )); then
    echo "main smoke step $index failed ($status); protected log: $log" >&2
    return "$status"
  fi
  budget_path_valid "$budget_receipt" || return 1
  private_file "$budget_receipt" || return 1
  jq -e --arg root "$root" --arg commit "$commit" --arg command "$command_text" \
    '.root==$root and .source.revision==$commit and .admission.admitted==true and
     .final_exit_status==0 and .lane=="handoff-smoke" and
     .command_argv==["bash","-c",$command]' "$budget_receipt" >/dev/null || return 1
  command_log="$(jq -r '.artifacts.command_log // empty' "$budget_receipt")"
  command_log_valid "$command_log" "$budget_receipt" || return 1
  refresh_inputs || return 1
  [[ "$(git rev-parse HEAD)" == "$commit" &&
     -z "$(git status --porcelain --untracked-files=normal)" &&
     "$input" == "$(step_input "$index" "$command_text")" ]] || return 1
  scratch="$(mktemp "$smoke_dir/step-$index.XXXXXX")"
  jq -n --argjson index "$index" --arg commit "$commit" \
    --arg command "$command_text" --arg input "$input" \
    --arg log "$log" --arg log_hash "$(sha256sum "$log" | awk '{print $1}')" \
    --arg budget "$budget_receipt" \
    --arg budget_hash "$(sha256sum "$budget_receipt" | awk '{print $1}')" \
    --arg command_log "$command_log" \
    --arg command_log_hash "$(sha256sum "$command_log" | awk '{print $1}')" \
    '{schema:1,index:$index,commit:$commit,command:$command,input_sha256:$input,log:$log,
      log_sha256:$log_hash,budget_receipt:$budget,budget_receipt_sha256:$budget_hash,
      command_log:$command_log,command_log_sha256:$command_log_hash,
      keyring_cleanup:"acknowledged",result:"passed"}' \
    > "$scratch"
  chmod 600 "$scratch"
  mv "$scratch" "$receipt"
}

index=0
for command_text in "${commands[@]}"; do
  index=$((index + 1))
  if [[ "$mode" == run ]]; then refresh_inputs || exit 1; fi
  input="$(step_input "$index" "$command_text")"
  if valid_step "$index" "$command_text" "$input"; then
    [[ "$mode" != run ]] || echo "Main smoke step $index: reusing exact passed receipt"
  elif [[ "$mode" == --verify ]]; then
    echo "main smoke step $index has no valid exact evidence" >&2
    exit 1
  else
    run_step "$index" "$command_text" "$input"
  fi
done

refresh_inputs || exit 1
index=0
for command_text in "${commands[@]}"; do
  index=$((index + 1))
  valid_step "$index" "$command_text" "$(step_input "$index" "$command_text")" || {
    echo "main smoke step $index changed before final proof" >&2; exit 1;
  }
done
[[ "$(git rev-parse HEAD)" == "$commit" &&
   -z "$(git status --porcelain --untracked-files=normal)" ]] || {
  echo 'main smoke source changed during verification' >&2; exit 1;
}

if [[ "$mode" == run ]]; then
  [[ "$(git rev-parse HEAD)" == "$commit" &&
     -z "$(git status --porcelain --untracked-files=normal)" ]] || exit 1
  scratch="$(mktemp "$smoke_dir/validated-commit.XXXXXX")"
  printf '%s\n' "$commit" > "$scratch"
  chmod 600 "$scratch"
  mv "$scratch" "$validated"
fi
[[ -f "$validated" && ! -L "$validated" && "$(cat "$validated")" == "$commit" ]] || exit 1
echo "Main smoke verified: $commit; evidence: $smoke_dir"
