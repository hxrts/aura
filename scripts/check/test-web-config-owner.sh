#!/usr/bin/env bash
# Real wrapper transaction, fake host commands; no Dioxus/Cargo or server startup.
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-web-config-contract.XXXXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
cat > "$fixture/bin/mktemp" <<'MOCK'
#!/usr/bin/env bash
[[ ${REFUSE_ALLOC:-0} != 1 ]] || exit 41
# Keep even the baseline's argument-free allocation inside this fixture owner.
[[ $# != 0 ]] || set -- "$TMPDIR/owner.XXXXXXXX"
actual=$(/usr/bin/mktemp "$@") || exit
printf '%s\n' "$actual" >> "$OWNER_PATHS"
printf '%s\n' "$actual"
MOCK
cat > "$fixture/bin/cp" <<'MOCK'
#!/usr/bin/env bash
last="${!#}"
source="${1:-}"
[[ "$source" != -p ]] || source="${2:-}"
if [[ "$source" == "$LIVE_CONFIG" && "$last" != "$LIVE_CONFIG" && ${REFUSE_BACKUP:-0} == 1 ]]; then
    printf 'partial backup\n' > "$last"
    exit 43
fi
case "$last" in
 */.Dioxus.toml.restore.*|*/Dioxus.toml) [[ ${REFUSE_RESTORE:-0} != 1 ]] || exit 73;;
esac
exec /bin/cp "$@"
MOCK
cat > "$fixture/bin/perl" <<'MOCK'
#!/usr/bin/env bash
count=0; [[ ! -f "$PERL_COUNT" ]] || read -r count < "$PERL_COUNT"
count=$((count+1)); printf '%s\n' "$count" > "$PERL_COUNT"
last="${!#}"
/usr/bin/perl "$@" || exit
if [[ ${REFUSE_EDIT:-0} == "$count" ]]; then
    printf '# actual partial edit before refusal\n' >> "$last"
    exit 47
fi
if [[ ${CANCEL_EDIT:-0} == "$count" ]]; then kill -TERM "$PPID"; fi
MOCK
cat > "$fixture/bin/mv" <<'MOCK'
#!/usr/bin/env bash
case "$*" in *'.Dioxus.toml.restore.'*) [[ ${REFUSE_PUBLISH:-0} != 1 ]] || exit 79;; esac
exec /bin/mv "$@"
MOCK
cat > "$fixture/bin/rm" <<'MOCK'
#!/usr/bin/env bash
case "$*" in *'/aura-web-config.'*) [[ ${REFUSE_CLEANUP:-0} != 1 ]] || exit 81;; esac
exec /bin/rm "$@"
MOCK
cat > "$fixture/bin/npm" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$HOST_CALLS"
exit 0
MOCK
cat > "$fixture/bin/node" <<'MOCK'
#!/usr/bin/env bash
cmp "$EXPECTED_CONFIG" "$LIVE_CONFIG" || exit 91
printf 'node-after-restoration\n' >> "$HOST_CALLS"
MOCK
chmod +x "$fixture/bin/"*
run_case() {
    local name=$1 expected=$2; shift 2
    local case_root="$fixture/$name with ' quote" actual=0
    mkdir -p "$case_root/repo/scripts/web" "$case_root/repo/crates/aura-web/node_modules/ws" "$case_root/tmp"
    /bin/cp "$root/scripts/web/"{prebuilt-provenance,prebuilt-transfer,log-bootstrap}.sh "$case_root/repo/scripts/web/"
    /bin/cp "${AURA_WEB_CONFIG_WRAPPER_UNDER_TEST:-$root/scripts/web/serve-static.sh}" "$case_root/repo/scripts/web/serve-static.sh"
    cat > "$case_root/repo/scripts/web/dx.sh" <<'MOCK'
#!/usr/bin/env bash
# Actual build boundary must see both temporary edits before injected refusal.
grep -q '^hot_reload = false$' "$LIVE_CONFIG" || exit 92
grep -q '^reload_html = false$' "$LIVE_CONFIG" || exit 93
printf 'dx-original-boundary\n' >> "$HOST_CALLS"
if [[ ${REFUSE_BUILD:-0} == 1 ]]; then echo 'actual fixture build refusal 37' >&2; exit 37; fi
mkdir -p "$PUBLIC_OUTPUT"
printf '<html>fixture</html>\n' > "$PUBLIC_OUTPUT/index.html"
MOCK
    chmod +x "$case_root/repo/scripts/web/dx.sh"
    printf '[web]\nhot_reload = true\nreload_html = true\n# same original bytes\n' > "$case_root/expected"
    chmod 640 "$case_root/expected"
    /bin/cp -p "$case_root/expected" "$case_root/repo/crates/aura-web/Dioxus.toml"
    case "$name" in
        symlink)
            /bin/rm "$case_root/repo/crates/aura-web/Dioxus.toml"
            ln -s "$case_root/expected" "$case_root/repo/crates/aura-web/Dioxus.toml";;
        nonregular)
            /bin/rm "$case_root/repo/crates/aura-web/Dioxus.toml"
            mkdir "$case_root/repo/crates/aura-web/Dioxus.toml";;
    esac
    : > "$case_root/paths"; : > "$case_root/calls"
    env PATH="$fixture/bin:$PATH" TMPDIR="$case_root/tmp" OWNER_PATHS="$case_root/paths" \
        PERL_COUNT="$case_root/perl-count" HOST_CALLS="$case_root/calls" \
        LIVE_CONFIG="$case_root/repo/crates/aura-web/Dioxus.toml" EXPECTED_CONFIG="$case_root/expected" \
        PUBLIC_OUTPUT="$case_root/repo/target/dx/aura-web/debug/web/public" \
        AURA_HARNESS_WEB_BUILD_PROFILE=debug AURA_HARNESS_WEB_PREBUILT_ONLY=0 \
        "$@" bash "$case_root/repo/scripts/web/serve-static.sh" 4317 > "$case_root/output" 2>&1 || actual=$?
    if [[ $actual != "$expected" ]]; then
        cat "$case_root/output" >&2
        echo "case $name: expected $expected, actual $actual" >&2
        exit 1
    fi
    if [[ $name == restore-copy || $name == restore-publish || $name == dual-refusal ]]; then
        local backup
        backup=$(find "$case_root/tmp" -name original -type f -print)
        [[ -n "$backup" ]]
        cmp "$case_root/expected" "$backup"
        ! cmp -s "$case_root/expected" "$case_root/repo/crates/aura-web/Dioxus.toml"
        grep -Fq "$backup" "$case_root/output"
        [[ -z $(find "$case_root/repo/crates/aura-web" -name '.Dioxus.toml.restore.*' -print) ]]
        # Explicit recovery uses the SAME retained complete backup, not a copy
        # of the already mutated config. No server was admitted on refusal.
        /bin/cp -p "$backup" "$case_root/repo/crates/aura-web/Dioxus.toml"
        cmp "$case_root/expected" "$case_root/repo/crates/aura-web/Dioxus.toml"
    elif [[ $name == nonregular ]]; then
        [[ -d "$case_root/repo/crates/aura-web/Dioxus.toml" ]]
    else
        cmp "$case_root/expected" "$case_root/repo/crates/aura-web/Dioxus.toml"
    fi
    if [[ $name == symlink ]]; then
        [[ -L "$case_root/repo/crates/aura-web/Dioxus.toml" ]]
        [[ $(readlink "$case_root/repo/crates/aura-web/Dioxus.toml") == "$case_root/expected" ]]
    elif [[ $name != nonregular ]]; then
        local permissions
        permissions=$(LC_ALL=C ls -ld "$case_root/repo/crates/aura-web/Dioxus.toml")
        [[ ${permissions:0:10} == '-rw-r-----' ]]
    fi
    if [[ $name == cleanup || $name == dual-cleanup ]]; then
        [[ -n $(find "$case_root/tmp" -name original -type f -print) ]]
    elif [[ $name != restore-copy && $name != restore-publish && $name != dual-refusal ]]; then
        while IFS= read -r owned; do
            if [[ -e "$owned" ]]; then
                echo "case $name: original owned scratch remains at $owned" >&2
                exit 1
            fi
        done < "$case_root/paths"
    fi
    if [[ $name == success ]]; then
        grep -q '^node-after-restoration$' "$case_root/calls"
    else
        ! grep -q '^node-after-restoration$' "$case_root/calls"
    fi
    case "$name" in
        dual-refusal) grep -q 'original status=37; config cleanup status=73' "$case_root/output";;
        dual-cleanup) grep -q 'original status=37; config cleanup status=81' "$case_root/output";;
    esac
    printf 'PASS %s (exit %s)\n' "$name" "$actual"
}
run_selected() {
    [[ -z ${AURA_WEB_CONFIG_CASE:-} || "$AURA_WEB_CONFIG_CASE" == "$1" ]] || return 0
    run_case "$@"
}
run_selected success 0
run_selected symlink 2
run_selected nonregular 2
run_selected allocation 41 REFUSE_ALLOC=1
run_selected backup-copy 43 REFUSE_BACKUP=1
run_selected first-edit 47 REFUSE_EDIT=1
run_selected second-edit 47 REFUSE_EDIT=2
run_selected build 37 REFUSE_BUILD=1
run_selected term 143 CANCEL_EDIT=1
run_selected restore-copy 73 REFUSE_RESTORE=1
run_selected restore-publish 79 REFUSE_PUBLISH=1
run_selected cleanup 81 REFUSE_CLEANUP=1
run_selected dual-refusal 37 REFUSE_BUILD=1 REFUSE_RESTORE=1
run_selected dual-cleanup 37 REFUSE_BUILD=1 REFUSE_CLEANUP=1
printf 'Web config owner contracts PASS (no real build or server)\n'
