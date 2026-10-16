#!/usr/bin/env bash
# Isolated assertion contract for the future rehydration caller.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-lane-idle.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
holder=''
cleanup() {
  [[ -z "$holder" ]] || { kill "$holder" 2>/dev/null || true; wait "$holder" 2>/dev/null || true; }
  rm -rf "$fixture"
}
trap cleanup EXIT
project="$fixture/project"
mkdir -p "$project/target/debug" "$fixture/bin"
printf '[package]\nname="lane-idle-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf 'artifact\n' > "$project/target/debug/artifact"
cat > "$fixture/bin/ps" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${AURA_FIXTURE_BUILDER:-}" ]]; then
  printf '123 cargo %s/target/debug/artifact\n' "$AURA_FIXTURE_PROJECT"
fi
EOF
cat > "$fixture/bin/lsof" <<'EOF'
#!/usr/bin/env bash
if [[ -n "${AURA_FIXTURE_OPEN:-}" ]]; then
  printf 'cargo 123 user txt REG 1 1 %s/target/debug/artifact\n' "$AURA_FIXTURE_PROJECT"
fi
EOF
chmod +x "$fixture/bin/ps" "$fixture/bin/lsof"
export PATH="$fixture/bin:$PATH" AURA_FIXTURE_PROJECT="$project"
guard() { bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$project" --lane debug --assert-idle -- "$@"; }
refuse() { if guard true > "$fixture/output" 2>&1; then cat "$fixture/output" >&2; echo "unsafe idle assertion passed: builder=${AURA_FIXTURE_BUILDER:-0} open=${AURA_FIXTURE_OPEN:-0}" >&2; exit 1; fi; }
guard true > "$fixture/output"
if guard bash -c 'exit 17' > "$fixture/output" 2>&1; then
  echo 'guarded command failure lost' >&2; exit 1
fi
[[ -f "$project/target/debug/artifact" ]]
export AURA_FIXTURE_BUILDER=1
refuse
unset AURA_FIXTURE_BUILDER
export AURA_FIXTURE_OPEN=1
refuse
unset AURA_FIXTURE_OPEN
mv "$project/target/debug" "$project/target/held"
ln -s held "$project/target/debug"
refuse
rm "$project/target/debug"
guard true > "$fixture/output"
[[ ! -e "$project/target/debug" ]]
# The assertion owner retains the checkout lock until the child has finished.
guard bash -c 'touch "$1/entered"; while [[ ! -e "$1/release" ]]; do sleep 0.05; done' _ "$fixture" > "$fixture/holder-output" &
holder=$!
for (( attempt=0; attempt<100; attempt++ )); do
  [[ -e "$fixture/entered" ]] && break
  sleep 0.05
done
[[ -e "$fixture/entered" ]] || { echo 'guarded command did not start' >&2; exit 1; }
refuse
touch "$fixture/release"
wait "$holder"
holder=''
guard true > "$fixture/output"
mv "$project/target/held" "$project/target/debug"
bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$project" --lane debug --dry-run > "$fixture/output"
[[ -f "$project/target/debug/artifact" ]]
bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$project" --lane debug --apply > "$fixture/output"
[[ ! -e "$project/target/debug" ]]
echo 'lane idle guard: existing/absent, active builder, open file, symlink and held-lock race passed'
