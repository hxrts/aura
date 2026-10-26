#!/usr/bin/env bash
# Actual wrapper contracts with fake host commands; never builds or opens accounts.
set -euo pipefail
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-scratch-contract.XXXXXXXX")
trap 'rm -rf -- "$fixture"' EXIT
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
mkdir -p "$fixture/repo/scripts/"{check,ci,dev} "$fixture/bin" "$fixture/quoted ' temps"
cp "$root/scripts/check/protocol-compat.sh" "$fixture/repo/scripts/check/"
cp "$root/scripts/ci/preflight.sh" "$fixture/repo/scripts/ci/"
cp "$root/scripts/dev/quickstart.sh" "$fixture/repo/scripts/dev/"
printf 'protocol Fixture {}\n' > "$fixture/repo/changed.tell"
cat > "$fixture/bin/mktemp" <<'MOCK'
#!/usr/bin/env bash
[[ ${REFUSE_TEMP:-0} != 1 ]] || exit 41
if [[ $# == 0 ]]; then set -- "$TMPDIR/scratch.XXXXXXXX"; fi
owned=$(/usr/bin/mktemp "$@") || exit
printf '%s\n' "$owned" >> "$OWNED_PATHS"
printf '%s\n' "$owned"
MOCK
cat > "$fixture/bin/git" <<'MOCK'
#!/usr/bin/env bash
case "$1" in
 rev-parse|cat-file) exit 0;;
 diff) printf 'changed.tell\n';;
 show) [[ ${REFUSE_SHOW:-0} != 1 ]] || exit 37; printf 'protocol Fixture {}\n';;
 *) exit 89;;
esac
MOCK
cat > "$fixture/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CARGO_ARGS"
case "$*" in
 *'-- init '*) [[ ${REFUSE_CARGO:-0} != 1 ]] || exit 37
   [[ ${MISSING_METADATA:-0} == 1 ]] || printf 'Created effect API metadata\nCreated device_1.toml\n';;
 *'-- status '*) printf 'Configuration loaded successfully\n';;
 *'-- threshold '*|test\ -q\ -p\ aura-testkit\ protocol_compat_pair_from_env*) exit 0;;
 *) exit 89;;
esac
MOCK
cat > "$fixture/bin/df" <<'MOCK'
#!/usr/bin/env bash
printf 'Filesystem 1024-blocks Used Available Capacity Mounted\nfixture 200000000 1 199999999 1%% /\n'
MOCK
# Preflight only checks command availability for these; no real tool invocation.
for name in nix node npm lake quint markdown-link-check rustc just; do
    printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/$name"
done
chmod +x "$fixture/bin/"*
export PATH="$fixture/bin:$PATH" TMPDIR="$fixture/quoted ' temps" OWNED_PATHS="$fixture/paths" CARGO_ARGS="$fixture/cargo.args"
: > "$OWNED_PATHS"; : > "$CARGO_ARGS"
run() { local expected=$1; shift; local actual=0; "$@" > "$fixture/output" 2>&1 || actual=$?; [[ "$actual" == "$expected" ]]; }
cleaned() { while IFS= read -r owned; do [[ ! -e "$owned" ]]; done < "$OWNED_PATHS"; }
# The old predictable PID path belongs to someone else. Never truncate/remove it.
run 0 bash -c 'printf "unrelated\n" > "$TMPDIR/ci-preflight.$$"; printf "%s\n" "$TMPDIR/ci-preflight.$$" > "$1"; exec bash "$2"' bash "$fixture/unrelated-path" "$fixture/repo/scripts/ci/preflight.sh"
[[ $(cat "$(cat "$fixture/unrelated-path")") == unrelated ]]
cleaned
export REFUSE_TEMP=1
run 1 bash "$fixture/repo/scripts/ci/preflight.sh"
unset REFUSE_TEMP
for refused in 0 1; do
    export REFUSE_SHOW=$refused
    expected=0; [[ "$refused" == 0 ]] || expected=37
    run "$expected" bash "$fixture/repo/scripts/check/protocol-compat.sh"
    cleaned
done
unset REFUSE_SHOW
for mode in success native-refusal metadata-refusal; do
    export REFUSE_CARGO=0 MISSING_METADATA=0
    expected=0
    case "$mode" in native-refusal) export REFUSE_CARGO=1; expected=37;; metadata-refusal) export MISSING_METADATA=1; expected=1;; esac
    run "$expected" bash "$fixture/repo/scripts/dev/quickstart.sh" smoke
    cleaned
done
printf 'Scratch wrapper contracts PASS (fake commands, no Cargo/account operation)\n'
