#!/usr/bin/env bash
# Validate CI runner prerequisites: required commands and resource limits.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

fail() {
  echo "ci-preflight: $*" >&2
  exit 1
}

require_cmd() {
  local cmd="$1"
  command -v "$cmd" >/dev/null 2>&1 || fail "required command missing from PATH: $cmd"
}

format_gib() {
  local kib="$1"
  awk -v kib="$kib" 'BEGIN { printf "%.1f GiB", kib / 1024 / 1024 }'
}

min_free_gib="${AURA_CI_PREFLIGHT_MIN_FREE_GIB:-5}"
if ! [[ "$min_free_gib" =~ ^[0-9]+$ ]]; then
  fail "AURA_CI_PREFLIGHT_MIN_FREE_GIB must be an integer, got: $min_free_gib"
fi

avail_kib="$(df -Pk "$repo_root" | awk 'NR==2 { print $4 }')"
min_kib="$((min_free_gib * 1024 * 1024))"
if [[ -z "$avail_kib" || "$avail_kib" -lt "$min_kib" ]]; then
  fail "need at least ${min_free_gib} GiB free in $(df -Pk "$repo_root" | awk 'NR==2 { print $1 }'); found $(format_gib "${avail_kib:-0}")"
fi

tmp_root="${TMPDIR:-$repo_root/.tmp}"
mkdir -p "$tmp_root"
tmp_probe="$tmp_root/ci-preflight.$$"
: >"$tmp_probe" || fail "failed to write temp probe under $tmp_root"
rm -f "$tmp_probe"

nix_cache_dir="${HOME}/.cache/nix"
if [[ -d "$nix_cache_dir" && ! -w "$nix_cache_dir" ]]; then
  fail "Nix cache directory is not writable: $nix_cache_dir"
fi

for cmd in bash git rg jq rustc cargo nix just node npm lake quint markdown-link-check; do
  require_cmd "$cmd"
done

echo "ci-preflight: disk free $(format_gib "$avail_kib") (threshold ${min_free_gib}.0 GiB)"
echo "ci-preflight: temp dir writable at $tmp_root"
echo "ci-preflight: required toolchain commands present"
