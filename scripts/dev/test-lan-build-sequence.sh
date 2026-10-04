#!/usr/bin/env bash
# Actual LAN entry point with isolated Git state and a non-building Just stub.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/aura-lan-sequence-test.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT
project="$scratch/project"
mkdir -p "$project/scripts/harness/lan" "$scratch/bin"
cp "$repo_root/scripts/harness/lan/build.sh" "$project/scripts/harness/lan/build.sh"
git -C "$project" init -q
git -C "$project" add .
git -C "$project" -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm fixture
export IN_NIX_SHELL=1 CALLS_FILE="$scratch/calls" FAIL_LANE='' CHANGE_COMMIT=0
export PATH="$scratch/bin:$PATH"
cat > "$scratch/bin/just" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$CALLS_FILE"
[[ "$1" != "$FAIL_LANE" ]] || exit 7
if [[ "$CHANGE_COMMIT" == 1 ]]; then
  git -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm changed
fi
EOF
chmod +x "$scratch/bin/just"
run() { bash "$project/scripts/harness/lan/build.sh" "$@"; }
expect() {
  local wanted="$1" actual
  shift
  if "$@" > "$scratch/output" 2>&1; then actual=0; else actual=$?; fi
  [[ "$actual" == "$wanted" ]] || { cat "$scratch/output" >&2; exit 1; }
}
expect 0 run all --dry-run
[[ ! -e "$CALLS_FILE" ]]
for lane in terminal web harness; do rg -q "Dry run: just e2e-build-$lane" "$scratch/output"; done
expect 0 run all
printf 'e2e-build-terminal\ne2e-build-web\ne2e-build-harness\n' > "$scratch/expected"
cmp "$scratch/expected" "$CALLS_FILE"
: > "$CALLS_FILE"
export FAIL_LANE=e2e-build-web
expect 7 run all
[[ "$(wc -l < "$CALLS_FILE")" == 2 ]]
export FAIL_LANE=''
: > "$CALLS_FILE"
touch "$project/untracked"
expect 1 run all
[[ ! -s "$CALLS_FILE" ]]
rm "$project/untracked"
export CHANGE_COMMIT=1
expect 1 run all
[[ "$(wc -l < "$CALLS_FILE")" == 1 ]]
rg -q 'expected commit' "$scratch/output"
echo 'LAN fixed-commit sequence safety tests passed'
