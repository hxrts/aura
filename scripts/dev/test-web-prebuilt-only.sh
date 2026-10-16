#!/usr/bin/env bash
# Refuse implicit web compilation/cache deletion after a LAN harness starts.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-web-prebuilt-test.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
project="$scratch/project"
public="$project/target/dx/aura-web/release/web/public"
mkdir -p "$project/scripts/web" "$project/crates/aura-web/node_modules/ws" \
  "$project/crates/aura-web/public/assets" "$project/crates/aura-web/src" \
  "$public/assets" "$scratch/bin"
cp "$repo_root/scripts/web/serve-static.sh" "$repo_root/scripts/web/log-bootstrap.sh" \
  "$repo_root/scripts/web/prebuilt-provenance.sh" "$project/scripts/web/"
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
cat > "$project/scripts/web/dx.sh" <<'EOF'
#!/usr/bin/env bash
echo dx >> "$CALLS_FILE"
exit 99
EOF
chmod +x "$scratch/bin/"* "$project/scripts/web/dx.sh"
printf 'target/\n' > "$project/.gitignore"
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
expect 0
rg -q '^node$' "$CALLS_FILE"
rg -q 'reusing prebuilt release' "$scratch/output"
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
aura_web_write_prebuilt_provenance "$project" "$public"
expect 0
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
