#!/usr/bin/env bash
set -euo pipefail
source "$(cd "$(dirname "$0")" && pwd)/prebuilt-provenance.sh"
fixture=$(mktemp -d /private/tmp/aura-web-prebuilt.XXXXXX)
fixture=$(cd "$fixture" && pwd -P)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/target/public/assets"
mkdir -p "$fixture/crates/aura-web"
mkdir -p "$fixture/scripts/web"
mkdir -p "$fixture/scripts/dev"
cp "$(cd "$(dirname "$0")/../.." && pwd)/scripts/dev/policy-dependency-inputs.sh" "$fixture/scripts/dev/"
mkdir -p "$fixture/bin"
mkdir -p "$fixture/binaryen/lib"
printf 'library-v1\n' > "$fixture/binaryen/lib/libbinaryen.dylib"
for selected in node npm cargo rustc dx wasm-bindgen wasm-opt; do
    printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/$selected"
    chmod +x "$fixture/bin/$selected"
done
cat > "$fixture/scripts/web/dx.sh" <<'MOCK'
#!/usr/bin/env bash
[[ "$1" == --selected-tool-inventory ]] || exit 2
cp "$FIXTURE_WEB_INVENTORY" "$2"
if [[ "${FIXTURE_ALT_SELECTED:-0}" == 1 ]]; then
    sed -i.bak 's|/bin/dx|/bin/foreign-dx|' "$2"
    rm "$2.bak"
fi
MOCK
chmod +x "$fixture/scripts/web/dx.sh"
export PATH="$fixture/bin:$PATH"
export CARGO_HOME="$fixture/cargo-home"
mkdir -p "$CARGO_HOME"
inventory="$fixture/tool-inventory"
export FIXTURE_WEB_INVENTORY="$inventory"
: > "$inventory"
chmod 600 "$inventory"
for selected in node npm cargo rustc dx wasm-bindgen wasm-opt; do
    aura_web_record_selected_tool "$inventory" "$selected" "$fixture/bin/$selected"
done
aura_web_record_binaryen_libraries "$inventory" "$fixture/binaryen/lib"
printf 'target/\nartifacts/\nbin/\nbinaryen/\ncargo-home/\ntool-inventory\n' > "$fixture/.gitignore"
printf 'source=v1\n' > "$fixture/source.rs"
printf 'pinned-toolchain=v1\n' > "$fixture/flake.lock"
git -C "$fixture" init -q
git -C "$fixture" add .gitignore source.rs flake.lock scripts/web/dx.sh scripts/dev/policy-dependency-inputs.sh
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
    git -C "$fixture" commit -qm fixture
public_dir="$fixture/target/public"
printf '<html>fixture</html>\n' > "$public_dir/index.html"
printf 'window.__AURA_HARNESS__ = {};\n' > "$public_dir/assets/aura-web.js"
printf 'wasm-fixture\n' > "$public_dir/assets/aura-web.wasm"

start_commit=$(aura_web_clean_commit "$fixture")
start_tree=$(aura_web_clean_tree "$fixture")
start_config=$(aura_web_build_config_digest "$fixture")
aura_web_write_prebuilt_provenance "$fixture" "$public_dir" "$inventory" "$start_commit" "$start_tree" "$start_config"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"
CARGO_INCREMENTAL=0 aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"
AURA_HARNESS_WEB_PREBUILT_ONLY=1 AURA_HARNESS_WEB_BUILD_PROFILE=release \
    AURA_HARNESS_BROWSER_BROKER_AUTH=private-fixture-bearer RUST_LOG=debug \
    aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"
if CARGO_INCREMENTAL=1 aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed incremental policy' >&2; exit 1
fi
if RUSTFLAGS=-Cdebuginfo=0 aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed Rust flags' >&2; exit 1
fi
if CARGO_PROFILE_RELEASE_DEBUG=2 aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed Cargo profile' >&2; exit 1
fi
if RUSTC_WRAPPER="$fixture/bin/dx" aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed compiler wrapper' >&2; exit 1
fi
printf '[build]\nrustflags = ["-Cdebuginfo=0"]\n' > "$CARGO_HOME/config.toml"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed Cargo home config' >&2; exit 1
fi
rm "$CARGO_HOME/config.toml"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"

printf 'library-v2\n' > "$fixture/binaryen/lib/libbinaryen.dylib"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed Binaryen library bytes' >&2; exit 1
fi
printf 'library-v1\n' > "$fixture/binaryen/lib/libbinaryen.dylib"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"
if FIXTURE_ALT_SELECTED=1 aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted a changed selected cache path' >&2; exit 1
fi

sed -i.bak 's/web,harness/web/' "$public_dir/.aura-prebuilt-provenance"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted a different feature command' >&2; exit 1
fi
rm "$public_dir/.aura-prebuilt-provenance.bak"
aura_web_write_prebuilt_provenance "$fixture" "$public_dir" "$inventory" "$start_commit" "$start_tree" "$start_config"

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

ln -s "$fixture/source.rs" "$public_dir/assets/linked.js"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted a linked JavaScript artifact' >&2; exit 1
fi
rm "$public_dir/assets/linked.js"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"

printf '#!/usr/bin/env bash\nexit 9\n' > "$fixture/bin/dx"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted changed selected dx bytes' >&2; exit 1
fi
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/bin/dx"
aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"

sed -i.bak '1s/commit=.*/commit=0000000000000000000000000000000000000000/' \
    "$public_dir/.aura-prebuilt-provenance"
rm "$public_dir/.aura-prebuilt-provenance.bak"
if aura_web_prebuilt_provenance_matches "$fixture" "$public_dir"; then
    echo 'prebuilt provenance accepted foreign commit marker' >&2; exit 1
fi
rm "$public_dir/.aura-prebuilt-provenance"
printf 'source=v2\n' > "$fixture/source.rs"
git -C "$fixture" add source.rs
GIT_AUTHOR_NAME=Fixture GIT_AUTHOR_EMAIL=fixture@example.invalid \
GIT_COMMITTER_NAME=Fixture GIT_COMMITTER_EMAIL=fixture@example.invalid \
    git -C "$fixture" commit -qm advanced
aura_web_write_prebuilt_provenance "$fixture" "$public_dir" "$inventory" "$start_commit" "$start_tree" "$start_config"
[[ ! -e "$public_dir/.aura-prebuilt-provenance" ]] || {
    echo 'build source changed from its start revision' >&2; exit 1;
}
echo 'prebuilt provenance fixture passed'
