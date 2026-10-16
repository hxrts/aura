#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-ownership-fast-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'if [[ "${AURA_KEEP_POLICY_FIXTURE:-0}" != 1 ]]; then rm -rf "$fixture"; else echo "fixture=$fixture" >&2; fi' EXIT
project="$fixture/project"
mkdir -p "$project/crates/demo/src" "$project/target/debug" "$fixture/fakebin"
git init -q "$project"
printf '[package]\nname="fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf 'pub fn owned() {}\n' > "$project/crates/demo/src/lib.rs"
printf '/target/\n/artifacts/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml crates/demo/src/lib.rs .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
cat > "$fixture/fakebin/cargo" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == -V || "$*" == -Vv ]]; then printf 'cargo fixture 1.93\n'; exit 0; fi
[[ "$*" == 'build -p hxrts-aura-macros --bin ownership_lints' ]] || exit 2
printf 'compile\n' >> "$COMPILE_COUNTER"
EOF
cat > "$project/target/debug/ownership_lints" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$1" >> "$LINT_COUNTER"
[[ "${FAIL_MODE:-}" != "$1" ]]
EOF
chmod +x "$fixture/fakebin/cargo" "$project/target/debug/ownership_lints"
export PATH="$fixture/fakebin:$PATH" COMPILE_COUNTER="$fixture/compile-count" LINT_COUNTER="$fixture/lint-count"
source "$repo_root/scripts/dev/build-receipt.sh"
export AURA_BUILD_PACKAGES=hxrts-aura-macros
aura_receipt_init "$project" ownership-lint-compile bash ownership-lint-compile
run_fast() {
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
    --compile-receipt "$compile_receipt" crates/demo/src/lib.rs
}
compile_fast() {
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --compile-stage --root "$project" \
    crates/demo/src/lib.rs
}
compile_fast > "$fixture/output"
compile_receipt="$AURA_BUILD_RECEIPT"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
jq -e '.metadata.policy_binary_hash!=null and
  [.phases[].phase]==["compile"]' "$compile_receipt" >/dev/null
aura_receipt_finish 0
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
run_fast > "$fixture/output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
[[ "$(wc -l < "$LINT_COUNTER" | tr -d ' ')" == 6 ]]
jq -e '.metadata.source_scope==["crates/demo/src/lib.rs"] and
  (.metadata.policy_modes | length)==6 and
  .metadata.policy_compile_receipt!=null and
  ([.phases[].phase] == ["execution","execution","execution","execution","execution","execution"]) and
  ([.phases[].exit_status] | all(.==0))' "$AURA_BUILD_RECEIPT" >/dev/null
if bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
  --compile-receipt "$compile_receipt" \
  ../outside.rs > "$fixture/output" 2>&1; then exit 1; fi
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
if FAIL_MODE=optional-owner-boundary run_fast > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[] | select(.phase=="execution" and .exit_status!=0)] | length==1' \
  "$AURA_BUILD_RECEIPT" >/dev/null
mkdir -p "$fixture/shared"
AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=2 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --root "$project" \
    crates/demo/src/lib.rs > "$fixture/packet-output"
packet_compile="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '1p')"
packet_execute="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '2p')"
[[ -f "$packet_compile" && -f "$packet_execute" && "$packet_compile" != "$packet_execute" ]]
jq -e '.lane=="ownership-lint-compile" and .final_exit_status==0 and
  ([.phases[] | .phase] | index("compile")!=null) and
  ([.phases[] | .phase] | index("command")!=null)' "$packet_compile" >/dev/null
jq -e --arg parent "$packet_compile" '.lane=="ownership-lint-exec" and
  .final_exit_status==0 and .metadata.policy_compile_receipt==$parent and
  ([.phases[] | select(.phase=="execution")] | length)==6' "$packet_execute" >/dev/null
if AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=999999 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane ownership-lint-exec --no-prune -- \
    bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage \
      --root "$project" --compile-receipt "$packet_compile" crates/demo/src/lib.rs \
    > "$fixture/refused-output" 2>&1; then exit 1; fi
refused="$(sed -n 's/^Build receipt: //p' "$fixture/refused-output" | tail -1)"
jq -e '.admission.admitted!=true and .final_exit_status!=0 and
  ([.phases[] | select(.phase=="execution")] | length)==0' "$refused" >/dev/null
jq -e '.final_exit_status==0 and
  ([.phases[] | select(.phase=="compile")] | length)==1' "$packet_compile" >/dev/null
prior_compiles="$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')"
AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=2 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --root "$project" \
    --compile-receipt "$packet_compile" crates/demo/src/lib.rs \
    > "$fixture/resumed-output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == "$prior_compiles" ]]
resumed="$(sed -n 's/^Build receipt: //p' "$fixture/resumed-output" | tail -1)"
jq -e --arg parent "$packet_compile" '.lane=="ownership-lint-exec" and
  .final_exit_status==0 and .metadata.policy_compile_receipt==$parent' "$resumed" >/dev/null
echo 'ownership fast: real guarded split receipts, six modes, invalid path and lint failure passed'
