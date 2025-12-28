#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

fail=0

echo "Domain fact contract lint"

# Ensure DomainFact derive annotations include schema_version + context/context_fn.
while IFS= read -r line; do
  file="${line%%:*}"
  attr="${line#*:}"
  if [[ "$attr" != *"schema_version"* ]]; then
    echo "Missing schema_version in domain_fact attribute: ${file}"
    fail=1
  fi
  if [[ "$attr" != *"context"* && "$attr" != *"context_fn"* ]]; then
    echo "Missing context/context_fn in domain_fact attribute: ${file}"
    fail=1
  fi
done < <(rg -n --no-heading "#\\[domain_fact\\(" crates)

# Validate fact type IDs declare schema versions and reducers.
while IFS= read -r line; do
  file="${line%%:*}"
  const_name=$(echo "$line" | sed -E 's/.*pub const ([A-Z0-9_]+_FACT_TYPE_ID).*/\1/')
  type_id=$(echo "$line" | sed -E 's/.*= "([^"]+)".*/\1/')

  if [[ -z "$const_name" || -z "$type_id" ]]; then
    continue
  fi

  if rg -n --no-heading "#\\[domain_fact\\(.*type_id = \"${type_id}\"" "$file" >/dev/null; then
    :
  else
    schema_const="${const_name/_TYPE_ID/_SCHEMA_VERSION}"
    if ! rg -n --no-heading "pub const ${schema_const}: u16" "$file" >/dev/null; then
      echo "Missing schema version constant ${schema_const} for ${type_id} in ${file}"
      fail=1
    fi
    if ! rg -n --no-heading "encode_domain_fact|VersionedMessage" "$file" >/dev/null; then
      echo "Missing canonical encoding helper for ${type_id} in ${file}"
      fail=1
    fi
  fi

  if ! rg -n --no-heading "FactReducer" "$file" >/dev/null; then
    echo "Missing FactReducer implementation near ${const_name} in ${file}"
    fail=1
  fi
done < <(rg -n --no-heading "pub const [A-Z0-9_]+_FACT_TYPE_ID: &str = \"[^\"]+\";" crates)

if [[ "$fail" -ne 0 ]]; then
  echo "Domain fact contract lint failed"
  exit 1
fi

echo "Domain fact contract lint passed"
