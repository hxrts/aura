#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-ownership-fast-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
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
aura_receipt_init "$project" ownership-fast bash ownership-fast
run_fast() {
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --inside --root "$project" \
    crates/demo/src/lib.rs
}
run_fast > "$fixture/output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
[[ "$(wc -l < "$LINT_COUNTER" | tr -d ' ')" == 6 ]]
jq -e '.metadata.source_scope==["crates/demo/src/lib.rs"] and
  (.metadata.policy_modes | length)==6 and
  ([.phases[].phase] == ["compile","execution","execution","execution","execution","execution","execution"]) and
  ([.phases[].exit_status] | all(.==0))' "$AURA_BUILD_RECEIPT" >/dev/null
if bash "$repo_root/scripts/dev/run-ownership-fast.sh" --inside --root "$project" \
  ../outside.rs > "$fixture/output" 2>&1; then exit 1; fi
aura_receipt_init "$project" ownership-fast bash ownership-fast
if FAIL_MODE=optional-owner-boundary run_fast > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[] | select(.phase=="execution" and .exit_status!=0)] | length==1' \
  "$AURA_BUILD_RECEIPT" >/dev/null
echo 'ownership fast: scoped source, one compile, six recorded lint phases, invalid path and lint failure passed'
