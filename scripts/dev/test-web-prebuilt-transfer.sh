#!/usr/bin/env bash
set -euo pipefail
prepared=$(cd "$(dirname "$0")" && pwd -P)
repo_root=$(git -C "$prepared" rev-parse --show-toplevel)
source "$repo_root/scripts/web/prebuilt-provenance.sh"
source "$repo_root/scripts/web/prebuilt-transfer.sh"
fixture_parent=$(cd "${TMPDIR:-/tmp}" && pwd -P)
fixture=$(mktemp -d "$fixture_parent/aura-web-transfer.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
producer="$fixture/producer"
consumer="$fixture/consumer checkout's"
mkdir -p "$producer/scripts/web" "$producer/scripts/dev" "$producer/crates/aura-web" "$producer/target/public" "$fixture/bin" "$fixture/binaryen/lib" "$fixture/cargo-home"
cp "$repo_root/scripts/dev/policy-dependency-inputs.sh" "$producer/scripts/dev/"
cp "$repo_root/scripts/web/prebuilt-provenance.sh" "$repo_root/scripts/web/prebuilt-transfer.sh" "$producer/scripts/web/"
for name in node npm cargo rustc dx wasm-bindgen wasm-opt; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/$name"
  chmod +x "$fixture/bin/$name"
done
export PATH="$fixture/bin:$PATH" CARGO_HOME="$fixture/cargo-home"
printf 'library\n' > "$fixture/binaryen/lib/libbinaryen.dylib"
inventory="$fixture/inventory"
: > "$inventory"; chmod 600 "$inventory"
for name in node npm cargo rustc dx wasm-bindgen wasm-opt; do
  aura_web_record_selected_tool "$inventory" "$name" "$fixture/bin/$name"
done
aura_web_record_binaryen_libraries "$inventory" "$fixture/binaryen/lib"
export FIXTURE_INVENTORY="$inventory"
cat > "$producer/scripts/web/dx.sh" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == --selected-tool-inventory ]] || exit 2
cp "$FIXTURE_INVENTORY" "$2"
MOCK
printf 'target/\nartifacts/\n' > "$producer/.gitignore"
printf 'source\n' > "$producer/source.rs"
printf '[package]\nname="aura-web"\nversion="0.0.0"\n' > "$producer/crates/aura-web/Cargo.toml"
printf 'lock\n' > "$producer/flake.lock"
git -C "$producer" init -q
git -C "$producer" add .
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid git -C "$producer" commit -qm fixture
printf 'index\n' > "$producer/target/public/index.html"
printf 'wasm\n' > "$producer/target/public/app.wasm"
aura_web_write_prebuilt_provenance "$producer" "$producer/target/public" "$inventory" \
  "$(aura_web_clean_commit "$producer")" "$(aura_web_clean_tree "$producer")" "$(aura_web_build_config_digest "$producer")"
evidence="$fixture/export.json"
: > "$evidence"; chmod 600 "$evidence"
bash "$producer/scripts/web/prebuilt-transfer.sh" export "$producer" "$producer/target/public" "$evidence"
aura_web_admitted_prebuilt_matches "$producer" "$producer/target/public"
expected=$(sha256sum "$evidence" | cut -d' ' -f1)
git clone -q "$producer" "$consumer"
mkdir -p "$consumer/target/public"
cp -p "$producer/target/public/"* "$consumer/target/public/"
cp -p "$producer/target/public/.aura-prebuilt-provenance" "$consumer/target/public/"
# Different checkout path is not local producer equality.
if aura_web_prebuilt_provenance_matches "$consumer" "$consumer/target/public"; then
  echo 'foreign consumer passed strict local producer checks' >&2; exit 1
fi
# Missing admission refuses without creating directories.
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then exit 1; fi
[[ ! -e "$consumer/artifacts" ]]
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$(printf '0%.0s' {1..64})" < "$evidence"; then
  echo 'wrong expected digest admitted' >&2; exit 1
fi
bad_evidence="$fixture/bad.json"
jq '.tools|="foreign|/foreign|bad\n"' "$evidence" > "$bad_evidence"
bad_hash=$(sha256sum "$bad_evidence" | cut -d' ' -f1)
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$bad_hash" < "$bad_evidence"; then
  echo 'foreign inventory admitted' >&2; exit 1
fi
dd if=/dev/zero of="$fixture/oversized" bs=1048577 count=1 2>/dev/null
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$expected" < "$fixture/oversized"; then
  echo 'oversized evidence admitted' >&2; exit 1
fi
[[ -z $(find "$consumer/artifacts/aura-web/transferred-provenance" -name 'pending.*' -print -quit) ]]
exec {held_fd}> "$consumer/artifacts/aura-web/transferred-provenance/.install.lock"
flock "$held_fd"
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$expected" < "$evidence"; then
  echo 'concurrent locked publisher admitted' >&2; exit 1
fi
flock -u "$held_fd"
exec {held_fd}>&-
chmod 777 "$consumer/artifacts"
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$expected" < "$evidence"; then
  echo 'writable parent admitted' >&2; exit 1
fi
chmod 700 "$consumer/artifacts"
protected_dir="$consumer/artifacts/aura-web/transferred-provenance"
mv "$protected_dir" "$fixture/saved-provenance"
mkdir -m 700 "$fixture/foreign-provenance"
ln -s "$fixture/foreign-provenance" "$protected_dir"
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$expected" < "$evidence"; then
  echo 'symlink evidence destination admitted' >&2; exit 1
fi
[[ -z $(find "$fixture/foreign-provenance" -mindepth 1 -print -quit) ]]
rm "$protected_dir"
mv "$fixture/saved-provenance" "$protected_dir"
jq '.platform="foreign-platform"' "$evidence" > "$bad_evidence"
bad_hash=$(sha256sum "$bad_evidence" | cut -d' ' -f1)
if aura_web_install_transfer "$consumer" "$consumer/target/public" "$bad_hash" < "$bad_evidence"; then
  echo 'foreign platform admitted' >&2; exit 1
fi
# Inject a real process signal at the named input-read boundary, after the
# real installer acquired its lock and created protected staging. The override
# only selects that fault boundary; the actual installer owns all state.
export FIXTURE_PROVENANCE_HELPER="$repo_root/scripts/web/prebuilt-provenance.sh"
export FIXTURE_TRANSFER_HELPER="$repo_root/scripts/web/prebuilt-transfer.sh"
cat > "$fixture/interrupted-install.sh" <<'CHILD'
#!/usr/bin/env bash
set -euo pipefail
source "$FIXTURE_PROVENANCE_HELPER"
source "$FIXTURE_TRANSFER_HELPER"
head() {
  if [[ "${FIXTURE_FAULT_BOUNDARY:-input}" == input ]]; then
    kill -"$FIXTURE_KILL_SIGNAL" "$BASHPID"; return 125
  fi
  command head "$@"
}
mv() {
  command mv "$@" || return
  if [[ "${FIXTURE_FAULT_BOUNDARY:-input}" == ready && $# == 2 && "$2" == */ready-* ]]; then
    kill -"$FIXTURE_KILL_SIGNAL" "$BASHPID"; return 125
  fi
}
aura_web_install_transfer "$@"
CHILD
for signal in TERM KILL; do
  if FIXTURE_KILL_SIGNAL="$signal" bash "$fixture/interrupted-install.sh" "$consumer" "$consumer/target/public" "$expected" < "$evidence" > "$fixture/interruption" 2>&1; then
    echo 'interrupted install succeeded' >&2; exit 1
  else
    status=$?
  fi
  if [[ "$signal" == TERM ]]; then
    [[ "$status" == 143 ]]
    [[ -z $(find "$consumer/artifacts/aura-web/transferred-provenance" -name 'pending.*' -print -quit) ]]
  else
    [[ "$status" == 137 ]]
    orphan=$(find "$consumer/artifacts/aura-web/transferred-provenance" -name 'pending.*' -type d -print -quit)
    [[ -n "$orphan" && $(aura_web_mode "$orphan") == 700 ]]
    aura_web_private_file "$orphan/evidence.json"
  fi
  if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then
    echo 'interrupted stage authorized serving' >&2; exit 1
  fi
done
if FIXTURE_FAULT_BOUNDARY=ready FIXTURE_KILL_SIGNAL=TERM bash "$fixture/interrupted-install.sh" "$consumer" "$consumer/target/public" "$expected" < "$evidence" > "$fixture/interruption" 2>&1; then
  echo 'ready-only interrupted install succeeded' >&2; exit 1
else
  [[ $? == 143 ]]
fi
[[ -n $(find "$consumer/artifacts/aura-web/transferred-provenance" -name 'ready-*' -type d -print -quit) ]]
[[ ! -e "$consumer/artifacts/aura-web/transferred-provenance/selected" ]]
if aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"; then
  echo 'ready-only state authorized consumer admission' >&2; exit 1
fi
# Execute the exact prepared ship command block through an isolated SSH/Nix
# facade. This tests quoting and actual remote CLI dispatch, not SSH crypto.
mkdir -p "$fixture/remote-bin"
cat > "$fixture/remote-bin/nix" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == develop && "$3" == --command ]] || exit 91
shift 3
exec "$@"
MOCK
chmod +x "$fixture/remote-bin/nix"
ssh() {
  [[ "$1" == -o && "$2" == BatchMode=yes && "$3" == -o && "$4" == StrictHostKeyChecking=yes && "$5" == fixture-host && $# == 6 ]] || return 92
  bash -c "$6"
}
remote_root="$consumer"
remote_bin="$fixture/remote-bin"
web_public=target/public
web_transfer_digest="$expected"
web_transfer_evidence="$evidence"
AURA_E2E_REMOTE=fixture-host
awk '/^printf -v remote_provenance_cmd / {copy=1} copy {print} copy && /"\$remote_provenance_cmd" < "\$web_transfer_evidence"/ {exit}' \
  "$repo_root/scripts/harness/lan/ship.sh" > "$fixture/ship-transfer-block.sh"
[[ -s "$fixture/ship-transfer-block.sh" ]]
source "$fixture/ship-transfer-block.sh"
unset -f ssh
bash "$consumer/scripts/web/prebuilt-transfer.sh" verify "$consumer" "$consumer/target/public"
aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"
# The uncatchable kill leaves a protected, unpublished orphan. A valid retry
# reacquires the released lock and publishes independently of that orphan.
rm -rf -- "$orphan"
aura_web_install_transfer "$consumer" "$consumer/target/public" "$expected" < "$evidence"
[[ -z $(find "$consumer/artifacts/aura-web/transferred-provenance" -name 'pending.*' -print -quit) ]]
aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"
aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"
# A selected transfer must never try local matching when its evidence fails.
record_key=$(sha256sum "$consumer/target/public/.aura-prebuilt-provenance" | cut -d' ' -f1)
ready="$consumer/artifacts/aura-web/transferred-provenance/ready-$record_key"
mv "$ready" "$fixture/retained-ready"
(
  aura_web_prebuilt_provenance_matches() { : > "$fixture/fallback-called"; return 0; }
  if aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"; then
    echo 'selected transfer fell back to local producer' >&2; exit 1
  fi
  [[ ! -e "$fixture/fallback-called" ]]
)
mv "$fixture/retained-ready" "$ready"
selected="$consumer/artifacts/aura-web/transferred-provenance/selected"
chmod 644 "$selected"
if aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"; then
  echo 'unsafe consumer selection admitted' >&2; exit 1
fi
chmod 600 "$selected"
cp -p "$selected" "$fixture/selected-backup"
printf 'foreign\n' > "$selected"
if aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"; then
  echo 'foreign consumer selection admitted' >&2; exit 1
fi
cp -p "$fixture/selected-backup" "$selected"
aura_web_admitted_prebuilt_matches "$consumer" "$consumer/target/public"
if aura_web_select_local_producer "$consumer" "$consumer/target/public"; then
  echo 'consumer manufactured local producer selection' >&2; exit 1
fi
cmp -s "$fixture/selected-backup" "$selected"
[[ ! -e "$consumer/target/public/evidence.json" ]]
foreign_consumer="$fixture/foreign-consumer"
git clone -q "$producer" "$foreign_consumer"
mkdir -p "$foreign_consumer/target"
cp -Rp "$consumer/target/public" "$foreign_consumer/target/"
cp -Rp "$consumer/artifacts" "$foreign_consumer/"
if aura_web_transferred_provenance_matches "$foreign_consumer" "$foreign_consumer/target/public"; then
  echo 'copied destination admission accepted foreign checkout' >&2; exit 1
fi
printf '\n' >> "$consumer/target/public/.aura-prebuilt-provenance"
if aura_web_transfer_payload_matches "$consumer" "$consumer/target/public" "$evidence"; then
  echo 'payload verifier accepted changed original marker bytes' >&2; exit 1
fi
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then
  echo 'modified marker bytes admitted' >&2; exit 1
fi
cp -p "$producer/target/public/.aura-prebuilt-provenance" "$consumer/target/public/"
printf 'changed source\n' >> "$consumer/source.rs"
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then exit 1; fi
git -C "$consumer" checkout -q -- source.rs
# Payload, producer evidence and required private permissions are independent.
printf 'tampered\n' >> "$consumer/target/public/app.wasm"
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then exit 1; fi
cp -p "$producer/target/public/app.wasm" "$consumer/target/public/app.wasm"
record_key=$(sha256sum "$consumer/target/public/.aura-prebuilt-provenance" | cut -d' ' -f1)
record="$consumer/artifacts/aura-web/transferred-provenance/ready-$record_key/evidence.json"
chmod 644 "$record"
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then exit 1; fi
chmod 600 "$record"
printf ' ' >> "$record"
if aura_web_transferred_provenance_matches "$consumer" "$consumer/target/public"; then exit 1; fi
aura_web_install_transfer "$producer" "$producer/target/public" "$expected" < "$evidence"
aura_web_admitted_prebuilt_matches "$producer" "$producer/target/public"
aura_web_select_local_producer "$producer" "$producer/target/public"
[[ ! -e "$producer/artifacts/aura-web/transferred-provenance/selected" ]]
[[ -n $(find "$producer/artifacts/aura-web/transferred-provenance" -name 'ready-*' -type d -print -quit) ]]
aura_web_admitted_prebuilt_matches "$producer" "$producer/target/public"
(
  uname() {
    if [[ "$*" == -sm ]]; then return 47; fi
    command uname "$@"
  }
  if aura_web_transfer_payload_matches "$consumer" "$consumer/target/public" "$evidence"; then
    echo 'failed platform observation admitted transferred evidence' >&2; exit 1
  fi
  : > "$fixture/failed-uname-export.json"
  chmod 600 "$fixture/failed-uname-export.json"
  if aura_web_export_transfer "$producer" "$producer/target/public" "$fixture/failed-uname-export.json"; then
    echo 'failed platform observation exported producer evidence' >&2; exit 1
  fi
)
# A failed clean-status command must not become an empty clean result while
# the real rev-parse still succeeds.
(
  git() {
    if [[ " $* " == *' status '* ]]; then return 47; fi
    command git "$@"
  }
  if aura_web_clean_commit "$producer" >/dev/null; then
    echo 'failed Git status admitted a clean producer' >&2; exit 1
  fi
)
printf 'prepared transfer cross-root, payload, permission, evidence and no-mutation cases passed\n'
