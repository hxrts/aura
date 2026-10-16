#!/usr/bin/env bash
# Exact dx-owner refusal when the pinned Nix Binaryen output is unavailable.
set -euo pipefail
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-binaryen-select.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/project/crates/aura-web" "$fixture/bin" \
  "$fixture/cache/binaryen/version_123/bin"
: > "$fixture/project/Cargo.toml"
: > "$fixture/project/crates/aura-web/Cargo.toml"
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/cache/binaryen/version_123/bin/wasm-opt"
cat > "$fixture/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
printf '%s\n' '{"packages":[{"name":"dioxus","version":"0.6.3"},{"name":"wasm-bindgen","version":"0.2.100"}]}'
CARGO
cat > "$fixture/bin/nix" <<'NIX'
#!/usr/bin/env bash
if [[ "$*" == *builtins.currentSystem* ]]; then
  printf 'aarch64-darwin'
else
  printf '%s' "$FIXTURE_EXPECTED_BINARYEN"
fi
NIX
cat > "$fixture/bin/curl" <<'CURL'
#!/usr/bin/env bash
printf 'unexpected mutable download\n' >> "$FIXTURE_CURL_CALLS"
exit 97
CURL
chmod +x "$fixture/bin/"* "$fixture/cache/binaryen/version_123/bin/wasm-opt"
export PATH="$fixture/bin:$PATH" AURA_WORKSPACE_ROOT="$fixture/project" \
  AURA_WEB_TOOLS_CACHE_ROOT="$fixture/cache" FIXTURE_CURL_CALLS="$fixture/curl-calls"
export FIXTURE_EXPECTED_BINARYEN=/nix/store/11111111111111111111111111111111-binaryen-version_123
inventory="$fixture/inventory"
: > "$inventory"
chmod 600 "$inventory"
if env -u AURA_NIX_BINARYEN_ROOT bash "$repo_root/scripts/web/dx.sh" \
    --selected-tool-inventory "$inventory" > "$fixture/output" 2>&1; then
  echo 'mutable cache substituted for missing Nix Binaryen' >&2; exit 1
fi
rg -q 'pinned Nix Binaryen version_123 output unavailable' "$fixture/output"
[[ ! -e "$FIXTURE_CURL_CALLS" ]]
if AURA_NIX_BINARYEN_ROOT="$fixture/cache/binaryen/version_123" \
    bash "$repo_root/scripts/web/dx.sh" --selected-tool-inventory "$inventory" \
    > "$fixture/output" 2>&1; then
  echo 'mutable Binaryen root accepted' >&2; exit 1
fi
rg -q 'Binaryen selection is not a version_123 Nix output' "$fixture/output"
foreign=/nix/store/22222222222222222222222222222222-binaryen-version_123
if AURA_NIX_BINARYEN_ROOT="$foreign" \
    bash "$repo_root/scripts/web/dx.sh" --selected-tool-inventory "$inventory" \
    > "$fixture/output" 2>&1; then
  echo 'foreign same-version store output accepted' >&2; exit 1
fi
rg -q 'foreign Binaryen output differs from pinned derivation' "$fixture/output"
[[ ! -e "$FIXTURE_CURL_CALLS" && ! -s "$inventory" ]]
echo 'Binaryen selection: missing, mutable and foreign outputs refused without download'
