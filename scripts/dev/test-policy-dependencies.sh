#!/usr/bin/env bash
# Isolated dependency receipts; no workspace compilation or policy bypass.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-policy-deps.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
env -i PATH="$PATH" FIXTURE_ONLY_SECRET=fixture-only bash -xc \
  'source "$1"; policy_exported_names' _ \
  "$repo_root/scripts/dev/policy-dependency-inputs.sh" \
  > "$fixture/exported-names" 2> "$fixture/exported-trace"
[[ -s "$fixture/exported-names" ]]
if rg -q 'fixture-only|FIXTURE_ONLY_SECRET=' \
    "$fixture/exported-names" "$fixture/exported-trace"; then
  echo 'exported value leaked under Bash tracing' >&2; exit 1
fi
project="$fixture/project"
mkdir -p "$project/toolkit/xtask/src" "$project/toolkit/xtask/target/debug" \
  "$project/crates/aura-web/src/harness" \
  "$project/crates/aura-harness/playwright-driver/src" "$fixture/fakebin"
printf 'toolkit\n' > "$project/toolkit/xtask/src/main.rs"
printf '[package]\nname="policy-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf 'manifest\n' > "$project/toolkit/xtask/Cargo.toml"
printf 'lock\n' > "$project/toolkit/xtask/Cargo.lock"
printf 'flake\n' > "$project/flake.nix"
printf 'flake-lock\n' > "$project/flake.lock"
printf 'rust contract\n' > "$project/crates/aura-web/src/harness/driver_contract.rs"
printf 'ts contract\n' > "$project/crates/aura-harness/playwright-driver/src/driver_contract.ts"
printf 'ts driver\n' > "$project/crates/aura-harness/playwright-driver/src/playwright_driver.ts"
printf '/target/\n/artifacts/\n/toolkit/xtask/target/\n' > "$project/.gitignore"
git init -q "$project"
git -C "$project" add .
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
cat > "$fixture/fakebin/cargo" <<'FAKE_CARGO'
#!/usr/bin/env bash
if [[ "$*" == -V || "$*" == -Vv ]]; then printf 'cargo fixture 1.93\n'; exit 0; fi
[[ "$*" == 'build --manifest-path toolkit/xtask/Cargo.toml --bin aura-toolkit-xtask' ]] || exit 2
printf 'compile\n' >> "$COMPILE_COUNTER"
FAKE_CARGO
cat > "$project/toolkit/xtask/target/debug/aura-toolkit-xtask" <<'FAKE_TOOL'
#!/usr/bin/env bash
[[ "$*" == 'check browser-driver-contract-sync' ]] || exit 2
printf 'check\n' >> "$STATIC_COUNTER"
FAKE_TOOL
chmod +x "$fixture/fakebin/cargo" "$project/toolkit/xtask/target/debug/aura-toolkit-xtask"
export PATH="$fixture/fakebin:$PATH" COMPILE_COUNTER="$fixture/compile-count" \
  STATIC_COUNTER="$fixture/static-count"
source "$repo_root/scripts/dev/build-receipt.sh"
aura_receipt_init "$project" policy-toolkit-compile bash toolkit-compile
bash "$repo_root/scripts/dev/run-policy-static.sh" --compile-stage --root "$project" \
  browser-driver-contract-sync > "$fixture/output"
compile_receipt="$AURA_BUILD_RECEIPT"
aura_receipt_finish 0
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
jq -e '.metadata.policy_toolkit.binary_hash!=null and
  [.phases[].phase]==["compile"]' "$compile_receipt" >/dev/null
execute() {
  aura_receipt_init "$project" policy-static-exec bash static-exec
  bash "$repo_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$project" \
    --compile-receipt "$compile_receipt" browser-driver-contract-sync > "$fixture/output" 2>&1
}
execute
[[ "$(wc -l < "$STATIC_COUNTER" | tr -d ' ')" == 1 ]]
jq -e --arg parent "$compile_receipt" \
  '.metadata.policy_static.compile_receipt==$parent and
   .metadata.policy_static.check=="browser-driver-contract-sync" and
   (.metadata.policy_static.scope|length)==3 and
   [.phases[].phase]==["execution"] and .phases[0].exit_status==0' \
  "$AURA_BUILD_RECEIPT" >/dev/null
# An unrelated source change does not invalidate the toolkit compile identity.
printf 'unrelated\n' > "$project/other.rs"
execute
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
printf 'changed\n' >> "$project/crates/aura-web/src/harness/driver_contract.rs"
execute
[[ "$(wc -l < "$STATIC_COUNTER" | tr -d ' ')" == 3 ]]
# A changed toolkit implementation refuses the original compile dependency.
printf 'changed\n' >> "$project/toolkit/xtask/src/main.rs"
aura_receipt_init "$project" policy-static-exec bash static-exec
if bash "$repo_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$project" \
  --compile-receipt "$compile_receipt" browser-driver-contract-sync > "$fixture/output" 2>&1; then exit 1; fi
git -C "$project" checkout -q -- toolkit/xtask/src/main.rs
# Admission refusal and failed compile cannot satisfy the static check.
cp "$compile_receipt" "$fixture/compile-original.json"
jq '.final_exit_status=75 | .admission={admitted:false,reason:"fixture"}' \
  "$compile_receipt" > "$fixture/altered"
mv "$fixture/altered" "$compile_receipt"
aura_receipt_init "$project" policy-static-exec bash static-exec
if bash "$repo_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$project" \
  --compile-receipt "$compile_receipt" browser-driver-contract-sync > "$fixture/output" 2>&1; then exit 1; fi
cp "$fixture/compile-original.json" "$compile_receipt"
cp "$project/toolkit/xtask/target/debug/aura-toolkit-xtask" "$fixture/binary-original"
printf 'tamper\n' >> "$project/toolkit/xtask/target/debug/aura-toolkit-xtask"
aura_receipt_init "$project" policy-static-exec bash static-exec
if bash "$repo_root/scripts/dev/run-policy-static.sh" --execution-stage --root "$project" \
  --compile-receipt "$compile_receipt" browser-driver-contract-sync > "$fixture/output" 2>&1; then exit 1; fi
cp "$fixture/binary-original" "$project/toolkit/xtask/target/debug/aura-toolkit-xtask"
mkdir -p "$fixture/shared"
AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=2 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/run-policy-static.sh" --root "$project" \
    browser-driver-contract-sync > "$fixture/packet-output"
packet_compile="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '1p')"
packet_execute="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '2p')"
[[ -f "$packet_compile" && -f "$packet_execute" && "$packet_compile" != "$packet_execute" ]]
jq -e '.lane=="policy-toolkit-compile" and .final_exit_status==0 and
  ([.phases[] | .phase] | index("compile")!=null) and
  ([.phases[] | .phase] | index("command")!=null)' "$packet_compile" >/dev/null
jq -e --arg parent "$packet_compile" '.lane=="policy-static-exec" and
  .final_exit_status==0 and .metadata.policy_static.compile_receipt==$parent and
  ([.phases[] | select(.phase=="execution")] | length)==1' "$packet_execute" >/dev/null
source "$repo_root/scripts/dev/policy-dependency-inputs.sh"
printf 'fn main() {}\n' > "$project/toolkit/xtask/build.rs"
if policy_toolkit_inputs "$project" > "$fixture/output"; then exit 1; fi
rm "$project/toolkit/xtask/build.rs"
printf 'newdep = { path = "../newdep" }\n' >> "$project/toolkit/xtask/Cargo.toml"
if policy_toolkit_inputs "$project" > "$fixture/output"; then exit 1; fi
git -C "$project" checkout -q -- toolkit/xtask/Cargo.toml
printf 'build = "custom-build.rs"\n' >> "$project/toolkit/xtask/Cargo.toml"
if policy_toolkit_inputs "$project" > "$fixture/output"; then exit 1; fi
git -C "$project" checkout -q -- toolkit/xtask/Cargo.toml
ln -s main.rs "$project/toolkit/xtask/src/linked.rs"
if policy_toolkit_inputs "$project" > "$fixture/output"; then exit 1; fi
rm "$project/toolkit/xtask/src/linked.rs"
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/fakebin/wrapper"
chmod +x "$fixture/fakebin/wrapper"
wrapper_before="$(RUSTC_WORKSPACE_WRAPPER="$fixture/fakebin/wrapper" policy_tool_identity "$project" "$project")"
printf '# updated\n' >> "$fixture/fakebin/wrapper"
wrapper_after="$(RUSTC_WORKSPACE_WRAPPER="$fixture/fakebin/wrapper" policy_tool_identity "$project" "$project")"
[[ "$wrapper_before" != "$wrapper_after" ]]
mkdir -p "$fixture/cargo-home"
printf '[build]\n' > "$fixture/cargo-home/config.toml"
config_before="$(CARGO_HOME="$fixture/cargo-home" policy_tool_identity "$project" "$project")"
printf 'jobs = 1\n' >> "$fixture/cargo-home/config.toml"
config_after="$(CARGO_HOME="$fixture/cargo-home" policy_tool_identity "$project" "$project")"
[[ "$config_before" != "$config_after" ]]
ancestor_before="$(policy_tool_identity "$project" "$project")"
mkdir -p "$fixture/.cargo"
printf '[build]\njobs = 1\n' > "$fixture/.cargo/config.toml"
ancestor_added="$(policy_tool_identity "$project" "$project")"
[[ "$ancestor_before" != "$ancestor_added" ]]
printf '[build]\njobs = 2\n' > "$fixture/.cargo/config.toml"
ancestor_changed="$(policy_tool_identity "$project" "$project")"
[[ "$ancestor_added" != "$ancestor_changed" ]]
rm "$fixture/.cargo/config.toml"
[[ "$ancestor_before" == "$(policy_tool_identity "$project" "$project")" ]]
rmdir "$fixture/.cargo"
ln -s "$project/.cargo" "$fixture/.cargo"
if policy_tool_identity "$project" "$project" > "$fixture/output"; then
  echo 'linked ancestor Cargo config directory was accepted' >&2; exit 1
fi
rm "$fixture/.cargo"
cat > "$project/Justfile" <<'JUST'
_policy-check *ARGS:
    AURA_BUILD_WAIT_SECONDS=1800 bash scripts/dev/build-budget.sh --lane gates -- cargo run --quiet --manifest-path toolkit/xtask/Cargo.toml -- {{ ARGS }}
ci-ownership-policy:
    just _policy-check check ownership-policy
JUST
git -C "$project" add Justfile .gitignore other.rs \
  crates/aura-web/src/harness/driver_contract.rs
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm justfile
# A synthetic receipt checks the verifier's refusal logic; it is never used as
# actual aggregate acceptance evidence.
aura_receipt_init "$project" gates cargo run --quiet --manifest-path \
  toolkit/xtask/Cargo.toml -- check ownership-policy
aggregate="$AURA_BUILD_RECEIPT"
aura_receipt_update '.admission={admitted:true} | .command_exit_status=0 |
  .final_exit_status=0 | .phases=[{phase:"command",exit_status:0}]'
bash "$repo_root/scripts/dev/verify-policy-aggregate.sh" --root "$project" \
  --receipt "$aggregate" > "$fixture/output"
jq '.command_argv[7]="security-boundary-policy"' "$aggregate" > "$fixture/altered"
mv "$fixture/altered" "$aggregate"
if bash "$repo_root/scripts/dev/verify-policy-aggregate.sh" --root "$project" \
  --receipt "$aggregate" > "$fixture/output" 2>&1; then exit 1; fi
printf 'changed source\n' >> "$project/other.rs"
if bash "$repo_root/scripts/dev/verify-policy-aggregate.sh" --root "$project" \
  --receipt "$aggregate" > "$fixture/output" 2>&1; then exit 1; fi
echo 'policy dependencies: separate compile/static receipts; unrelated, relevant, refused, changed-tool and tampered-binary cases passed'
