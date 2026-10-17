#!/usr/bin/env bash
# Scratch-only exact compile/execute contract, with fake Cargo and toolkit.
set -euo pipefail
scratch_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-static-extension.XXXXXX")"
retain_failure() {
  local status=$?
  if (( status != 0 )); then
    chmod 700 "$fixture" 2>/dev/null || true
    printf 'protected failed fixture: %s\n' "$fixture" >&2
    return "$status"
  fi
  rm -rf "$fixture"
}
trap retain_failure EXIT
project="$fixture/project"
mkdir -p "$project/toolkit/xtask/src/checks" "$project/toolkit/xtask/target/debug" \
  "$project/crates/aura-web/src/harness" "$project/crates/aura-harness/playwright-driver/src" "$fixture/fakebin"
project="$(cd "$project" && pwd -P)"
printf 'toolkit\n' > "$project/toolkit/xtask/src/main.rs"
printf 'policy\n' > "$project/toolkit/xtask/src/checks/policy.rs"
printf 'recipe\n' > "$project/justfile"
printf 'manifest\n' > "$project/toolkit/xtask/Cargo.toml"
printf 'lock\n' > "$project/toolkit/xtask/Cargo.lock"
printf 'flake\n' > "$project/flake.nix"
printf 'flake-lock\n' > "$project/flake.lock"
printf 'rust\n' > "$project/crates/aura-web/src/harness/driver_contract.rs"
printf 'ts\n' > "$project/crates/aura-harness/playwright-driver/src/driver_contract.ts"
printf 'ts\n' > "$project/crates/aura-harness/playwright-driver/src/playwright_driver.ts"
printf '/target/\n/artifacts/\n' > "$project/.gitignore"
git init -q "$project"
git -C "$project" add .
git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid \
  -c commit.gpgsign=false commit -qm initial
cat > "$fixture/fakebin/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
if [[ "$*" == -V || "$*" == -Vv ]]; then echo 'cargo fixture'; exit 0; fi
[[ "$*" == 'build --manifest-path toolkit/xtask/Cargo.toml --bin aura-toolkit-xtask' ]] || exit 2
printf 'compile\n' >> "$COMPILE_COUNTER"
FAKE_CARGO
cat > "$project/toolkit/xtask/target/debug/aura-toolkit-xtask" <<'FAKE_TOOL'
#!/usr/bin/env bash
[[ "$1" == check ]] || exit 2
case "$2" in user-flow-policy-guardrails|user-flow-guidance-sync|browser-driver-contract-sync) ;;
  *) exit 2;; esac
printf '%s\n' "$2" >> "$STATIC_COUNTER"
[[ ! -f "$FAIL_MARKER" ]] || exit 9
FAKE_TOOL
chmod +x "$fixture/fakebin/cargo" "$project/toolkit/xtask/target/debug/aura-toolkit-xtask"
export PATH="$fixture/fakebin:$PATH" COMPILE_COUNTER="$fixture/compile-count" \
  STATIC_COUNTER="$fixture/static-count" FAIL_MARKER="$fixture/fail"
source "$scratch_root/scripts/dev/build-receipt.sh"
aura_receipt_init "$project" policy-toolkit-compile bash toolkit-compile
compile_receipt="$AURA_BUILD_RECEIPT"
bash "$scratch_root/scripts/dev/run-policy-static.sh" --compile-stage --root "$project" \
  user-flow-policy-guardrails > "$fixture/compile-output"
aura_receipt_finish 0
execute() {
  local check="$1"
  aura_receipt_init "$project" policy-static-exec bash static-exec
  bash "$scratch_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$project" \
    --compile-receipt "$compile_receipt" "$check" > "$fixture/execute-output" 2>&1
}
execute user-flow-policy-guardrails
execute user-flow-guidance-sync
successful_execution_receipt="$AURA_BUILD_RECEIPT"
jq -e '.metadata.policy_static.git_identity_hash|test("^[a-f0-9]{64}$")' \
  "$successful_execution_receipt" >/dev/null
if AURA_UX_GUIDANCE_DIFF_RANGE=missing-revision execute user-flow-guidance-sync; then
  echo 'invalid selected guidance range was accepted by runner' >&2; exit 1
fi
if AURA_UX_POLICY_DIFF_RANGE=missing-revision execute user-flow-policy-guardrails; then
  echo 'invalid selected policy range was accepted by runner' >&2; exit 1
fi
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
[[ "$(wc -l < "$STATIC_COUNTER" | tr -d ' ')" == 2 ]]
# Original compile proof stays valid across unrelated current-source changes;
# execution reruns and can report the current typed failure.
printf 'changed\n' > "$project/unrelated.rs"
touch "$FAIL_MARKER"
if execute user-flow-guidance-sync; then
  echo 'fresh current-source failure was hidden' >&2; exit 1
fi
[[ "$(wc -l < "$STATIC_COUNTER" | tr -d ' ')" == 3 ]]
rm "$FAIL_MARKER"
# A failed or forged compile receipt cannot authorize execution.
cp "$compile_receipt" "$fixture/original-receipt"
jq '.final_exit_status=75' "$compile_receipt" > "$fixture/altered"
mv "$fixture/altered" "$compile_receipt"
if execute user-flow-policy-guardrails; then echo 'failed compile accepted' >&2; exit 1; fi
cp "$fixture/original-receipt" "$compile_receipt"
jq '.metadata.policy_toolkit.binary_hash="forged"' "$compile_receipt" > "$fixture/altered"
mv "$fixture/altered" "$compile_receipt"
if execute user-flow-policy-guardrails; then echo 'forged binary accepted' >&2; exit 1; fi
cp "$fixture/original-receipt" "$compile_receipt"
if execute security-boundary-policy; then echo 'security check misclassified static' >&2; exit 1; fi
printf 'change\n' >> "$project/toolkit/xtask/src/main.rs"
if execute user-flow-policy-guardrails; then echo 'stale toolkit compile accepted' >&2; exit 1; fi
git -C "$project" checkout -q -- toolkit/xtask/src/main.rs
printf 'tamper\n' >> "$project/toolkit/xtask/target/debug/aura-toolkit-xtask"
if execute user-flow-policy-guardrails; then echo 'tampered binary accepted' >&2; exit 1; fi
printf 'static extension: original compile reused, checks rerun, invalid receipts and security refused\n'
