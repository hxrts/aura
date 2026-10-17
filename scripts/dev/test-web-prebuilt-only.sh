#!/usr/bin/env bash
# Refuse implicit web compilation/cache deletion after a LAN harness starts.
set -euo pipefail
prepared="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git -C "$prepared" rev-parse --show-toplevel)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-web-prebuilt-test.XXXXXX")"
scratch="$(cd "$scratch" && pwd -P)"
trap 'rm -rf "$scratch"' EXIT
project="$scratch/project"
public="$project/target/dx/aura-web/release/web/public"
mkdir -p "$project/scripts/web" "$project/scripts/dev" "$project/crates/aura-web/node_modules/ws" \
  "$project/crates/aura-web/public/assets" "$project/crates/aura-web/src" \
  "$public/assets" "$scratch/bin"
cp "$repo_root/scripts/web/serve-static.sh" "$repo_root/scripts/web/log-bootstrap.sh" \
  "$repo_root/scripts/web/prebuilt-provenance.sh" "$project/scripts/web/"
cp "$repo_root/scripts/dev/policy-dependency-inputs.sh" "$project/scripts/dev/"
cp "$repo_root/scripts/web/prebuilt-transfer.sh" "$project/scripts/web/"
: > "$project/Cargo.toml"
: > "$project/Cargo.lock"
printf 'pinned-toolchain=v1\n' > "$project/flake.lock"
: > "$project/crates/aura-web/Cargo.toml"
: > "$project/crates/aura-web/src/main.rs"
: > "$project/crates/aura-web/public/assets/tailwind.css"
printf 'preserved\n' > "$public/evidence"
export CALLS_FILE="$scratch/calls" AURA_HARNESS_WEB_PREBUILT_ONLY=1
export PATH="$scratch/bin:$PATH"
for command in npm node; do
  cat > "$scratch/bin/$command" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${0##*/}" >> "$CALLS_FILE"
EOF
done
for command in cargo rustc dx wasm-bindgen wasm-opt; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$scratch/bin/$command"
done
cat > "$project/scripts/web/dx.sh" <<'EOF'
#!/usr/bin/env bash
if [[ "${1:-}" == --selected-tool-inventory ]]; then cp "$AURA_WEB_FIXTURE_INVENTORY" "$2"; exit; fi
echo dx >> "$CALLS_FILE"
exit 99
EOF
chmod +x "$scratch/bin/"* "$project/scripts/web/dx.sh"
source "$project/scripts/web/prebuilt-provenance.sh"
tool_inventory="$scratch/tool-inventory"
export AURA_WEB_FIXTURE_INVENTORY="$tool_inventory"
: > "$tool_inventory"
chmod 600 "$tool_inventory"
mkdir -p "$scratch/binaryen/lib"
printf 'library-v1\n' > "$scratch/binaryen/lib/libbinaryen.dylib"
for command in node npm cargo rustc dx wasm-bindgen wasm-opt; do
  aura_web_record_selected_tool "$tool_inventory" "$command" "$scratch/bin/$command"
done
aura_web_record_binaryen_libraries "$tool_inventory" "$scratch/binaryen/lib"
printf 'target/\nartifacts/\n' > "$project/.gitignore"
git -C "$project" init -q
git -C "$project" add .
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
  git -C "$project" commit -qm fixture
source "$project/scripts/web/prebuilt-provenance.sh"
valid_bundle() {
  rm -f "$public/.aura-prebuilt-provenance"
  printf 'index\n' > "$public/index.html"
  # Asset admission needs the exported marker, without installing a driver.
  printf 'const __AURA_HARNESS__ = {};\n' > "$public/assets/app.js"
  touch -t 203001010000 "$public/index.html"
  : > "$CALLS_FILE"
}
expect() {
  local wanted="$1" actual
  if bash "$project/scripts/web/serve-static.sh" > "$scratch/output" 2>&1; then actual=0; else actual=$?; fi
  [[ "$actual" == "$wanted" ]] || { cat "$scratch/output" >&2; exit 1; }
  ! rg -q '^dx$' "$CALLS_FILE"
  [[ "$(cat "$public/evidence")" == preserved ]]
}
valid_bundle
expect 1
[[ ! -s "$CALLS_FILE" ]]
[[ ! -e "$public/assets/tailwind.css" ]]
valid_bundle
rm "$public/index.html"
expect 1
! rg -q '^node$' "$CALLS_FILE"
[[ -f "$public/assets/app.js" ]]
rg -q 'stop the harness and run' "$scratch/output"
valid_bundle
touch -t 200001010000 "$public/index.html"
expect 1
[[ -f "$public/index.html" && -f "$public/assets/app.js" ]]
valid_bundle
touch -t 200001010000 "$public/index.html"
AURA_HARNESS_WEB_PREBUILT_ONLY=0 aura_web_write_prebuilt_provenance "$project" "$public" "$tool_inventory" \
  "$(aura_web_clean_commit "$project")" "$(aura_web_clean_tree "$project")" \
  "$(aura_web_build_config_digest "$project")"
expect 0
rg -q '^node$' "$CALLS_FILE"
! rg -q '^npm$' "$CALLS_FILE"
[[ ! -e "$public/assets/tailwind.css" ]]
[[ ! -d "$project/target/dx/aura-web/debug" ]]
rg -q 'reusing admitted prebuilt release' "$scratch/output"
rm -rf "$project/crates/aura-web/node_modules/ws"
: > "$CALLS_FILE"
expect 1
[[ ! -s "$CALLS_FILE" ]]
rg -q 'missing prebuilt server runtime dependency ws' "$scratch/output"
mkdir -p "$project/crates/aura-web/node_modules/ws"
printf 'changed source\n' > "$project/crates/aura-web/src/main.rs"
expect 1
git -C "$project" checkout -q -- crates/aura-web/src/main.rs
printf 'tampered asset\n' > "$public/assets/app.js"
expect 1
valid_bundle
printf 'no harness support\n' > "$public/assets/app.js"
expect 1
[[ -f "$public/index.html" ]]
valid_bundle
export AURA_HARNESS_WEB_BUILD_PROFILE=invalid
expect 1
unset AURA_HARNESS_WEB_BUILD_PROFILE
export AURA_HARNESS_WEB_PREBUILT_ONLY=invalid
expect 2
echo 'LAN prebuilt web safety tests passed'
