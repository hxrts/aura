#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

required_files=(
  "crates/aura-rendezvous/src/service.rs"
)

for file in "${required_files[@]}"; do
  if ! rg -q '#\[aura_macros::service_surface\(' "$file"; then
    echo "missing #[aura_macros::service_surface(...)] declaration in $file" >&2
    exit 1
  fi
done

exceptions="$(rg -n 'service_surface_(exception|allowlist|compat_alias)' crates scripts work docs || true)"
if [[ -n "$exceptions" ]]; then
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    file="${line%%:*}"
    line_no="${line#*:}"
    line_no="${line_no%%:*}"
    context="$(sed -n "${line_no},$((line_no + 2))p" "$file")"
    if [[ "$context" != *"owner ="* ]] || [[ "$context" != *"remove_by ="* ]]; then
      echo "service-surface exception in $file:$line_no must declare owner = ... and remove_by = ..." >&2
      exit 1
    fi
  done <<< "$exceptions"
fi

echo "service-surface declaration policy passed"
