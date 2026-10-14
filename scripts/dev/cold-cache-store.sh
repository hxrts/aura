#!/usr/bin/env bash
# Optional local cold store for one inactive compiler cache lane. Never accepts
# an unmounted/same-device destination or a network filesystem.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
action='' lane='' root="$repo_root" lock_owner=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --export|--restore) action="${1#--}"; shift ;;
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --lock-owned-by) lock_owner="${2:?missing owner}"; shift 2 ;;
    *) echo 'usage: cold-cache-store.sh --export|--restore --lane LANE [--root PATH] [--lock-owned-by PID]' >&2; exit 2 ;;
  esac
done
case "$lane" in
  debug-incremental) relative=target/debug/incremental ;;
  wasm-debug) relative=target/wasm32-unknown-unknown/debug ;;
  dylint) relative=target/dylint ;;
  trybuild) relative=target/tests/trybuild ;;
  *) echo 'cold-cache-store: unsupported cache lane' >&2; exit 2 ;;
esac
[[ -n "$action" ]] || { echo 'cold-cache-store: action required' >&2; exit 2; }
root="$(cd "$root" && pwd -P)"
store="${AURA_COLD_CACHE_STORE:-}"
[[ -n "$store" && -d "$store" && ! -L "$store" ]] || {
  echo 'cold-cache-store: AURA_COLD_CACHE_STORE must be an existing local directory' >&2; exit 2;
}
store="$(cd "$store" && pwd -P)"
stat_owner() {
  local result
  result="$(stat -f %u "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %u "$1"
}
stat_mode() {
  local result
  result="$(stat -f %Lp "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-7]{3,4}$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %a "$1"
}
mode="$(stat_mode "$store")"
[[ "$(stat_owner "$store")" == "$(id -u)" && "$mode" =~ ^[0-7]{3,4}$ ]] || {
  echo 'cold-cache-store: destination must be owned by current user' >&2; exit 1;
}
(( (8#$mode & 8#022) == 0 )) || {
  echo 'cold-cache-store: destination is group/other writable' >&2; exit 1;
}
device_id() {
  local result
  result="$(stat -f %d "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %d "$1"
}
filesystem_type() {
  local result
  result="$(stat -f %T "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[A-Za-z0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -f -c %T "$1"
}
[[ "$(device_id "$root")" != "$(device_id "$store")" ]] || {
  echo 'cold-cache-store: destination shares the source filesystem' >&2; exit 1;
}
case "$(filesystem_type "$store")" in
  apfs|hfs|hfsplus|ext2|ext3|ext4|xfs|btrfs|zfs|tmpfs) ;;
  *) echo 'cold-cache-store: destination is not a recognized local filesystem' >&2; exit 1 ;;
esac
[[ ! -L "$root/target" && ! -L "$root/${relative%/*}" ]] || {
  echo 'cold-cache-store: target path contains a symlink' >&2; exit 1;
}
[[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]] || {
  echo 'cold-cache-store: source checkout must be clean' >&2; exit 1;
}
revision="$(git -C "$root" rev-parse HEAD)"
root_key="$(printf '%s' "$root" | sha256sum | awk '{print $1}')"
bucket="$store/aura-cold-cache/$root_key/$revision/$lane"
for ancestor in "$store/aura-cold-cache" "$store/aura-cold-cache/$root_key" \
  "$store/aura-cold-cache/$root_key/$revision" "$bucket"; do
  [[ ! -L "$ancestor" ]] || { echo 'cold-cache-store: linked store ancestor' >&2; exit 1; }
  [[ ! -e "$ancestor" || -d "$ancestor" ]] || {
    echo 'cold-cache-store: non-directory store ancestor' >&2; exit 1;
  }
done
path="$root/$relative"
shared_dir="${AURA_BUILD_SHARED_DIR:-$HOME/.cache/aura-build}"
mkdir -p "$shared_dir/cold-keys"
[[ "$(device_id "$shared_dir")" == "$(device_id "$root")" ]] || {
  echo 'cold-cache-store: signing key must remain on source filesystem' >&2; exit 1;
}
chmod 700 "$shared_dir/cold-keys"
signing_key="$shared_dir/cold-keys/$root_key.pem"
lock_dir="$root/target/.aura-build-budget.lock"
owns_lock=0
admission_lock=''
reservation=''
public_key=''
member_list=''
kind_list=''
cleanup() {
  [[ -z "$public_key" ]] || rm -f "$public_key"
  [[ -z "$member_list" ]] || rm -f "$member_list"
  [[ -z "$kind_list" ]] || rm -f "$kind_list"
  [[ -z "$reservation" ]] || rm -f "$reservation"
  if [[ -n "$admission_lock" ]]; then
    rm -f "$admission_lock/pid"
    rmdir "$admission_lock" 2>/dev/null || true
  fi
  if (( owns_lock == 1 )); then rm -f "$lock_dir/pid"; rmdir "$lock_dir" 2>/dev/null || true; fi
  [[ -z "${pending:-}" ]] || rm -rf -- "$pending"
}
trap cleanup EXIT
if [[ -n "$lock_owner" ]]; then
  [[ "$lock_owner" =~ ^[0-9]+$ && "$lock_owner" == "$PPID" && -f "$lock_dir/pid" ]] || exit 1
  [[ "$(cat "$lock_dir/pid")" == "$lock_owner" ]] || exit 1
else
  mkdir "$lock_dir" 2>/dev/null || { echo 'cold-cache-store: checkout lock busy' >&2; exit 1; }
  owns_lock=1
  printf '%s\n' "$$" > "$lock_dir/pid"
fi
source "$repo_root/scripts/dev/scoped-builders.sh"
[[ -z "$(aura_scoped_processes "$root" 'cargo|rustc|rustdoc|dx|cargo-dylint|cargo-sweep|cargo-kani|kani-driver' "$$")" ]] || {
  echo 'cold-cache-store: builder active' >&2; exit 1;
}
command -v lsof >/dev/null || { echo 'cold-cache-store: lsof required' >&2; exit 1; }
open_files="$(lsof -n -P 2>/dev/null)" || { echo 'cold-cache-store: open-file scan failed' >&2; exit 1; }
if printf '%s\n' "$open_files" | grep -F -- "$path" >/dev/null; then
  echo 'cold-cache-store: lane has open files' >&2; exit 1
fi
if [[ "$action" == export ]]; then
  [[ -d "$path" && ! -L "$path" ]] || { echo 'cold-cache-store: lane absent or linked' >&2; exit 1; }
  [[ -z "$(find "$path" -type l -print -quit)" ]] || { echo 'cold-cache-store: linked artifact' >&2; exit 1; }
  [[ -z "$(find "$path" -type f -links +1 -print -quit)" ]] || { echo 'cold-cache-store: hard-linked artifact' >&2; exit 1; }
  [[ ! -e "$signing_key" || ( -f "$signing_key" && ! -L "$signing_key" ) ]] || {
    echo 'cold-cache-store: invalid local signing key' >&2; exit 1;
  }
  if [[ ! -f "$signing_key" ]]; then
    umask 077
    temp_key="$(mktemp "$shared_dir/cold-keys/.key.XXXXXX")"
    openssl ecparam -name prime256v1 -genkey -noout -out "$temp_key" 2>/dev/null
    chmod 600 "$temp_key"
    mv "$temp_key" "$signing_key"
  fi
  umask 077
  mkdir -p "$bucket"
  chmod 700 "$store/aura-cold-cache" "$store/aura-cold-cache/$root_key" \
    "$store/aura-cold-cache/$root_key/$revision" "$bucket"
  pending="$(mktemp -d "$bucket/.pending.XXXXXX")"
  tar -C "$path" -cf "$pending/archive.tar" .
  sha="$(sha256sum "$pending/archive.tar" | awk '{print $1}')"
  bytes="$(wc -c < "$pending/archive.tar" | tr -d ' ')"
  jq -n --arg revision "$revision" --arg root "$root" --arg lane "$lane" \
    --arg sha "$sha" --argjson bytes "$bytes" \
    '{schema:1,source_revision:$revision,source_root:$root,lane:$lane,sha256:$sha,bytes:$bytes}' \
    > "$pending/manifest.json"
  openssl dgst -sha256 -sign "$signing_key" -out "$pending/manifest.sig" \
    "$pending/manifest.json"
  chmod 600 "$pending/archive.tar" "$pending/manifest.json" "$pending/manifest.sig"
  ready="$bucket/ready.$(basename "$pending" | sed 's/^\.pending\.//')"
  mv "$pending" "$ready"
  pending=''
  # The guarded pruner repeats its own lock, process, open-file and path checks.
  bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" --lane "$lane" \
    --lock-owned-by "$$" --apply
  printf 'cold-cache-store: exported %s KiB to %s\n' "$(( (bytes + 1023) / 1024 ))" "$ready"
else
  [[ ! -e "$path" && ! -L "$path" ]] || { echo 'cold-cache-store: restore lane already exists' >&2; exit 1; }
  snapshot=''
  for choice in "$bucket"/ready.*; do
    [[ -d "$choice" && ! -L "$choice" ]] || continue
    if [[ -z "$snapshot" || "$choice" -nt "$snapshot" ]]; then snapshot="$choice"; fi
  done
  [[ -n "$snapshot" && ! -L "$snapshot" && -f "$snapshot/archive.tar" &&
    -f "$snapshot/manifest.json" && -f "$snapshot/manifest.sig" &&
    -f "$signing_key" && ! -L "$signing_key" ]] || {
    echo 'cold-cache-store: no complete snapshot for source and lane' >&2; exit 1;
  }
  public_key="$(mktemp "$shared_dir/cold-keys/.pub.XXXXXX")"
  openssl pkey -in "$signing_key" -pubout -out "$public_key" 2>/dev/null
  openssl dgst -sha256 -verify "$public_key" -signature "$snapshot/manifest.sig" \
    "$snapshot/manifest.json" >/dev/null || {
      echo 'cold-cache-store: manifest signature mismatch' >&2; exit 1;
    }
  jq -e --arg revision "$revision" --arg root "$root" --arg lane "$lane" \
    '.schema==1 and .source_revision==$revision and .source_root==$root and .lane==$lane and
     (.sha256 | type=="string" and test("^[0-9a-f]{64}$")) and
     (.bytes | type=="number" and .>0 and .<1099511627776)' \
    "$snapshot/manifest.json" >/dev/null
  expected="$(jq -r '.sha256' "$snapshot/manifest.json")"
  actual="$(sha256sum "$snapshot/archive.tar" | awk '{print $1}')"
  [[ "$expected" == "$actual" ]] || { echo 'cold-cache-store: archive hash mismatch' >&2; exit 1; }
  bytes="$(wc -c < "$snapshot/archive.tar" | tr -d ' ')"
  [[ "$bytes" == "$(jq -r '.bytes' "$snapshot/manifest.json")" ]] || {
    echo 'cold-cache-store: archive size mismatch' >&2; exit 1;
  }
  # The archive is signed, but still reject any path that could leave the
  # selected lane and any link entry before invoking tar's extractor.
  member_list="$(mktemp "$shared_dir/cold-keys/.members.XXXXXX")"
  kind_list="$(mktemp "$shared_dir/cold-keys/.kinds.XXXXXX")"
  tar -tf "$snapshot/archive.tar" > "$member_list"
  tar -tvf "$snapshot/archive.tar" | cut -c1 > "$kind_list"
  while IFS= read -r member; do
    [[ "$member" == . || "$member" == ./* ]] || { echo 'cold-cache-store: unsafe archive member' >&2; exit 1; }
    [[ ! "$member" =~ (^|/)\.\.(/|$) ]] || { echo 'cold-cache-store: parent archive member' >&2; exit 1; }
  done < "$member_list"
  while IFS= read -r kind; do
    [[ "$kind" == - || "$kind" == d ]] || { echo 'cold-cache-store: linked archive member' >&2; exit 1; }
  done < "$kind_list"
  mkdir -p "$shared_dir/reservations"
  admission_lock="$shared_dir/admission.lock"
  acquired=0
  for (( attempt=0; attempt<50; attempt++ )); do
    if mkdir "$admission_lock" 2>/dev/null; then acquired=1; break; fi
    holder="$(cat "$admission_lock/pid" 2>/dev/null || true)"
    if [[ "$holder" =~ ^[0-9]+$ ]] && ! kill -0 "$holder" 2>/dev/null; then
      rm -f "$admission_lock/pid"
      rmdir "$admission_lock" 2>/dev/null || true
    fi
    sleep 0.2
  done
  (( acquired == 1 )) || {
    admission_lock=''
    echo 'cold-cache-store: admission lock busy' >&2; exit 1;
  }
  printf '%s\n' "$$" > "$admission_lock/pid"
  others=0
  for file in "$shared_dir"/reservations/*; do
    [[ -f "$file" ]] || continue
    pid="${file##*/}"
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    if ! kill -0 "$pid" 2>/dev/null; then rm -f "$file"; continue; fi
    kib="$(awk 'NR==1 {print $1}' "$file")"
    [[ "$kib" =~ ^[0-9]+$ ]] && others=$((others + kib))
  done
  free="$(df -Pk "$root" | awk 'NR==2 {print $4}')"
  floor=$(( ${AURA_BUILD_MIN_FREE_GIB:-15} * 1024 * 1024 ))
  reserve_kib=$(( (bytes + 1023) / 1024 + 1024 * 1024 ))
  (( free - others - reserve_kib >= floor )) || {
    echo 'cold-cache-store: restore would breach admission floor' >&2; exit 1;
  }
  reservation="$shared_dir/reservations/$$"
  printf '%s\n' "$reserve_kib" > "$reservation"
  rm -f "$admission_lock/pid"
  rmdir "$admission_lock"
  admission_lock=''
  mkdir -p "$path"
  chmod 700 "$path"
  if ! tar -C "$path" -xf "$snapshot/archive.tar"; then
    rm -rf -- "$path"
    exit 1
  fi
  printf 'cold-cache-store: restored %s\n' "$snapshot"
fi
