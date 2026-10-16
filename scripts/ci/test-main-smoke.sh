#!/usr/bin/env bash
# Isolated fixed-source smoke receipt and resume contract; no workspace build.
set -euo pipefail
source_root="$(cd "$(dirname "$0")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-main-smoke.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
project="$fixture/project"
mkdir -p "$project/scripts/ci" "$project/scripts/dev" "$fixture/bin"
cp "$source_root/scripts/ci/main-smoke.sh" "$project/scripts/ci/"
cp "$source_root/scripts/dev/build-receipt.sh" "$project/scripts/dev/"
cp "$source_root/scripts/dev/policy-dependency-inputs.sh" "$project/scripts/dev/"
printf 'pinned\n' > "$project/flake.lock"
printf '.tmp/\nartifacts/\n' > "$project/.gitignore"
cat > "$fixture/bin/nix" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == develop && "$2" == -c ]] || exit 2
if [[ -n "${FIXTURE_NIX_CALLS:-}" ]]; then
  count=$(( $(cat "$FIXTURE_NIX_CALLS") + 1 ))
  printf '%s\n' "$count" > "$FIXTURE_NIX_CALLS"
  if [[ "$count" == "${FIXTURE_MUTATE_SOURCE_ON_NIX_CALL:-0}" ]]; then
    printf 'changed during verifier\n' >> "$FIXTURE_PROJECT/flake.lock"
  fi
fi
shift 2
exec "$@"
MOCK
cat > "$project/scripts/dev/build-budget.sh" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1" == --lane && "$2" == handoff-smoke && "$3" == -- && "$4" == bash && "$5" == -c ]] || exit 2
command_text=$6
index=$(( $(wc -l < "$FIXTURE_CALLS") + 1 ))
printf '%s\n' "$command_text" >> "$FIXTURE_CALLS"
if [[ "$command_text" == 'just ci-clippy' && "${FIXTURE_MUTATE_TOOL:-0}" == 1 ]]; then
  printf '#!/usr/bin/env bash\nexit 9\n' > "${FIXTURE_BIN}/just"
fi
if [[ "$command_text" == cargo\ test\ -p\ hxrts-aura-agent* && "${FIXTURE_FAIL_STEP:-0}" == 5 ]]; then
  echo 'injected typed fifth-step failure' >&2
  exit 47
fi
if [[ "${FIXTURE_REFUSE_STEP:-0}" == "$index" ]]; then
  echo 'injected admission refusal' >&2
  exit 75
fi
printf -v run_suffix '%06d' "$index"
dir="$PWD/artifacts/disk-budget/receipts/run.$run_suffix"
mkdir -p "$dir"
chmod 700 "$dir"
revision="$(git rev-parse HEAD)"
printf 'fixture command output\n' > "$dir/command.000001.log"
chmod 600 "$dir/command.000001.log"
jq -n --arg root "$PWD" --arg revision "$revision" --arg command "$command_text" \
  --arg command_log "$dir/command.000001.log" \
  '{root:$root,lane:"handoff-smoke",source:{revision:$revision},
    admission:{admitted:true},final_exit_status:0,command_argv:["bash","-c",$command],
    artifacts:{command_log:$command_log}}' \
  > "$dir/receipt.json"
chmod 600 "$dir/receipt.json"
echo "Build receipt: $dir/receipt.json"
MOCK
for executable in just cargo rustc node npm; do
  printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/$executable"
done
chmod +x "$fixture/bin/"* "$project/scripts/dev/build-budget.sh"
git -C "$project" init -q
git -C "$project" add .
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
  git -C "$project" commit -qm fixture
commit="$(git -C "$project" rev-parse HEAD)"
export PATH="$fixture/bin:$PATH" FIXTURE_CALLS="$fixture/calls" FIXTURE_BIN="$fixture/bin"
export FIXTURE_PROJECT="$project"
: > "$FIXTURE_CALLS"
run() { (cd "$project" && bash scripts/ci/main-smoke.sh "$@"); }
if FIXTURE_FAIL_STEP=5 run > "$fixture/out" 2>&1; then
  echo 'failed fifth step accepted' >&2; exit 1
fi
dir="$project/.tmp/main-smoke-$commit"
[[ ! -e "$dir/validated-commit" && $(wc -l < "$FIXTURE_CALLS") -eq 5 ]] || {
  cat "$fixture/out" >&2; exit 1;
}
for index in 1 2 3 4; do [[ -f "$dir/step-$index.json" ]]; done
[[ ! -e "$dir/step-5.json" && ! -e "$dir/keyring-5" ]]
if run --verify > "$fixture/out" 2>&1; then
  echo 'incomplete smoke accepted' >&2; exit 1
fi
if ! run > "$fixture/out" 2>&1; then cat "$fixture/out" >&2; exit 1; fi
[[ $(wc -l < "$FIXTURE_CALLS") -eq 7 ]]
run --verify > "$fixture/out"
[[ "$(cat "$dir/validated-commit")" == "$commit" ]]
[[ ! -e "$dir/keyring-6" ]]
[[ -n "$(find "$dir" -maxdepth 1 -type f -name 'step-5.attempt-*.json' -print -quit)" ]]

printf '0\n' > "$fixture/nix-calls"
if FIXTURE_NIX_CALLS="$fixture/nix-calls" FIXTURE_MUTATE_SOURCE_ON_NIX_CALL=2 \
    run --verify > "$fixture/out" 2>&1; then
  echo 'source change during smoke verification accepted' >&2; exit 1
fi
git -C "$project" checkout -q -- flake.lock
run --verify > "$fixture/out"

# A refused attempt retains earlier exact success but cannot publish a marker.
if FIXTURE_REFUSE_STEP=8 RUST_TEST_THREADS=2 run > "$fixture/out" 2>&1; then
  echo 'admission refusal accepted' >&2; exit 1
fi
[[ ! -e "$dir/validated-commit" &&
   -n "$(find "$dir" -maxdepth 1 -type f -name 'step-1.attempt-*.json' -print -quit)" ]]
run > "$fixture/out"
run --verify > "$fixture/out"

# Same commit, different execution selector invalidates every successful step.
if RUST_TEST_THREADS=2 run --verify > "$fixture/out" 2>&1; then
  echo 'changed test scheduling accepted' >&2; exit 1
fi
if CARGO_PROFILE_DEV_DEBUG=0 run --verify > "$fixture/out" 2>&1; then
  echo 'changed Cargo profile accepted' >&2; exit 1
fi
if RUSTC_WRAPPER="$fixture/bin/just" run --verify > "$fixture/out" 2>&1; then
  echo 'changed compiler wrapper accepted' >&2; exit 1
fi
# Same selected executable path but changed bytes invalidates evidence.
printf '#!/usr/bin/env bash\nexit 9\n' > "$fixture/bin/just"
if run --verify > "$fixture/out" 2>&1; then
  echo 'changed tool bytes accepted' >&2; exit 1
fi
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/just"
run --verify > "$fixture/out"

# A tool changed by an owned step must fail before its success receipt exists.
if FIXTURE_MUTATE_TOOL=1 RUST_TEST_THREADS=3 run > "$fixture/out" 2>&1; then
  echo 'mid-step tool mutation accepted' >&2; exit 1
fi
[[ ! -e "$dir/validated-commit" && ! -e "$dir/step-1.json" ]]
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/just"
run > "$fixture/out"
run --verify > "$fixture/out"

step2log="$(jq -r .log "$dir/step-2.json")"
cp "$step2log" "$fixture/original-log"
printf 'forged output\n' >> "$step2log"
if run --verify > "$fixture/out" 2>&1; then
  echo 'corrupted step log accepted' >&2; exit 1
fi
cp "$fixture/original-log" "$step2log"
run --verify > "$fixture/out"
cp "$dir/step-3.json" "$fixture/original-receipt"
jq '.command="just different-command"' "$dir/step-3.json" > "$fixture/tampered"
cat "$fixture/tampered" > "$dir/step-3.json"
if run --verify > "$fixture/out" 2>&1; then
  echo 'changed step command accepted' >&2; exit 1
fi
cp "$fixture/original-receipt" "$dir/step-3.json"
run --verify > "$fixture/out"
budget_receipt="$(jq -r .budget_receipt "$dir/step-3.json")"
cp "$budget_receipt" "$fixture/original-budget-receipt"
jq '.final_exit_status=1' "$budget_receipt" > "$fixture/tampered"
cat "$fixture/tampered" > "$budget_receipt"
if run --verify > "$fixture/out" 2>&1; then
  echo 'corrupt guarded receipt accepted' >&2; exit 1
fi
cp "$fixture/original-budget-receipt" "$budget_receipt"
run --verify > "$fixture/out"
command_log="$(jq -r .command_log "$dir/step-3.json")"
printf 'tampered command output\n' >> "$command_log"
if run --verify > "$fixture/out" 2>&1; then
  echo 'corrupt guarded output accepted' >&2; exit 1
fi
printf 'fixture command output\n' > "$command_log"
run --verify > "$fixture/out"
printf 'foreign\n' > "$dir/validated-commit"
if run --verify > "$fixture/out" 2>&1; then
  echo 'corrupt success marker accepted' >&2; exit 1
fi
printf '%s\n' "$commit" > "$dir/validated-commit"
run --verify > "$fixture/out"

# Never follow an existing evidence symlink before mkdir, truncate or remove.
mv "$dir" "$fixture/preserved-evidence"
ln -s "$fixture/preserved-evidence" "$dir"
if run > "$fixture/out" 2>&1; then
  echo 'symlinked smoke evidence accepted' >&2; exit 1
fi
[[ "$(cat "$fixture/preserved-evidence/validated-commit")" == "$commit" ]]
rm "$dir"
mv "$fixture/preserved-evidence" "$dir"
mv "$dir/step-1.json" "$fixture/preserved-step-1.json"
ln -s "$fixture/outside" "$dir/step-1.json"
if run > "$fixture/out" 2>&1; then
  echo 'symlinked step receipt accepted' >&2; exit 1
fi
[[ ! -e "$fixture/outside" ]]
rm "$dir/step-1.json"
mv "$fixture/preserved-step-1.json" "$dir/step-1.json"
run > "$fixture/out"
run --verify > "$fixture/out"
echo 'main smoke fixture passed'
