#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/prebuilt-provenance.sh"
fixture=$(mktemp -d /private/tmp/aura-web-prebuilt.XXXXXX)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/target/public/assets"
printf 'target/\n' > "$fixture/.gitignore"
printf 'source=v1\n' > "$fixture/source.rs"
printf 'pinned-toolchain=v1\n' > "$fixture/flake.lock"
git -C "$fixture" init -q
git -C "$fixture" add .gitignore source.rs flake.lock
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
    git -C "$fixture" commit -qm fixture
public_dir="$fixture/target/public"
printf '<html>fixture</html>\n' > "$public_dir/index.html"
printf 'window.__AURA_HARNESS__ = {};\n' > "$public_dir/assets/aura-web.js"
printf 'wasm-fixture\n' > "$public_dir/assets/aura-web.wasm"

aura_web_write_prebuilt_provenance "$fixture" "$public_dir"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"

sed -i.bak 's/web,harness/web/' "$public_dir/.aura-prebuilt-provenance"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted a different feature command' >&2; exit 1
fi
rm "$public_dir/.aura-prebuilt-provenance.bak"
aura_web_write_prebuilt_provenance "$fixture" "$public_dir"

printf 'pinned-toolchain=v2\n' > "$fixture/flake.lock"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed toolchain pin' >&2; exit 1
fi
git -C "$fixture" checkout -q -- flake.lock

printf 'source=v2\n' > "$fixture/source.rs"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed source' >&2; exit 1
fi
git -C "$fixture" checkout -q -- source.rs

printf 'tampered\n' > "$public_dir/assets/aura-web.js"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed emitted bundle' >&2; exit 1
fi
printf 'window.__AURA_HARNESS__ = {};\n' > "$public_dir/assets/aura-web.js"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"

sed -i.bak '1s/commit=.*/commit=0000000000000000000000000000000000000000/' \
    "$public_dir/.aura-prebuilt-provenance"
rm "$public_dir/.aura-prebuilt-provenance.bak"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted foreign commit marker' >&2; exit 1
fi
echo 'prebuilt provenance fixture passed'
