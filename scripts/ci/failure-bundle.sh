#!/usr/bin/env bash
# Bundle CI failure metadata and artifacts for triage and replay.
set -euo pipefail

BUNDLE_DIR="${1:?bundle dir required}"
ARTIFACT_ROOT="${2:-artifacts}"
REPLAY_COMMAND="${3:-}"

mkdir -p "$BUNDLE_DIR"

# Stable metadata for quick triage/replay.
{
  echo "workflow=${GITHUB_WORKFLOW:-unknown}"
  echo "job=${GITHUB_JOB:-unknown}"
  echo "run_id=${GITHUB_RUN_ID:-unknown}"
  echo "run_attempt=${GITHUB_RUN_ATTEMPT:-unknown}"
  echo "event=${GITHUB_EVENT_NAME:-unknown}"
  echo "ref=${GITHUB_REF:-unknown}"
  echo "sha=${GITHUB_SHA:-unknown}"
  echo "actor=${GITHUB_ACTOR:-unknown}"
  echo "runner_os=${RUNNER_OS:-unknown}"
  echo "runner_arch=${RUNNER_ARCH:-unknown}"
  echo "generated_at_utc=$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
} > "$BUNDLE_DIR/metadata.env"

if [[ -n "$REPLAY_COMMAND" ]]; then
  {
    echo "#!/usr/bin/env bash"
    echo "set -euo pipefail"
    echo "$REPLAY_COMMAND"
  } > "$BUNDLE_DIR/replay.sh"
  chmod +x "$BUNDLE_DIR/replay.sh"
fi

# Environment + workspace diagnostics.
(
  echo "=== uname -a ==="
  uname -a || true
  echo
  echo "=== rustc --version ==="
  rustc --version || true
  echo
  echo "=== cargo --version ==="
  cargo --version || true
  echo
  echo "=== nix --version ==="
  nix --version || true
  echo
  echo "=== df -h ==="
  df -h || true
  echo
  echo "=== git status --short ==="
  git status --short || true
  echo
  echo "=== git log -n 5 --oneline ==="
  git log -n 5 --oneline || true
) > "$BUNDLE_DIR/system.txt" 2>&1

# Capture selected artifact files deterministically.
if [[ -d "$ARTIFACT_ROOT" ]]; then
  while IFS= read -r -d '' src; do
    rel="${src#${ARTIFACT_ROOT}/}"
    dst="$BUNDLE_DIR/artifacts/$rel"
    mkdir -p "$(dirname "$dst")"
    cp "$src" "$dst"
  done < <(
    find "$ARTIFACT_ROOT" -type f \
      \( -name "*.log" -o -name "*.json" -o -name "*.txt" -o -name "*.md" -o -name "*.xml" -o -name "*.trace" -o -name "*.out" -o -name "*.err" -o -name "*.png" -o -name "*.jpg" -o -name "*.jpeg" -o -name "*.toml" \) \
      -print0 | sort -z
  )
fi

# Keep an index for quick scanning.
find "$BUNDLE_DIR" -type f | LC_ALL=C sort > "$BUNDLE_DIR/file_index.txt"
