#!/usr/bin/env bash
# Install a signed Aura binary atomically so failed/interrupted builds leave
# the previous working bin/aura in place.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_binary="${1:-$repo_root/target/release/aura}"
destination="${2:-$repo_root/bin/aura}"
[[ -f "$source_binary" ]] || { echo "missing binary: $source_binary" >&2; exit 1; }
mkdir -p "$(dirname "$destination")"
temporary="$(mktemp "$(dirname "$destination")/.aura-install.XXXXXX")"
trap 'rm -f "$temporary"' EXIT
cp "$source_binary" "$temporary"
chmod 755 "$temporary"
codesign -s - "$temporary"
mv -f "$temporary" "$destination"
printf 'Binary available at: %s\n' "$destination"
