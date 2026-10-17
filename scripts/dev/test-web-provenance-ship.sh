#!/usr/bin/env bash
# Actual prepared ship orchestration; isolated SSH/Nix/loader facades only.
set -euo pipefail
prepared=$(cd "$(dirname "$0")" && pwd -P)
fixture_parent=$(cd "${TMPDIR:-/tmp}" && pwd -P)
prefix=$(mktemp "$fixture_parent/aura-ship-fixture-prefix.XXXXXX")
awk '/^git clone -q/ {exit} {print}' "$prepared/test-web-prebuilt-transfer.sh" > "$prefix"
source "$prefix"
rm "$prefix"
public=target/dx/aura-web/release/web/public
mkdir -p "$producer/target/dx/aura-web/release/web" "$producer/scripts/harness/lan" "$producer/target/lan" "$producer/crates/aura-web/public/assets"
mv "$producer/target/public" "$producer/$public"
cp "$repo_root/scripts/harness/lan/"{drv,env,runtime-refs,ship-web-assets}.sh "$producer/scripts/harness/lan/"
cp "$repo_root/scripts/harness/lan/ship.sh" "$producer/scripts/harness/lan/ship.sh"
printf '.tmp/\n.nix-ship/\nbin/\n' >> "$producer/.gitignore"
printf 'stylesheet\n' > "$producer/crates/aura-web/public/assets/tailwind.css"
for name in aura tool_repl; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$producer/target/lan/$name"
  chmod +x "$producer/target/lan/$name"
done
git -C "$producer" add .
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid git -C "$producer" commit -qm ship-fixture
git -C "$producer" branch -M main
commit=$(git -C "$producer" rev-parse HEAD)
consumer="$fixture/remote checkout's"
git clone -q "$producer" "$consumer"
export FIXTURE_REMOTE_HOME="$fixture" FIXTURE_SHIP_CALLS="$fixture/ship-calls"
flock_bin=$(command -v flock)
export FIXTURE_RUNTIME_STORE="${flock_bin%/bin/flock}"
cat > "$fixture/bin/nix" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf 'nix:%s\n' "$1" >> "$FIXTURE_SHIP_CALLS"
case "$1" in
  copy) exit 0;;
  develop)
    [[ "$3" == --command ]] || exit 91
    shift 3
    export IN_NIX_SHELL=fixture
    exec "$@";;
  *) exit 92;;
esac
MOCK
cat > "$fixture/bin/nix-store" <<'MOCK'
#!/usr/bin/env bash
printf 'root:%s\n' "$*" >> "$FIXTURE_SHIP_CALLS"
MOCK
cat > "$fixture/bin/otool" <<'MOCK'
#!/usr/bin/env bash
printf '%s:\n\t%s/lib/fixture.dylib (compat)\n' "$2" "$FIXTURE_RUNTIME_STORE"
MOCK
cat > "$fixture/bin/ssh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
while [[ "${1:-}" == -o ]]; do shift 2; done
[[ "$1" == fixture-host ]] || exit 93
shift
command_text="$*"
unset AURA_E2E_ROOT AURA_E2E_RUN_DIR AURA_E2E_TOOL_REPL AURA_E2E_AURA_BIN
cd "$FIXTURE_REMOTE_HOME"
exec zsh -f -c "$command_text"
MOCK
chmod +x "$fixture/bin/"*
aura_web_write_prebuilt_provenance "$producer" "$producer/$public" "$inventory" \
  "$(aura_web_clean_commit "$producer")" "$(aura_web_clean_tree "$producer")" "$(aura_web_build_config_digest "$producer")"
touch -t 200001010000 "$producer/$public/index.html"
export AURA_E2E_ROOT="$producer" AURA_E2E_REMOTE=fixture-host AURA_E2E_REMOTE_ROOT="$consumer"
export AURA_NIX_BIN="$fixture/bin/nix" AURA_NIX_REMOTE_BIN="$fixture/bin"
export IN_NIX_SHELL=fixture AURA_E2E_HOST_ADDR=127.0.0.1
bash "$producer/scripts/harness/lan/ship.sh" --no-build > "$fixture/ship-output" 2>&1 || { cat "$fixture/ship-output" >&2; exit 1; }
rg -q 'shipped to fixture-host' "$fixture/ship-output"
[[ $(git -C "$consumer" rev-parse HEAD) == "$commit" ]]
aura_web_admitted_prebuilt_matches "$consumer" "$consumer/$public"
cmp "$producer/$public/.aura-prebuilt-provenance" "$consumer/$public/.aura-prebuilt-provenance"
file_mtime() {
  if stat --version >/dev/null 2>&1; then
    stat -c %Y "$1"
  else
    stat -f %m "$1"
  fi
}
[[ $(file_mtime "$producer/$public/index.html") == "$(file_mtime "$consumer/$public/index.html")" ]]
[[ -f "$consumer/artifacts/aura-web/transferred-provenance/selected" ]]
[[ -z $(find "$consumer/$public" -name 'evidence.json' -o -name 'prebuilt-tools-*') ]]
[[ -f "$producer/.tmp/e2e/run.lifecycle.lock" && -f "$consumer/.tmp/e2e/run.lifecycle.lock" ]]
inventory_hash=$(sed -n '6s/^tool_inventory_sha256=//p' "$producer/$public/.aura-prebuilt-provenance")
protected_inventory="$producer/artifacts/aura-web/private-provenance/prebuilt-tools-$commit-$inventory_hash"
mv "$protected_inventory" "$fixture/retained-producer-inventory"
before=$(sha256sum "$consumer/$public/index.html" | cut -d' ' -f1)
if bash "$producer/scripts/harness/lan/ship.sh" --no-build > "$fixture/refused-output" 2>&1; then
  echo 'full ship accepted absent original private inventory' >&2; exit 1
fi
rg -q 'ship: web producer provenance does not match' "$fixture/refused-output"
[[ $(sha256sum "$consumer/$public/index.html" | cut -d' ' -f1) == "$before" ]]
mv "$fixture/retained-producer-inventory" "$protected_inventory"
aura_web_admitted_prebuilt_matches "$consumer" "$consumer/$public"
echo 'prepared full ship exports/transfers/admits protected original provenance'
