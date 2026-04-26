#!/usr/bin/env bash
set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  echo "gh is required for ci-github-dry-run" >&2
  exit 127
fi

gh auth status >/dev/null

branch="$(git branch --show-current)"
if [[ -z "$branch" ]]; then
  echo "ci-github-dry-run requires a named local branch" >&2
  exit 2
fi

repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
workflows=(
  ci.yml
  docs.yml
  conform.yml
  harness.yml
  lan.yml
  verify.yml
)

run_ids=()

for workflow in "${workflows[@]}"; do
  echo "dispatching ${workflow} on ${branch}"
  gh workflow run "$workflow" --repo "$repo" --ref "$branch"
done

for workflow in "${workflows[@]}"; do
  echo "waiting for ${workflow} run id"
  run_id=""
  for _ in {1..60}; do
    run_id="$(
      gh run list \
        --repo "$repo" \
        --workflow "$workflow" \
        --branch "$branch" \
        --event workflow_dispatch \
        --limit 20 \
        --json databaseId,createdAt \
        --jq ".[] | select(.createdAt >= \"${started_at}\") | .databaseId" |
        head -n 1
    )"
    if [[ -n "$run_id" ]]; then
      break
    fi
    sleep 5
  done
  if [[ -z "$run_id" ]]; then
    echo "failed to find workflow_dispatch run for ${workflow}" >&2
    exit 1
  fi
  run_ids+=("$run_id")
done

while :; do
  pending=0
  failed=0
  for run_id in "${run_ids[@]}"; do
    status="$(gh run view "$run_id" --repo "$repo" --json status -q .status)"
    conclusion="$(gh run view "$run_id" --repo "$repo" --json conclusion -q .conclusion)"
    url="$(gh run view "$run_id" --repo "$repo" --json url -q .url)"
    if [[ "$status" != "completed" ]]; then
      pending=$((pending + 1))
      continue
    fi
    if [[ "$conclusion" != "success" ]]; then
      failed=$((failed + 1))
      echo "failed: ${url} (${conclusion})" >&2
    fi
  done

  if [[ "$pending" -eq 0 ]]; then
    if [[ "$failed" -ne 0 ]]; then
      exit 1
    fi
    echo "ci-github-dry-run passed"
    exit 0
  fi

  echo "waiting for ${pending} GitHub workflow run(s)"
  sleep 30
done
