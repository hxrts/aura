#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

allowlist_file="scripts/check/capability-gated-mutation.allowlist"

fail() {
  echo "capability-gated-mutation: $*" >&2
  exit 1
}

[[ -f "$allowlist_file" ]] || fail "missing allowlist: $allowlist_file"

# Thin inventory check: public mutation/publication surfaces with
# ownership-critical names must accept an explicit capability artifact rather
# than relying on ambient reachability.

violations=()
legacy_exemptions=0

while IFS= read -r match; do
  [[ -z "$match" ]] && continue

  allowed=0
  while IFS= read -r pattern; do
    [[ -z "$pattern" || "$pattern" =~ ^# ]] && continue
    if [[ "$match" =~ $pattern ]]; then
      allowed=1
      legacy_exemptions=$((legacy_exemptions + 1))
      break
    fi
  done < "$allowlist_file"

  if (( allowed == 0 )); then
    violations+=("$match")
  fi
done < <(
  while IFS= read -r file; do
    perl -0ne '
      while (
        /pub\s+(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\((.*?)\)\s*(?:->.*?)?\s*\{/sg
      ) {
        my ($name, $args) = ($1, $2);
        next unless $name =~ /^(?:publish_authoritative_|replace_authoritative_semantic_facts_of_kind|issue_operation_handle|issue_owner_token)/;
        next if $args =~ /(?:LifecyclePublicationCapability|ReadinessPublicationCapability|ActorIngressMutationCapability|OwnershipTransferCapability|AuthorizedLifecyclePublication)/;

        print "$ARGV:$name\n";
      }
    ' "$file"
  done < <(find crates -path '*/src/*.rs' -type f | sort)
)

if (( ${#violations[@]} > 0 )); then
  printf '%s\n' "${violations[@]}" >&2
  fail "public ownership-critical mutation/publication surfaces are missing explicit capability gating"
fi

echo "capability-gated mutation: clean (${legacy_exemptions} temporary exemptions)"
