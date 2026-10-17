#!/usr/bin/env bash
set -euo pipefail
prepared=$(cd "$(dirname "$0")" && pwd -P)
repo_root=$(git -C "$prepared" rev-parse --show-toplevel)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/repo/scripts/web" "$fixture/repo/crates/aura-web" "$fixture/bin"
: > "$fixture/repo/Cargo.toml"
: > "$fixture/repo/crates/aura-web/Cargo.toml"
# Exercise the actual prepared metadata preamble without installing/building tools.
awk '/^install_cached_cargo_tool\(\)/ {exit} {print}' "$repo_root/scripts/web/dx.sh" > "$fixture/repo/scripts/web/probe.sh"
cat > "$fixture/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$CALLS"
[[ "$*" == 'metadata --manifest-path '* ]] || exit 90
case "$MODE" in
  valid) printf '%s\n' '{"packages":[{"name":"dioxus","version":"0.7.0"},{"name":"wasm-bindgen","version":"0.2.100"}]}' ;;
  fail) exit 72 ;;
  malformed) printf 'invalid-json\n' ;;
  ambiguous) printf '%s\n' '{"packages":[{"name":"dioxus","version":"0.7.0"},{"name":"dioxus","version":"0.6.0"},{"name":"wasm-bindgen","version":"0.2.100"}]}' ;;
esac
MOCK
chmod +x "$fixture/bin/cargo"
export PATH="$fixture/bin:$PATH" AURA_WORKSPACE_ROOT="$fixture/repo" CALLS="$fixture/calls"
export MODE=valid
bash "$fixture/repo/scripts/web/probe.sh" --selected-tool-inventory
[[ $(wc -l < "$CALLS") -eq 1 ]]
rg -q -- '--locked --offline$' "$CALLS"
for MODE in fail malformed ambiguous; do
  export MODE
  : > "$CALLS"
  if bash "$fixture/repo/scripts/web/probe.sh" --selected-tool-inventory > "$fixture/output" 2>&1; then
    echo "metadata probe accepted $MODE" >&2; exit 1
  fi
  [[ $(wc -l < "$CALLS") -eq 1 ]]
done
export MODE=valid
: > "$CALLS"
bash "$fixture/repo/scripts/web/probe.sh" build
[[ $(wc -l < "$CALLS") -eq 1 ]]
! rg -q -- '--locked|--offline' "$CALLS"
echo 'prepared metadata preamble: single read, read-only admission, failures refused'
