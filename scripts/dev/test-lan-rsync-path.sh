#!/usr/bin/env bash
set -euo pipefail
prepared=$(cd "$(dirname "$0")" && pwd -P)
repo_root=$(git -C "$prepared" rev-parse --show-toplevel)
fixture_parent=$(cd "${TMPDIR:-/tmp}" && pwd -P)
fixture=$(mktemp -d "$fixture_parent/aura-rsync-path.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
printf 'payload\n' > "$fixture/source"
mkdir -p "$fixture/destination's space"
cat > "$fixture/ssh" <<'MOCK'
#!/usr/bin/env bash
shift
exec zsh -f -c "$*"
MOCK
chmod +x "$fixture/ssh"
if rsync -a -e "$fixture/ssh" "$fixture/source" "fixture-host:$fixture/destination's space/" > "$fixture/raw.log" 2>&1; then
  echo 'raw rsync remote path accepted'
else
  echo 'raw rsync remote path refused'
fi
awk '/^ship_remote_destination\(\)/ || /^ship_remote_rsync\(\)/ {copy=1} copy {print} copy && /^}/ {copy=0}' \
  "$repo_root/scripts/harness/lan/ship.sh" > "$fixture/upload-helpers.sh"
source "$fixture/upload-helpers.sh"
AURA_E2E_REMOTE=fixture-host
selected_rsync=$(command -v rsync)
rsync() { command "${FIXTURE_RSYNC_BIN:-$selected_rsync}" "$@"; }
ship_remote_rsync -a -e "$fixture/ssh" "$fixture/source" "$(ship_remote_destination "$fixture/destination's space/")" > "$fixture/quoted.log" 2>&1 || { cat "$fixture/quoted.log" >&2; exit 1; }
cmp "$fixture/source" "$fixture/destination's space/source"
echo 'quoted rsync remote path preserves exact bytes'
