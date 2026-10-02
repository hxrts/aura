#!/usr/bin/env bash
# Read-only, macOS/Linux-compatible inventory of Aura build artifacts.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="${1:-$repo_root}"
root="$(cd "$root" && pwd -P)"

size_kib() {
  local path="$1" sample
  if [[ ! -e "$path" ]]; then printf '0\n'; return; fi
  # Builds can unlink temporary outputs while this read-only inventory runs.
  # Keep a numeric partial sample instead of aborting the entire report.
  sample="$(du -sk "$path" 2>/dev/null | awk 'NR == 1 {print $1}' || true)"
  if [[ "$sample" =~ ^[0-9]+$ ]]; then printf '%s\n' "$sample";
  else printf 'unavailable\n'; fi
}

free_kib() {
  df -Pk "$root" | awk 'NR == 2 { print $4 }'
}

printf 'Aura disk report: %s (%s)\n' "$root" "$(hostname)"
printf 'Commit: %s\n' "$(git -C "$root" rev-parse HEAD 2>/dev/null || printf unknown)"
rust_host="$(rustc -vV 2>/dev/null | awk '/^host: / {print $2}' || true)"
printf 'Rust host: %s\n' "$rust_host"
printf 'Build context: lane=%s profile=%s target=%s features=%s\n' \
  "${AURA_BUILD_LANE:-unlabelled}" "${AURA_BUILD_PROFILE:-unlabelled}" \
  "${AURA_BUILD_TARGET_TRIPLE:-$rust_host}" "${AURA_BUILD_FEATURES:-unlabelled}"
printf 'Free: %s KiB\n' "$(free_kib)"
printf 'Checkout: %s KiB\n' "$(size_kib "$root")"

printf 'Other linked Aura worktrees (separate build caches, KiB):\n'
printf 'Checkout\tTarget\tDebug deps\tDebug incremental\tTrybuild\tPath\n'
while IFS= read -r line; do
  [[ "$line" == worktree\ * ]] || continue
  linked="${line#worktree }"
  [[ "$linked" != "$root" && -d "$linked" ]] || continue
  if [[ -L "$linked/target" ]]; then
    printf 'excluded\texcluded\texcluded\texcluded\texcluded\t%s (target symlink)\n' "$linked"
  else
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$(size_kib "$linked")" \
      "$(size_kib "$linked/target")" \
      "$(size_kib "$linked/target/debug/deps")" \
      "$(size_kib "$linked/target/debug/incremental")" \
      "$(size_kib "$linked/target/tests/trybuild")" "$linked"
  fi
done < <(git -C "$root" worktree list --porcelain 2>/dev/null)

paths=(
  target target/debug target/debug/deps target/debug/incremental
  target/tests/trybuild target/release target/release/deps target/release/build
  target/wasm-release target/wasm32-unknown-unknown target/dylint target/dx
  .tmp .tmp/e2e bin/aura
)
for path in "${paths[@]}"; do
  printf '%12s KiB  %s\n' "$(size_kib "$root/$path")" "$path"
done
for path in "$root"/.tmp/e2e/run/*/artifacts/runs; do
  [[ -d "$path" ]] || continue
  relative="${path#"$root"/}"
  printf '%12s KiB  %s\n' "$(size_kib "$path")" "$relative"
done

deps="$root/target/release/deps"
if [[ -d "$deps" ]]; then
  printf 'Release dependency artifact classes (allocated KiB):\n'
  for ext in rlib rmeta d o dylib a; do
    find "$deps" -maxdepth 1 -type f -name "*.$ext" -exec du -k {} + |
      awk -v ext="$ext" '{size += $1; count++} END {if (count) printf "%12d KiB  %5d  .%s\n", size, count, ext}'
  done
fi

fingerprints="$root/target/release/.fingerprint"
if [[ -d "$fingerprints" ]] && command -v jq >/dev/null 2>&1; then
  printf 'Release crates with multiple library fingerprints (count, feature sets):\n'
  find "$fingerprints" -type f -name 'lib-*.json' -exec jq -r \
    'input_filename + "\t" + (.features | tostring)' {} + |
    awk -F '\t' '{
      path=$1; sub(/\/lib-[^\/]*$/, "", path); sub(/^.*\//, "", path);
      sub(/-[[:xdigit:]]+$/, "", path); count[path]++;
      feature[path FS $2]=1
    } END {
      for (key in feature) {split(key, parts, FS); distinct[parts[1]]++}
      for (name in count) if (count[name] > 1)
        printf "%d\t%d\t%s\n", count[name], distinct[name], name
    }' | sort -nr | head -20
fi
