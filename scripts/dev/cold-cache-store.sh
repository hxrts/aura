#!/usr/bin/env bash
# Optional cold store for one inactive compiler cache lane. Local destinations
# must have independent capacity; the remote backend requires SSH to a local
# ext4 volume on a separate Linux host.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
action='' lane='' root="$repo_root" lock_owner='' run_host='' run_id='' evidence=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --export|--restore) action="${1#--}"; shift ;;
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --run-host) run_host="${2:?missing run host}"; shift 2 ;;
    --run-id) run_id="${2:?missing run id}"; shift 2 ;;
    --lock-owned-by) lock_owner="${2:?missing owner}"; shift 2 ;;
    *) echo 'usage: cold-cache-store.sh --export|--restore --lane LANE [--root PATH] [--lock-owned-by PID]' >&2; exit 2 ;;
  esac
done
if [[ -n "$run_host" || -n "$run_id" ]]; then
  [[ -z "$lane" && "$run_host" =~ ^[A-Za-z0-9_-]+$ &&
     "$run_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$run_id" != *..* ]] || {
    echo 'cold-cache-store: invalid evidence run identity' >&2; exit 2;
  }
  evidence=1
  lane="evidence-h${#run_host}-$run_host-r${#run_id}-$run_id"
  relative=".tmp/e2e/run/$run_host/artifacts/runs/$run_id"
else
case "$lane" in
  debug-incremental) relative=target/debug/incremental ;;
  wasm-debug) relative=target/wasm32-unknown-unknown/debug ;;
  wasm-profile) relative=target/wasm32-unknown-unknown/wasm ;;
  wasm-root-profile) relative=target/wasm ;;
  dylint) relative=target/dylint ;;
  trybuild) relative=target/tests/trybuild ;;
  release) relative=target/release ;;
  *) echo 'cold-cache-store: unsupported cache lane' >&2; exit 2 ;;
esac
fi
[[ -n "$action" ]] || { echo 'cold-cache-store: action required' >&2; exit 2; }
root="$(cd "$root" && pwd -P)"
remote_host="${AURA_COLD_CACHE_REMOTE_HOST:-}"
remote_dir="${AURA_COLD_CACHE_REMOTE_DIR:-}"
remote_mode=0
if [[ -n "$remote_host" || -n "$remote_dir" ]]; then
  [[ -n "$remote_host" && -n "$remote_dir" && -z "${AURA_COLD_CACHE_STORE:-}" &&
     "$remote_host" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]*@[A-Za-z0-9_][A-Za-z0-9_.-]*$ &&
     "$remote_dir" =~ ^/[A-Za-z0-9_./-]+$ &&
     "$remote_dir" != *'/../'* && "$remote_dir" != *'/./'* &&
     "$remote_dir" != */.. && "$remote_dir" != */. ]] || {
    echo 'cold-cache-store: invalid remote host/directory configuration' >&2; exit 2;
  }
  remote_mode=1
  store="${AURA_BUILD_SHARED_DIR:-$HOME/.cache/aura-build}/cold-remote-receipts"
else
  store="${AURA_COLD_CACHE_STORE:-}"
  [[ -n "$store" && -d "$store" && ! -L "$store" ]] || {
    echo 'cold-cache-store: AURA_COLD_CACHE_STORE must be an existing local directory' >&2; exit 2;
  }
  store="$(cd "$store" && pwd -P)"
fi
if (( evidence == 1 && remote_mode == 0 )); then
  echo 'cold-cache-store: protected run archival requires independent remote ext4' >&2
  exit 2
fi
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
stat_mtime() {
  local result
  result="$(stat -f %m "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %Y "$1"
}
stat_size() {
  local result
  result="$(stat -f %z "$1" 2>/dev/null || true)"
  if [[ "$result" =~ ^[0-9]+$ ]]; then printf '%s\n' "$result"; return; fi
  stat -c %s "$1"
}
if (( remote_mode == 0 )); then
  mode="$(stat_mode "$store")"
  [[ "$(stat_owner "$store")" == "$(id -u)" && "$mode" =~ ^[0-7]{3,4}$ ]] || {
    echo 'cold-cache-store: destination must be owned by current user' >&2; exit 1;
  }
  (( (8#$mode & 8#022) == 0 )) || {
    echo 'cold-cache-store: destination is group/other writable' >&2; exit 1;
  }
fi
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
if (( remote_mode == 0 )); then
  [[ "$(device_id "$root")" != "$(device_id "$store")" ]] || {
    echo 'cold-cache-store: destination shares the source filesystem' >&2; exit 1;
  }
  source_fs="$(filesystem_type "$root")"
  store_fs="$(filesystem_type "$store")"
  case "$store_fs" in
    apfs|hfs|hfsplus|ext2|ext3|ext4|xfs|btrfs|zfs|tmpfs) ;;
    *) echo 'cold-cache-store: destination is not a recognized local filesystem' >&2; exit 1 ;;
  esac
fi
# Separate APFS volumes can share one container's free-space pool. Exporting
# there consumes the same headroom it is meant to recover. On macOS, resolve
# both container identities from the mounted devices and fail closed if either
# identity is unavailable. Other filesystem behavior is unchanged.
if (( remote_mode == 0 )) && [[ "$(uname -s)" == Darwin && "$source_fs" == apfs && "$store_fs" == apfs ]]; then
  apfs_pool() {
    local mount_device
    mount_device="$(df -P "$1" | awk 'NR == 2 {print $1}')"
    [[ "$mount_device" == /dev/* ]] || return 1
    diskutil info -plist "$mount_device" |
      plutil -extract APFSContainerReference raw -o - -
  }
  source_pool="$(apfs_pool "$root" 2>/dev/null)" || source_pool=''
  store_pool="$(apfs_pool "$store" 2>/dev/null)" || store_pool=''
  [[ -n "$source_pool" && -n "$store_pool" ]] || {
    echo 'cold-cache-store: cannot verify independent APFS container capacity' >&2; exit 1;
  }
  [[ "$source_pool" != "$store_pool" ]] || {
    echo 'cold-cache-store: destination shares the source APFS free-space pool' >&2; exit 1;
  }
fi
[[ ! -L "$root/target" && ! -L "$root/${relative%/*}" ]] || {
  echo 'cold-cache-store: target path contains a symlink' >&2; exit 1;
}
[[ -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]] || {
  echo 'cold-cache-store: source checkout must be clean' >&2; exit 1;
}
revision="$(git -C "$root" rev-parse HEAD)"
root_key="$(printf '%s' "$root" | sha256sum | awk '{print $1}')"
bucket="$store/aura-cold-cache/$root_key/$revision/$lane"
if (( remote_mode == 0 )); then
  for ancestor in "$store/aura-cold-cache" "$store/aura-cold-cache/$root_key" \
    "$store/aura-cold-cache/$root_key/$revision" "$bucket"; do
    [[ ! -L "$ancestor" ]] || { echo 'cold-cache-store: linked store ancestor' >&2; exit 1; }
    [[ ! -e "$ancestor" || -d "$ancestor" ]] || {
      echo 'cold-cache-store: non-directory store ancestor' >&2; exit 1;
    }
  done
fi
path="$root/$relative"
evidence_runs="$root/.tmp/e2e/run/$run_host/artifacts/runs"
evidence_lock=''
evidence_members() {
  local base="$1" member relative kind bytes digest
  while IFS= read -r -d '' member; do
    relative="${member#"$base"}"
    [[ "$relative" != *$'\n'* && "$relative" != *$'\r'* &&
       "$relative" != *$'\t'* ]] || return 1
    if [[ -d "$member" ]]; then kind=d; bytes=0; digest=-
    elif [[ -f "$member" && ! -L "$member" ]]; then
      kind=f; bytes="$(stat_size "$member")"
      digest="$(sha256sum "$member" | awk '{print $1}')"
    else return 1; fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$relative" "$kind" \
      "$(stat_mode "$member")" "$(stat_mtime "$member")" "$bytes" "$digest"
  done < <(find "$base" -print0 | LC_ALL=C sort -z)
}
evidence_validate_payload() {
  local candidate="$1" check_newest="$2" manifest="$1/.aura-retention.json"
  local member mode owner sibling newest=-1 selected_epoch
  [[ -d "$candidate" && ! -L "$candidate" && -f "$manifest" && ! -L "$manifest" ]] || return 1
  [[ -z "$(find "$candidate" ! -type d ! -type f -print -quit)" &&
     -z "$(find "$candidate" -type f -links +1 -print -quit)" ]] || return 1
  while IFS= read -r -d '' member; do
    mode="$(stat_mode "$member")"; owner="$(stat_owner "$member")"
    [[ "$owner" == "$(id -u)" ]] || return 1
    if [[ -d "$member" ]]; then [[ "$mode" == 700 ]] || return 1
    else [[ "$mode" == 600 ]] || return 1; fi
  done < <(find "$candidate" -print0)
  jq -e --arg id "$run_id" '
    .schema_version==1 and .run_id==$id and .state=="completed" and
    (.outcome=="success" or .outcome=="failed") and .pinned==false and
    (.finished_epoch|type=="number" and .>=0)' "$manifest" >/dev/null || return 1
  if (( check_newest == 1 )); then
    selected_epoch="$(jq -r '.finished_epoch' "$manifest")"
    if [[ "$(jq -r '.outcome' "$manifest")" == success ]]; then
      for sibling in "$evidence_runs"/*/.aura-retention.json; do
        [[ -f "$sibling" && ! -L "$sibling" ]] || continue
        if jq -e '.schema_version==1 and .state=="completed" and
          .outcome=="success" and (.finished_epoch|type=="number")' "$sibling" >/dev/null 2>&1; then
          local epoch
          epoch="$(jq -r '.finished_epoch' "$sibling")"
          (( epoch > newest )) && newest="$epoch"
        fi
      done
      (( selected_epoch < newest )) || {
        echo 'cold-cache-store: newest completed success remains local' >&2; return 1;
      }
    fi
  fi
}
evidence_validate_source() {
  local ancestor active
  for ancestor in "$root/.tmp" "$root/.tmp/e2e" "$root/.tmp/e2e/run" \
    "$root/.tmp/e2e/run/$run_host" "$root/.tmp/e2e/run/$run_host/artifacts" \
    "$evidence_runs"; do
    [[ -d "$ancestor" && ! -L "$ancestor" ]] || return 1
  done
  active="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
  [[ -z "$active" ]] || { echo 'cold-cache-store: harness consumer active' >&2; return 1; }
  evidence_validate_payload "$path" 1
}
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
restore_stage=''
remote_lock_token=''
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
  [[ -z "$evidence_lock" ]] || rmdir "$evidence_lock" 2>/dev/null || true
  [[ -z "$restore_stage" ]] || rm -rf -- "$restore_stage"
  [[ -z "${pending:-}" ]] || rm -rf -- "$pending"
  if [[ -n "$remote_lock_token" ]]; then
    remote_ssh "test \"\$(cat $remote_dir/aura-cold-cache/.capacity.lock/token)\" = $remote_lock_token && rm -- $remote_dir/aura-cold-cache/.capacity.lock/token && rmdir -- $remote_dir/aura-cold-cache/.capacity.lock" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
if [[ -n "$lock_owner" ]]; then
  [[ "$lock_owner" =~ ^[0-9]+$ && "$lock_owner" == "$PPID" && -f "$lock_dir/pid" ]] || exit 1
  [[ "$(cat "$lock_dir/pid")" == "$lock_owner" ]] || exit 1
else
  mkdir "$lock_dir" 2>/dev/null || { echo 'cold-cache-store: checkout lock busy' >&2; exit 1; }
  owns_lock=1
  printf '%s\n' "$$" > "$lock_dir/pid"
fi
if (( evidence == 1 )); then
  [[ -d "$evidence_runs" && ! -L "$evidence_runs" ]] || {
    echo 'cold-cache-store: evidence run root absent or linked' >&2; exit 1;
  }
  evidence_lock="$evidence_runs/.aura-retention.lock"
  mkdir -m 700 "$evidence_lock" 2>/dev/null || {
    evidence_lock=''
    echo 'cold-cache-store: retention owner lock busy' >&2; exit 1;
  }
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
remote_ssh() {
  ssh -T -o BatchMode=yes -o StrictHostKeyChecking=yes -o ConnectTimeout=10 \
    "$remote_host" "$@"
}
remote_probe() {
  local info
  info="$(remote_ssh "bash -s -- $remote_dir" <<'REMOTE'
set -euo pipefail
dir=$1
[[ -d "$dir" && ! -L "$dir" ]]
[[ "$(readlink -f "$dir")" == "$dir" ]]
[[ "$(stat -c %u "$dir")" == "$(id -u)" ]]
[[ "$(stat -c %a "$dir")" == 700 ]]
[[ "$(uname -s)" == Linux ]]
[[ "$(findmnt -n -o FSTYPE -T "$dir")" == ext4 ]]
device="$(findmnt -n -o SOURCE -T "$dir")"
[[ "$device" == /dev/* && -b "$device" ]]
machine="$(cat /etc/machine-id)"
[[ "$machine" =~ ^[0-9a-f]{32}$ ]]
free="$(df -Pk "$dir" | awk 'NR==2 {print $4}')"
[[ "$free" =~ ^[0-9]+$ ]]
printf '%s %s %s\n' "$machine" "$device" "$free"
REMOTE
)" || { echo 'cold-cache-store: remote volume identity, ext4 mount, owner, or permissions refused' >&2; return 1; }
  read -r remote_machine remote_device remote_free <<< "$info"
  [[ "$remote_machine" =~ ^[0-9a-f]{32}$ && "$remote_device" == /dev/* &&
     "$remote_free" =~ ^[0-9]+$ ]] || {
    echo 'cold-cache-store: invalid remote capacity witness' >&2; return 1;
  }
  if [[ "$(uname -s)" == Linux ]]; then
    local local_machine
    local_machine="$(cat /etc/machine-id 2>/dev/null || true)"
    [[ "$local_machine" =~ ^[0-9a-f]{32}$ && "$local_machine" != "$remote_machine" ]] || {
      echo 'cold-cache-store: remote host is not independently identified from Linux source' >&2
      return 1
    }
  fi
}
remote_snapshot="$remote_dir/aura-cold-cache/$root_key/$revision/$lane"
remote_ready="$remote_snapshot/ready"
remote_pending="$remote_snapshot/.pending"
remote_location="$bucket/location.json"
remote_location_sig="$bucket/location.sig"
remote_verify_ready() {
  remote_ssh "bash -s -- $remote_ready $evidence" <<'REMOTE'
set -euo pipefail
ready=$1; evidence=$2
[[ -d "$ready" && ! -L "$ready" && "$(stat -c %a "$ready")" == 700 ]]
names=(archive.tar manifest.json manifest.sig location.json location.sig)
[[ "$evidence" == 0 ]] || names+=(members.tsv)
for name in "${names[@]}"; do
  file="$ready/$name"
  [[ -f "$file" && ! -L "$file" && "$(stat -c %a "$file")" == 600 &&
     "$(stat -c %u "$file")" == "$(id -u)" ]]
done
REMOTE
}
remote_local_bucket_preflight() {
  local ancestor
  for ancestor in "$store" "$store/aura-cold-cache" "$store/aura-cold-cache/$root_key" \
    "$store/aura-cold-cache/$root_key/$revision" "$bucket"; do
    [[ ! -L "$ancestor" && ( ! -e "$ancestor" || -d "$ancestor" ) ]] || {
      echo 'cold-cache-store: linked or invalid local receipt ancestor' >&2; return 1;
    }
  done
}
remote_export() {
  if (( evidence == 1 )); then
    evidence_validate_source || { echo 'cold-cache-store: evidence source not safely inactive' >&2; return 1; }
  fi
  [[ -d "$path" && ! -L "$path" ]] || { echo 'cold-cache-store: lane absent or linked' >&2; return 1; }
  [[ -z "$(find "$path" -type l -print -quit)" &&
     -z "$(find "$path" -type f -links +1 -print -quit)" ]] || {
    echo 'cold-cache-store: linked artifact' >&2; return 1;
  }
  [[ -z "$(find "$path" ! -type d ! -type f -print -quit)" ]] || {
    echo 'cold-cache-store: unsupported source member type' >&2; return 1;
  }
  while IFS= read -r -d '' member; do
    [[ "$member" != *$'\n'* && "$member" != *$'\r'* ]] || {
      echo 'cold-cache-store: source member has a control newline' >&2; return 1;
    }
  done < <(find "$path" -print0)
  remote_probe
  remote_local_bucket_preflight
  [[ ! -e "$remote_location" && ! -L "$remote_location" &&
     ! -e "$remote_location_sig" && ! -L "$remote_location_sig" ]] || {
    echo 'cold-cache-store: existing location receipt refuses overwrite' >&2; return 1;
  }
  local archive_bytes source_allocated_kib remote_floor expected_sha actual_sha remote_bytes metadata_sha='' rollback_text
  rollback_text="cold-cache-store.sh --restore --lane $lane --root $root with the same remote configuration and revision $revision"
  if (( evidence == 1 )); then
    rollback_text="cold-cache-store.sh --restore --root $root --run-host $run_host --run-id $run_id with the same remote configuration and revision $revision"
  fi
  # Tar's logical bytes can greatly exceed allocated blocks for sparse files.
  # Measure the stream itself before admission rather than reserving from du.
  archive_bytes="$(tar -C "$path" -cf - . | wc -c | tr -d ' ')"
  source_allocated_kib="$(du -sk "$path" | awk 'NR==1 {print $1}')"
  [[ "$archive_bytes" =~ ^[0-9]+$ && "$archive_bytes" -gt 0 ]] || {
    echo 'cold-cache-store: cannot measure archive bytes' >&2; return 1;
  }
  [[ "$source_allocated_kib" =~ ^[0-9]+$ ]] || {
    echo 'cold-cache-store: cannot measure source allocation' >&2; return 1;
  }
  # The remote lock is intentionally not stolen. A stale or unknown holder
  # needs an operator to inspect its pending archive before any new export.
  remote_lock_token="$(openssl rand -hex 16)"
  remote_ssh "bash -s -- $remote_dir $remote_lock_token" <<'REMOTE'
set -euo pipefail
umask 077
base=$1; token=$2
[[ ! -L "$base/aura-cold-cache" &&
   ( ! -e "$base/aura-cold-cache" || -d "$base/aura-cold-cache" ) ]]
mkdir -m 700 -p "$base/aura-cold-cache"
[[ "$(stat -c %a "$base/aura-cold-cache")" == 700 ]]
mkdir -m 700 "$base/aura-cold-cache/.capacity.lock"
printf '%s\n' "$token" > "$base/aura-cold-cache/.capacity.lock/token"
chmod 600 "$base/aura-cold-cache/.capacity.lock/token"
REMOTE
  remote_floor=$(( ${AURA_COLD_CACHE_REMOTE_MIN_FREE_GIB:-15} * 1024 * 1024 ))
  remote_free="$(remote_ssh "df -Pk $remote_dir | awk 'NR==2 {print \$4}'")"
  [[ "$remote_free" =~ ^[0-9]+$ ]] || {
    echo 'cold-cache-store: remote free-space witness invalid' >&2; return 1;
  }
  (( remote_free - (archive_bytes + 1023) / 1024 - 1024 * 1024 >= remote_floor )) || {
    echo 'cold-cache-store: remote archive would breach capacity floor' >&2; return 1;
  }
  [[ ! -e "$signing_key" || ( -f "$signing_key" && ! -L "$signing_key" &&
    "$(stat_owner "$signing_key")" == "$(id -u)" &&
    "$(stat_mode "$signing_key")" == 600 ) ]] || {
    echo 'cold-cache-store: invalid local signing key' >&2; return 1;
  }
  if [[ ! -f "$signing_key" ]]; then
    umask 077
    local temp_key
    temp_key="$(mktemp "$shared_dir/cold-keys/.key.XXXXXX")"
    openssl ecparam -name prime256v1 -genkey -noout -out "$temp_key" 2>/dev/null
    chmod 600 "$temp_key"
    mv "$temp_key" "$signing_key"
  fi
  # A first pass binds the original idle lane. Any change before the streamed
  # upload makes the remote hash differ and preserves the source.
  expected_sha="$(tar -C "$path" -cf - . | sha256sum | awk '{print $1}')"
  remote_ssh "bash -s -- $remote_dir $root_key $revision $lane" <<'REMOTE'
set -euo pipefail
umask 077
base=$1; key=$2; rev=$3; lane=$4
for dir in "$base/aura-cold-cache" "$base/aura-cold-cache/$key" \
  "$base/aura-cold-cache/$key/$rev" "$base/aura-cold-cache/$key/$rev/$lane"; do
  [[ ! -L "$dir" && ( ! -e "$dir" || -d "$dir" ) ]]
  mkdir -m 700 -p "$dir"
  [[ "$(stat -c %a "$dir")" == 700 ]]
done
bucket="$base/aura-cold-cache/$key/$rev/$lane"
[[ ! -e "$bucket/ready" && ! -L "$bucket/ready" &&
   ! -e "$bucket/.pending" && ! -L "$bucket/.pending" ]]
mkdir -m 700 "$bucket/.pending"
REMOTE
  tar -C "$path" -cf - . | remote_ssh "umask 077; cat > $remote_pending/archive.tar"
  read -r actual_sha remote_bytes < <(remote_ssh \
    "sha256sum $remote_pending/archive.tar; wc -c < $remote_pending/archive.tar" |
    awk 'NR==1 {sha=$1} NR==2 {bytes=$1} END {print sha, bytes}')
  [[ "$expected_sha" == "$actual_sha" && "$remote_bytes" == "$archive_bytes" ]] || {
    echo 'cold-cache-store: remote archive differs from original lane' >&2; return 1;
  }
  umask 077
  pending="$(mktemp -d "$shared_dir/cold-keys/.remote.XXXXXX")"
  if (( evidence == 1 )); then
    evidence_members "$path" > "$pending/members.tsv" || {
      echo 'cold-cache-store: evidence metadata inventory failed' >&2; return 1;
    }
    metadata_sha="$(sha256sum "$pending/members.tsv" | awk '{print $1}')"
    remote_ssh "umask 077; cat > $remote_pending/members.tsv" < "$pending/members.tsv"
  fi
  jq -n --arg revision "$revision" --arg root "$root" --arg lane "$lane" \
    --arg sha "$actual_sha" --argjson bytes "$remote_bytes" \
    --argjson allocated "$source_allocated_kib" --arg metadata "$metadata_sha" \
    --arg relative "${relative:-}" \
    '{schema:1,source_revision:$revision,source_root:$root,lane:$lane,
      sha256:$sha,bytes:$bytes,source_allocated_kib:$allocated} +
      (if $metadata=="" then {} else
        {metadata_sha256:$metadata,source_relative:$relative} end)' \
    > "$pending/manifest.json"
  openssl dgst -sha256 -sign "$signing_key" -out "$pending/manifest.sig" "$pending/manifest.json"
  remote_ssh "umask 077; cat > $remote_pending/manifest.json" < "$pending/manifest.json"
  remote_ssh "umask 077; cat > $remote_pending/manifest.sig" < "$pending/manifest.sig"
  mkdir -p "$bucket"
  chmod 700 "$store" "$store/aura-cold-cache" "$store/aura-cold-cache/$root_key" \
    "$store/aura-cold-cache/$root_key/$revision" "$bucket"
  [[ ! -e "$remote_location" && ! -L "$remote_location" &&
     ! -e "$remote_location_sig" && ! -L "$remote_location_sig" ]] || {
    echo 'cold-cache-store: existing location receipt refuses overwrite' >&2; return 1;
  }
  jq -n --arg revision "$revision" --arg root "$root" --arg lane "$lane" \
    --arg host "$remote_host" --arg dir "$remote_dir" --arg machine "$remote_machine" \
    --arg device "$remote_device" --arg sha "$actual_sha" --argjson bytes "$remote_bytes" \
    --argjson allocated "$source_allocated_kib" --arg metadata "$metadata_sha" \
    --arg relative "${relative:-}" --arg rollback "$rollback_text" \
    '{schema:1,source_revision:$revision,source_root:$root,lane:$lane,
      remote_host:$host,remote_dir:$dir,remote_machine:$machine,remote_device:$device,
      sha256:$sha,bytes:$bytes,source_allocated_kib:$allocated,
      rollback:$rollback} +
      (if $metadata=="" then {} else
        {metadata_sha256:$metadata,source_relative:$relative} end)' \
    > "$pending/location.json"
  openssl dgst -sha256 -sign "$signing_key" -out "$pending/location.sig" "$pending/location.json"
  remote_ssh "umask 077; cat > $remote_pending/location.json" < "$pending/location.json"
  remote_ssh "umask 077; cat > $remote_pending/location.sig" < "$pending/location.sig" || {
    echo 'cold-cache-store: remote location upload stopped before ready' >&2; return 1;
  }
  remote_ssh "bash -s -- $remote_pending $remote_ready $evidence" <<'REMOTE'
set -euo pipefail
pending=$1; ready=$2; evidence=$3
[[ -d "$pending" && ! -L "$pending" && ! -e "$ready" && ! -L "$ready" ]]
names=(archive.tar manifest.json manifest.sig location.json location.sig)
[[ "$evidence" == 0 ]] || names+=(members.tsv)
for name in "${names[@]}"; do
  file="$pending/$name"
  [[ -f "$file" && ! -L "$file" && "$(stat -c %a "$file")" == 600 &&
     "$(stat -c %u "$file")" == "$(id -u)" ]]
done
mv -T -- "$pending" "$ready"
REMOTE
  # The ready location is complete before it is visible; read it back before
  # publishing the local rollback receipt or removing source bytes.
  cmp "$pending/manifest.json" <(remote_ssh "cat $remote_ready/manifest.json")
  cmp "$pending/manifest.sig" <(remote_ssh "cat $remote_ready/manifest.sig")
  cmp "$pending/location.json" <(remote_ssh "cat $remote_ready/location.json")
  cmp "$pending/location.sig" <(remote_ssh "cat $remote_ready/location.sig")
  if (( evidence == 1 )); then
    cmp "$pending/members.tsv" <(remote_ssh "cat $remote_ready/members.tsv")
  fi
  remote_verify_ready
  [[ "$expected_sha" == "$(remote_ssh "cat $remote_ready/archive.tar" | sha256sum | awk '{print $1}')" ]] || {
    echo 'cold-cache-store: remote readback hash mismatch' >&2; return 1;
  }
  mv "$pending/location.json" "$remote_location"
  mv "$pending/location.sig" "$remote_location_sig"
  [[ "$(git -C "$root" rev-parse HEAD)" == "$revision" &&
     -z "$(git -C "$root" status --porcelain --untracked-files=normal)" &&
     "$expected_sha" == "$(tar -C "$path" -cf - . | sha256sum | awk '{print $1}')" ]] || {
    echo 'cold-cache-store: source changed before guarded prune' >&2; return 1;
  }
  if (( evidence == 1 )); then
    evidence_validate_source || { echo 'cold-cache-store: evidence changed before guarded prune' >&2; return 1; }
    cmp "$pending/members.tsv" <(evidence_members "$path") || {
      echo 'cold-cache-store: evidence metadata changed before guarded prune' >&2; return 1;
    }
    open_files="$(lsof -n -P 2>/dev/null)" || {
      echo 'cold-cache-store: final open-file scan failed' >&2; return 1;
    }
    if printf '%s\n' "$open_files" | grep -F -- "$path" >/dev/null; then
      echo 'cold-cache-store: evidence became open before guarded prune' >&2; return 1
    fi
    [[ -f "$remote_location" && -f "$remote_location_sig" ]] || return 1
    rm -rf -- "$path"
  else
    bash "$repo_root/scripts/dev/prune-inactive-lane.sh" --root "$root" --lane "$lane" \
      --lock-owned-by "$$" --apply
  fi
  printf 'cold-cache-store: remote export verified %s bytes at %s:%s\n' \
    "$remote_bytes" "$remote_host" "$remote_ready"
}
remote_restore() {
  [[ ! -e "$path" && ! -L "$path" ]] || { echo 'cold-cache-store: restore lane already exists' >&2; return 1; }
  if (( evidence == 1 )); then
    local ancestor active
    for ancestor in "$root/.tmp" "$root/.tmp/e2e" "$root/.tmp/e2e/run" \
      "$root/.tmp/e2e/run/$run_host" "$root/.tmp/e2e/run/$run_host/artifacts" \
      "$evidence_runs"; do
      [[ -d "$ancestor" && ! -L "$ancestor" ]] || {
        echo 'cold-cache-store: evidence restore parent absent or linked' >&2; return 1;
      }
    done
    active="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
    [[ -z "$active" ]] || { echo 'cold-cache-store: harness consumer active' >&2; return 1; }
  fi
  remote_probe
  remote_local_bucket_preflight
  [[ -f "$remote_location" && ! -L "$remote_location" &&
     -f "$remote_location_sig" && ! -L "$remote_location_sig" &&
     -f "$signing_key" && ! -L "$signing_key" ]] || {
    echo 'cold-cache-store: protected location receipt absent' >&2; return 1;
  }
  remote_verify_ready
  public_key="$(mktemp "$shared_dir/cold-keys/.pub.XXXXXX")"
  openssl pkey -in "$signing_key" -pubout -out "$public_key" 2>/dev/null
  [[ "$(stat_mode "$remote_location")" == 600 &&
     "$(stat_mode "$remote_location_sig")" == 600 ]] || {
    echo 'cold-cache-store: local location receipt permissions refused' >&2; return 1;
  }
  openssl dgst -sha256 -verify "$public_key" -signature "$remote_location_sig" \
    "$remote_location" >/dev/null || { echo 'cold-cache-store: location signature mismatch' >&2; return 1; }
  jq -e --arg rev "$revision" --arg root "$root" --arg lane "$lane" \
    --arg host "$remote_host" --arg dir "$remote_dir" --arg machine "$remote_machine" \
    --arg device "$remote_device" --argjson evidence "$evidence" \
    --arg relative "${relative:-}" \
    '.schema==1 and .source_revision==$rev and .source_root==$root and .lane==$lane and
     .remote_host==$host and .remote_dir==$dir and .remote_machine==$machine and
     .remote_device==$device and (.sha256|test("^[0-9a-f]{64}$")) and
     (.bytes|type=="number" and .>0 and .<1099511627776) and
     (.source_allocated_kib|type=="number" and .>=0 and .<1073741824) and
     (if $evidence==1 then
       (.metadata_sha256|test("^[0-9a-f]{64}$")) and .source_relative==$relative
      else (.metadata_sha256==null and .source_relative==null) end)' \
    "$remote_location" >/dev/null || { echo 'cold-cache-store: location identity mismatch' >&2; return 1; }
  local bytes expected allocated free floor others file pid kib archive_kib extract_kib metadata_sha
  local reserve_kib acquired holder attempt
  bytes="$(jq -r '.bytes' "$remote_location")"
  expected="$(jq -r '.sha256' "$remote_location")"
  allocated="$(jq -r '.source_allocated_kib' "$remote_location")"
  metadata_sha="$(jq -r '.metadata_sha256 // empty' "$remote_location")"
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
    echo 'cold-cache-store: admission lock busy' >&2; return 1;
  }
  printf '%s\n' "$$" > "$admission_lock/pid"
  free="$(df -Pk "$root" | awk 'NR==2 {print $4}')"
  floor=$(( ${AURA_BUILD_MIN_FREE_GIB:-15} * 1024 * 1024 ))
  others=0
  for file in "$shared_dir"/reservations/*; do
    [[ -f "$file" ]] || continue
    pid="${file##*/}"
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    if ! kill -0 "$pid" 2>/dev/null; then rm -f "$file"; continue; fi
    kib="$(awk 'NR==1 {print $1}' "$file")"
    [[ "$kib" =~ ^[0-9]+$ ]] && others=$((others + kib))
  done
  archive_kib=$(( (bytes + 1023) / 1024 ))
  extract_kib="$allocated"
  (( archive_kib > extract_kib )) && extract_kib="$archive_kib"
  # The archive download and extracted tree coexist until atomic publish.
  reserve_kib=$(( archive_kib + extract_kib + 1024 * 1024 ))
  (( free - others - reserve_kib >= floor )) || {
    echo 'cold-cache-store: remote restore would breach admission floor' >&2; return 1;
  }
  reservation="$shared_dir/reservations/$$"
  printf '%s\n' "$reserve_kib" > "$reservation"
  rm -f "$admission_lock/pid"
  rmdir "$admission_lock"
  admission_lock=''
  umask 077
  pending="$(mktemp -d "$shared_dir/cold-keys/.pull.XXXXXX")"
  remote_ssh "cat $remote_ready/location.json" > "$pending/location.json"
  remote_ssh "cat $remote_ready/location.sig" > "$pending/location.sig"
  cmp "$remote_location" "$pending/location.json"
  cmp "$remote_location_sig" "$pending/location.sig"
  remote_ssh "cat $remote_ready/manifest.json" > "$pending/manifest.json"
  remote_ssh "cat $remote_ready/manifest.sig" > "$pending/manifest.sig"
  openssl dgst -sha256 -verify "$public_key" -signature "$pending/manifest.sig" \
    "$pending/manifest.json" >/dev/null || { echo 'cold-cache-store: manifest signature mismatch' >&2; return 1; }
  jq -e --arg rev "$revision" --arg root "$root" --arg lane "$lane" \
    --arg sha "$expected" --argjson bytes "$bytes" --argjson allocated "$allocated" \
    --arg metadata "$metadata_sha" --arg relative "${relative:-}" \
    '.schema==1 and .source_revision==$rev and .source_root==$root and .lane==$lane and
     .sha256==$sha and .bytes==$bytes and .source_allocated_kib==$allocated and
     (.metadata_sha256 // "")==$metadata and
     (if $metadata=="" then .source_relative==null else .source_relative==$relative end)' \
    "$pending/manifest.json" >/dev/null || {
      echo 'cold-cache-store: remote manifest differs from location receipt' >&2; return 1;
    }
  remote_ssh "cat $remote_ready/archive.tar" > "$pending/archive.tar"
  [[ "$expected" == "$(sha256sum "$pending/archive.tar" | awk '{print $1}')" &&
     "$bytes" == "$(wc -c < "$pending/archive.tar" | tr -d ' ')" ]] || {
    echo 'cold-cache-store: remote archive hash or size mismatch' >&2; return 1;
  }
  if (( evidence == 1 )); then
    remote_ssh "cat $remote_ready/members.tsv" > "$pending/members.tsv"
    [[ "$metadata_sha" == "$(sha256sum "$pending/members.tsv" | awk '{print $1}')" ]] || {
      echo 'cold-cache-store: evidence metadata manifest mismatch' >&2; return 1;
    }
  fi
  member_list="$(mktemp "$shared_dir/cold-keys/.members.XXXXXX")"
  kind_list="$(mktemp "$shared_dir/cold-keys/.kinds.XXXXXX")"
  tar -tf "$pending/archive.tar" > "$member_list"
  tar -tvf "$pending/archive.tar" | cut -c1 > "$kind_list"
  while IFS= read -r member; do
    [[ "$member" == . || "$member" == ./* ]] && [[ ! "$member" =~ (^|/)\.\.(/|$) ]] || {
      echo 'cold-cache-store: unsafe remote archive member' >&2; return 1;
    }
  done < "$member_list"
  while IFS= read -r kind; do
    [[ "$kind" == - || "$kind" == d ]] || {
      echo 'cold-cache-store: linked remote archive member' >&2; return 1;
    }
  done < "$kind_list"
  restore_stage="$(mktemp -d "$root/target/.cold-restore.XXXXXX")"
  chmod 700 "$restore_stage"
  mkdir -m 700 "$restore_stage/payload"
  tar -C "$restore_stage/payload" -pxf "$pending/archive.tar" || {
    echo 'cold-cache-store: staged extraction failed' >&2; return 1;
  }
  if (( evidence == 1 )); then
    evidence_validate_payload "$restore_stage/payload" 0 || {
      echo 'cold-cache-store: restored evidence metadata or permissions refused' >&2; return 1;
    }
    # Extraction writes children after creating '.', so restore its signed
    # directory mtime before comparing the full metadata inventory.
    local root_epoch root_stamp
    root_epoch="$(awk -F '\t' 'NR==1 {print $4}' "$pending/members.tsv")"
    [[ "$root_epoch" =~ ^[0-9]+$ ]] || return 1
    root_stamp="$(date -u -r "$root_epoch" +%Y%m%d%H%M.%S 2>/dev/null ||
      date -u -d "@$root_epoch" +%Y%m%d%H%M.%S)" || return 1
    TZ=UTC touch -t "$root_stamp" "$restore_stage/payload"
    cmp "$pending/members.tsv" <(evidence_members "$restore_stage/payload") || {
      echo 'cold-cache-store: restored evidence metadata differs from signed inventory' >&2; return 1;
    }
  fi
  [[ ! -e "$path" && ! -L "$path" ]] || {
    echo 'cold-cache-store: restore target appeared before publish' >&2; return 1;
  }
  mkdir -p "${path%/*}"
  mv "$restore_stage/payload" "$path"
  rmdir "$restore_stage"
  restore_stage=''
  printf 'cold-cache-store: remote restored %s:%s\n' "$remote_host" "$remote_ready"
}
if (( remote_mode == 1 )); then
  if [[ "$action" == export ]]; then remote_export; else remote_restore; fi
  exit
fi
if [[ "$action" == export ]]; then
  [[ -d "$path" && ! -L "$path" ]] || { echo 'cold-cache-store: lane absent or linked' >&2; exit 1; }
  [[ -z "$(find "$path" -type l -print -quit)" ]] || { echo 'cold-cache-store: linked artifact' >&2; exit 1; }
  [[ -z "$(find "$path" -type f -links +1 -print -quit)" ]] || { echo 'cold-cache-store: hard-linked artifact' >&2; exit 1; }
  [[ -z "$(find "$path" ! -type d ! -type f -print -quit)" ]] || {
    echo 'cold-cache-store: unsupported source member type' >&2; exit 1;
  }
  while IFS= read -r -d '' member; do
    [[ "$member" != *$'\n'* && "$member" != *$'\r'* ]] || {
      echo 'cold-cache-store: source member has a control newline' >&2; exit 1;
    }
  done < <(find "$path" -print0)
  [[ ! -e "$signing_key" || ( -f "$signing_key" && ! -L "$signing_key" &&
    "$(stat_owner "$signing_key")" == "$(id -u)" &&
    "$(stat_mode "$signing_key")" == 600 ) ]] || {
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
  if ! tar -C "$path" -pxf "$snapshot/archive.tar"; then
    rm -rf -- "$path"
    exit 1
  fi
  printf 'cold-cache-store: restored %s\n' "$snapshot"
fi
