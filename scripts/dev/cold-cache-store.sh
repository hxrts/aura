#!/usr/bin/env bash
# Optional cold store for one inactive compiler cache lane. Local destinations
# must have independent capacity; the remote backend requires SSH to a local
# ext4 volume on a separate Linux host.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
action='' lane='' root="$repo_root" lock_owner='' run_host='' run_id='' evidence=0 native=0 native_run='' native_binding_file='' native_binding_blake3=''
compression=plain
while [[ $# -gt 0 ]]; do
  case "$1" in
    --export|--restore) action="${1#--}"; shift ;;
    --root) root="${2:?missing root}"; shift 2 ;;
    --lane) lane="${2:?missing lane}"; shift 2 ;;
    --native-binding) native_binding_file="${2:?missing native expectation file}"; shift 2 ;;
    --native-binding-blake3) native_binding_blake3="${2:?missing held native expectation BLAKE3}"; shift 2 ;;
    --native-run) native_run="${2:?missing native run}"; shift 2 ;;
    --run-host) run_host="${2:?missing run host}"; shift 2 ;;
    --run-id) run_id="${2:?missing run id}"; shift 2 ;;
    --lock-owned-by) lock_owner="${2:?missing owner}"; shift 2 ;;
    --compression) compression="${2:?missing compression}"; shift 2 ;;
    *) echo 'usage: cold-cache-store.sh --export|--restore (--lane LANE | --native-run run.ID | --run-host HOST --run-id ID) [--root PATH] [--lock-owned-by PID]' >&2; exit 2 ;;
  esac
done
if [[ -n "$native_run" ]]; then
  [[ -z "$lane" && -z "$run_host" && -z "$run_id" && "$native_run" =~ ^run\.[A-Za-z0-9]+$ &&
     "$native_binding_file" == /* && "$native_binding_blake3" =~ ^[0-9a-f]{64}$ ]] || {
    echo 'cold-cache-store: invalid completed native run identity' >&2; exit 2;
  }
  evidence=1 native=1
  lane="completed-native-$native_run"
  relative="artifacts/disk-budget/receipts/$native_run"
elif [[ -n "$native_binding_file" || -n "$native_binding_blake3" ]]; then
  echo 'cold-cache-store: native expectation requires --native-run' >&2; exit 2
elif [[ -n "$run_host" || -n "$run_id" ]]; then
  [[ -z "$lane" && "$run_host" =~ ^[A-Za-z0-9_-]+$ &&
     "$run_id" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$run_id" != *..* ]] || {
    echo 'cold-cache-store: invalid evidence run identity' >&2; exit 2;
  }
  evidence=1
  lane="evidence-h${#run_host}-$run_host-r${#run_id}-$run_id"
  relative=".tmp/e2e/run/$run_host/artifacts/runs/$run_id"
else
case "$lane" in
  debug) relative=target/debug ;;
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
[[ "$compression" == plain || "$compression" == gzip-1 ]] || {
  echo 'cold-cache-store: unsupported compression' >&2; exit 2;
}
[[ "$action" == export || "$compression" == plain ]] || {
  echo 'cold-cache-store: restore selects its signed archive format' >&2; exit 2;
}
[[ "$evidence" == 0 || "$compression" == plain ]] || {
  echo 'cold-cache-store: evidence compression is not supported' >&2; exit 2;
}
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
[[ "$native" == 1 || -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ]] || {
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
evidence_members() (
  local base="$1" member relative kind bytes digest mode mtime list
  list="$(mktemp "$shared_dir/cold-keys/.inventory.XXXXXX")" || return 1
  trap 'rm -f -- "$list"' EXIT
  find "$base" -print0 | LC_ALL=C sort -z > "$list" || return 1
  while IFS= read -r -d '' member; do
    relative="${member#"$base"}"
    [[ "$relative" != *$'\n'* && "$relative" != *$'\r'* &&
       "$relative" != *$'\t'* ]] || return 1
    if [[ -d "$member" ]]; then kind=d; bytes=0; digest=-
    elif [[ -f "$member" && ! -L "$member" ]]; then
      kind=f; bytes="$(stat_size "$member")" || return 1
      digest="$(sha256sum "$member" | awk '{print $1}')" || return 1
      [[ "$digest" =~ ^[0-9a-f]{64}$ ]] || return 1
    else return 1; fi
    mode="$(stat_mode "$member")" || return 1
    mtime="$(stat_mtime "$member")" || return 1
    [[ "$mode" =~ ^[0-7]{3,4}$ && "$mtime" =~ ^[0-9]+$ ]] || return 1
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$relative" "$kind" \
      "$mode" "$mtime" "$bytes" "$digest" || return 1
  done < "$list"
)
gzip_tool_identity() {
  local selected
  selected="$(command -v gzip 2>/dev/null || true)"
  [[ "$selected" == /nix/store/*/bin/gzip && -f "$selected" && ! -L "$selected" ]] || {
    echo 'cold-cache-store: pinned Nix gzip required' >&2; return 1;
  }
  gzip_bin="$selected"
  gzip_sha="$(sha256sum "$gzip_bin" | awk '{print $1}')"
  [[ "$gzip_sha" =~ ^[0-9a-f]{64}$ ]] || return 1
}
archive_members() {
  evidence_members "$1"
}
# Native receipt fields are consumed as completed original evidence, never
# rewritten into a new completion or converted into a LAN retention manifest.
native_binding_json=''
if (( native == 1 )); then
  [[ -f "$native_binding_file" && ! -L "$native_binding_file" &&
     "$(stat_mode "$native_binding_file")" == 600 &&
     "$(stat_owner "$native_binding_file")" == "$(id -u)" ]] || {
    echo 'cold-cache-store: native expectation file refused' >&2; exit 78;
  }
  command -v b3sum >/dev/null || { echo 'cold-cache-store: native retention requires b3sum' >&2; exit 2; }
  actual=$(b3sum "$native_binding_file") || exit "$?"
  [[ ${actual%% *} == "$native_binding_blake3" ]] || {
    echo 'cold-cache-store: native expectation file differs' >&2; exit 78;
  }
  native_binding_json=$(jq -ecS --arg receipt "$path/receipt.json" '
    select(.receipt==$receipt and (.source|type=="object") and (.config|type=="object") and
      (.receipt_blake3|test("^[0-9a-f]{64}$")) and
      (.completion_blake3|test("^[0-9a-f]{64}$")) and
      (.inventory_blake3|test("^[0-9a-f]{64}$")) and
      (.binary_blake3|test("^[0-9a-f]{64}$")))' "$native_binding_file") || {
    echo 'cold-cache-store: native expectation identity refused' >&2; exit 78;
  }
fi
native_validate_payload() {
  local candidate=$1 receipt="$1/receipt.json" completion="$1/completion.json"
  local source_fp protected producer expected actual member mode owner list receipt_digest completion_digest binding
  local pid identity observed relative_copy copy_dir inventory inventory_name inventory_digest
  command -v b3sum >/dev/null || return 1
  actual=$(b3sum "$native_binding_file") || return "$?"
  [[ ${actual%% *} == "$native_binding_blake3" ]] || {
    echo 'cold-cache-store: native expectation file differs' >&2; return 78;
  }
  [[ -d "$candidate" && ! -L "$candidate" && -f "$receipt" && ! -L "$receipt" &&
     -f "$completion" && ! -L "$completion" ]] || return 1
  receipt_digest=$(b3sum "$receipt") || return "$?"
  receipt_digest=${receipt_digest%% *}
  completion_digest=$(b3sum "$completion") || return "$?"
  completion_digest=${completion_digest%% *}
  [[ "$receipt_digest" == "$(jq -r '.receipt_blake3' <<< "$native_binding_json")" &&
     "$completion_digest" == "$(jq -r '.completion_blake3' <<< "$native_binding_json")" ]] || {
    echo 'cold-cache-store: native expected identity differs' >&2; return 78;
  }
  # This is the selected canonical source, not reconstructed authority from IDs.
  source_fp=$(aura_receipt_source_fingerprint "$root") || return "$?"
  jq -e --arg root "$root" --arg rev "$revision" --arg fp "$source_fp" \
    --arg receipt "$path/receipt.json" --arg notification "$path/completion.json" \
    --slurpfile done "$completion" '
    def status: type=="number" and floor==. and .>=0 and .<=255;
    .schema==2 and .root==$root and .source.revision==$rev and
    .source.fingerprint==$fp and .notification==$notification and
    .admission.admitted==true and .current_phase=="finished" and
    .active_phase==null and (.finished_utc|type=="string" and length>0) and
    (.command_exit_status|status) and (.final_exit_status|status) and
    .command_exit_status==.final_exit_status and
    .command_argv[0]=="bash" and .command_argv[1]==($root+"/scripts/dev/run-exact-fixtures.sh") and
    .command_argv[2]=="--inside" and any(.command_argv[]; .=="--replay-first-failure") and
    .metadata.replay_first_failure==true and
    (.metadata.expected_count|type=="number" and floor==. and .>0) and
    (.metadata.packages|type=="string" and length>0) and
    (.metadata.test_target|type=="string" and length>0) and
    ([.phases[]?|select(.phase=="compile")]|length==1) and
    all(.phases[]?|select(.phase=="compile");
      .exit_status==0 and .command_exit_status==0) and
    ($done|length==1) and $done[0].receipt==$receipt and
    $done[0].exit_status==.final_exit_status and
    $done[0].finished_utc==.finished_utc ' "$receipt" >/dev/null || return 1
  # Bind each event to the enclosing original status, including native failure.
  jq -e '.final_exit_status as $status | .source.fingerprint as $fp |
    any(.events[]?; .kind=="completion" and .source_fingerprint==$fp and
      .exit_status==$status)' "$receipt" >/dev/null || return 1
  protected=$(jq -er '.artifacts.protected_test_executable' "$receipt") || return "$?"
  producer=$(jq -er '.artifacts.test_executable' "$receipt") || return "$?"
  [[ "$protected" == "$path"/test-binary.*/* && "$producer" == "$root"/target/debug/deps/* &&
     "$protected" != *'/../'* && "$producer" != *'/../'* &&
     "$protected" != *$'\n'* && "$producer" != *$'\n'* ]] || return 1
  actual=${producer#"$root"/target/debug/deps/}
  [[ "$actual" =~ ^[A-Za-z0-9_.-]+$ && "${protected##*/}" == "$actual" ]] || return 1
  relative_copy=${protected#"$path"/}
  copy_dir=${relative_copy%/*}
  [[ "$copy_dir" =~ ^test-binary\.[A-Za-z0-9]+$ &&
     ${relative_copy##*/} =~ ^[A-Za-z0-9_.-]+$ ]] || return 1
  expected=$(jq -er '.artifacts.protected_test_executable_blake3' "$receipt") || return "$?"
  [[ "$expected" == *"  $protected" ]] || return 1
  expected=${expected%% *}
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || return 1
  actual=$(jq -er '.metadata.producer_binary_blake3' "$receipt") || return "$?"
  [[ "$actual" == "$expected  $producer" ]] || return 1
inventory=$(jq -er '.artifacts.expected_inventory' "$receipt") || return "$?"
[[ "$inventory" == "$path"/inventory.* ]] || return 1
inventory_name=${inventory#"$path"/}
[[ "$inventory_name" =~ ^inventory\.[A-Za-z0-9]+$ &&
   -f "$candidate/$inventory_name" && ! -L "$candidate/$inventory_name" ]] || return 1
jq -e --rawfile inventory "$candidate/$inventory_name" '
  (.metadata.test_inventory+"\n")==$inventory and
  ($inventory|split("\n")|.[0:-1]) as $names |
  ($names|length)==.metadata.expected_count and
  ($names|unique|length)==.metadata.expected_count and
  all($names[]; test("^[A-Za-z0-9_:]+$"))' "$receipt" >/dev/null || return 1
inventory_digest=$(b3sum "$candidate/$inventory_name") || return "$?"
inventory_digest=${inventory_digest%% *}
  # Validate the relocated staged copy, preserving original receipt paths.
  protected="$candidate/$relative_copy"
  [[ -f "$protected" && ! -L "$protected" && -x "$protected" ]] || return 1
  actual=$(b3sum "$protected") || return "$?"
  [[ "$expected" == "$(jq -r '.binary_blake3' <<< "$native_binding_json")" && ${actual%% *} == "$expected" ]] || {
    echo 'cold-cache-store: native expected artifact differs' >&2; return 78;
  }
  list=$(mktemp "$shared_dir/cold-keys/.native-members.XXXXXX") || return "$?"
  find "$candidate" -print0 > "$list" || { rm -f "$list"; return 1; }
  while IFS= read -r -d '' member; do
    [[ ! -L "$member" && ( -f "$member" || -d "$member" ) ]] || { rm -f "$list"; return 1; }
    mode=$(stat_mode "$member") || { rm -f "$list"; return 1; }
    owner=$(stat_owner "$member") || { rm -f "$list"; return 1; }
    [[ $owner == "$(id -u)" ]] || { rm -f "$list"; return 1; }
    if [[ -d "$member" ]]; then
      [[ $mode == 700 ]] || { rm -f "$list"; return 1; }
    elif [[ "$member" == "$protected" ]]; then
      [[ $mode == 500 ]] || { rm -f "$list"; return 1; }
    else
      [[ $mode == 600 ]] || { rm -f "$list"; return 1; }
    fi
  done < "$list"
  rm -f "$list" || return "$?"
  actual=$(find "$candidate" -type f -links +1 -print -quit) || return "$?"
  [[ -z $actual ]] || return 1
  actual=$(b3sum "$receipt") || return "$?"
  [[ ${actual%% *} == "$receipt_digest" ]] || return 1
  actual=$(b3sum "$completion") || return "$?"
  [[ ${actual%% *} == "$completion_digest" ]] || return 1
  binding=$(jq -cS --arg receipt "$path/receipt.json" --arg r "$receipt_digest" \
    --arg c "$completion_digest" --arg binary "$expected" --arg inventory "$inventory_digest" \
    '{receipt:$receipt,source:.source,config:(.metadata|{packages,features,test_target,test_filter,test_inventory,expected_count}),
      receipt_blake3:$r,completion_blake3:$c,binary_blake3:$binary,inventory_blake3:$inventory,
      protected_executable:.artifacts.protected_test_executable}' "$receipt") || return "$?"
  [[ "$binding" == "$native_binding_json" ]] || {
    echo 'cold-cache-store: native expected binding differs' >&2; return 78;
  }
  # Terminal files alone cannot authorize moving a still-live original owner.
  for member in wrapper child; do
    pid=$(jq -er --arg key "$member" '.[$key].pid|select(type=="number" and .>0 and floor==.)' "$receipt") || return 1
    identity=$(jq -er --arg key "$member" '.[$key].start_identity|select(type=="string" and length>0)' "$receipt") || return 1
    if kill -0 "$pid" 2>/dev/null; then
      observed=$(ps -p "$pid" -o lstart=) || return 1
      [[ "$observed" != "$identity" ]] || return 1
    fi
  done
  actual=$(b3sum "$receipt") || return "$?"
  [[ ${actual%% *} == "$receipt_digest" ]] || return 1
  actual=$(b3sum "$completion") || return "$?"
  [[ ${actual%% *} == "$completion_digest" ]] || return 1
  actual=$(b3sum "$protected") || return "$?"
  [[ ${actual%% *} == "$expected" ]] || return 1
}

evidence_validate_payload() {
  if (( native == 1 )); then native_validate_payload "$1"; return "$?"; fi
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
  if (( native == 1 )); then
    for ancestor in "$root/artifacts" "$root/artifacts/disk-budget" "$root/artifacts/disk-budget/receipts"; do
      [[ -d "$ancestor" && ! -L "$ancestor" ]] || return 1
    done
    native_validate_payload "$path"; return "$?"
  fi
  for ancestor in "$root/.tmp" "$root/.tmp/e2e" "$root/.tmp/e2e/run" \
    "$root/.tmp/e2e/run/$run_host" "$root/.tmp/e2e/run/$run_host/artifacts" \
    "$evidence_runs"; do
    [[ -d "$ancestor" && ! -L "$ancestor" ]] || return 1
  done
  active="$(ps -axo pid=,comm= | awk '{n=$2;sub(/^.*\//,"",n);if(n~/^(tool_repl|aura-harness|aura)$/)printf "%s(%s) ",n,$1}')"
  [[ -z "$active" ]] || { echo 'cold-cache-store: harness consumer active' >&2; return 1; }
  evidence_validate_payload "$path" 1
}
if (( native == 1 )); then source "$repo_root/scripts/dev/build-receipt.sh"; fi
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
  if (( native == 1 )); then evidence_runs="$root/artifacts/disk-budget/receipts"; fi
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
archive_name=archive.tar
gzip_bin='' gzip_sha=''
if [[ "$compression" == gzip-1 ]]; then
  (( remote_mode == 1 )) || {
    echo 'cold-cache-store: compressed archive requires remote ext4' >&2; exit 2;
  }
  archive_name=archive.tar.gz
  gzip_tool_identity
fi
remote_location="$bucket/location.json"
remote_location_sig="$bucket/location.sig"
remote_verify_ready() {
  remote_ssh "bash -s -- $remote_ready $evidence $archive_name $compression" <<'REMOTE'
set -euo pipefail
ready=$1; evidence=$2; archive_name=$3; compression=$4
[[ -d "$ready" && ! -L "$ready" && "$(stat -c %a "$ready")" == 700 ]]
[[ "$archive_name" == archive.tar || "$archive_name" == archive.tar.gz ]]
names=("$archive_name" manifest.json manifest.sig location.json location.sig)
[[ "$evidence" == 0 ]] || names+=(members.tsv)
[[ "$compression" == plain ]] || names+=(members.tsv)
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
    local source_status=0
    evidence_validate_source || source_status=$?
    if (( source_status != 0 )); then
      echo 'cold-cache-store: evidence source not safely inactive' >&2
      if (( native == 1 )); then return "$source_status"; else return 1; fi
    fi
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
  local tar_bytes='' tar_sha=''
  rollback_text="cold-cache-store.sh --restore --lane $lane --root $root with the same remote configuration and revision $revision"
  if (( evidence == 1 )); then
    if (( native == 1 )); then
      rollback_text="cold-cache-store.sh --restore --root $root --native-run $native_run --native-binding $native_binding_file --native-binding-blake3 $native_binding_blake3 with the same remote configuration and original source"
    else
      rollback_text="cold-cache-store.sh --restore --root $root --run-host $run_host --run-id $run_id with the same remote configuration and revision $revision"
    fi
  fi
  # Tar's logical bytes can greatly exceed allocated blocks for sparse files.
  # Measure the stream itself before admission rather than reserving from du.
  if [[ "$compression" == gzip-1 ]]; then
    tar_bytes="$(tar -C "$path" -cf - . | wc -c | tr -d ' ')"
    tar_sha="$(tar -C "$path" -cf - . | sha256sum | awk '{print $1}')"
    archive_bytes="$(tar -C "$path" -cf - . | "$gzip_bin" -1 -c | wc -c | tr -d ' ')"
  else
    archive_bytes="$(tar -C "$path" -cf - . | wc -c | tr -d ' ')"
  fi
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
  if [[ "$compression" == gzip-1 ]]; then
    expected_sha="$(tar -C "$path" -cf - . | "$gzip_bin" -1 -c | sha256sum | awk '{print $1}')"
  else
    expected_sha="$(tar -C "$path" -cf - . | sha256sum | awk '{print $1}')"
  fi
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
  if [[ "$compression" == gzip-1 ]]; then
    tar -C "$path" -cf - . | "$gzip_bin" -1 -c |
      remote_ssh "umask 077; cat > $remote_pending/$archive_name"
  else
    tar -C "$path" -cf - . | remote_ssh "umask 077; cat > $remote_pending/$archive_name"
  fi
  read -r actual_sha remote_bytes < <(remote_ssh \
    "sha256sum $remote_pending/$archive_name; wc -c < $remote_pending/$archive_name" |
    awk 'NR==1 {sha=$1} NR==2 {bytes=$1} END {print sha, bytes}')
  [[ "$expected_sha" == "$actual_sha" && "$remote_bytes" == "$archive_bytes" ]] || {
    echo 'cold-cache-store: remote archive differs from original lane' >&2; return 1;
  }
  if (( native == 1 )); then
    local native_status=0
    native_validate_payload "$path" || native_status=$?
    if (( native_status != 0 )); then
      echo 'cold-cache-store: native artifact changed before signing' >&2; return "$native_status";
    fi
  fi
  umask 077
  pending="$(mktemp -d "$shared_dir/cold-keys/.remote.XXXXXX")"
  if (( evidence == 1 )) || [[ "$compression" == gzip-1 ]]; then
    archive_members "$path" > "$pending/members.tsv" || {
      echo 'cold-cache-store: source member inventory failed' >&2; return 1;
    }
    metadata_sha="$(sha256sum "$pending/members.tsv" | awk '{print $1}')"
    remote_ssh "umask 077; cat > $remote_pending/members.tsv" < "$pending/members.tsv"
  fi
  jq -n --arg revision "$revision" --arg root "$root" --arg lane "$lane" \
    --arg sha "$actual_sha" --argjson bytes "$remote_bytes" \
    --argjson allocated "$source_allocated_kib" --arg metadata "$metadata_sha" \
    --argjson native_binding "${native_binding_json:-null}" --arg relative "${relative:-}" --arg compression "$compression" \
    --arg tar_sha "$tar_sha" --argjson tar_bytes "${tar_bytes:-0}" \
    --arg gzip_tool "$gzip_bin" --arg gzip_sha "$gzip_sha" \
    '{schema:(if $compression=="plain" then 1 else 2 end),source_revision:$revision,source_root:$root,lane:$lane,
      sha256:$sha,bytes:$bytes,source_allocated_kib:$allocated} +
      (if $native_binding==null then {} else {native_artifact:$native_binding} end) +
      (if $compression=="plain" then {} else
        {compression:$compression,tar_sha256:$tar_sha,tar_bytes:$tar_bytes,
         gzip_tool:$gzip_tool,gzip_sha256:$gzip_sha} end) +
      (if $metadata=="" then {} else
        {metadata_sha256:$metadata} +
        (if $compression=="plain" then {source_relative:$relative} else {} end) end)' \
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
    --argjson native_binding "${native_binding_json:-null}" --arg relative "${relative:-}" --arg rollback "$rollback_text" \
    --arg compression "$compression" --arg tar_sha "$tar_sha" \
    --argjson tar_bytes "${tar_bytes:-0}" --arg gzip_tool "$gzip_bin" \
    --arg gzip_sha "$gzip_sha" \
    '{schema:(if $compression=="plain" then 1 else 2 end),source_revision:$revision,source_root:$root,lane:$lane,
      remote_host:$host,remote_dir:$dir,remote_machine:$machine,remote_device:$device,
      sha256:$sha,bytes:$bytes,source_allocated_kib:$allocated,
      rollback:$rollback} +
      (if $native_binding==null then {} else {native_artifact:$native_binding} end) +
      (if $compression=="plain" then {} else
        {compression:$compression,tar_sha256:$tar_sha,tar_bytes:$tar_bytes,
         gzip_tool:$gzip_tool,gzip_sha256:$gzip_sha} end) +
      (if $metadata=="" then {} else
        {metadata_sha256:$metadata} +
        (if $compression=="plain" then {source_relative:$relative} else {} end) end)' \
    > "$pending/location.json"
  openssl dgst -sha256 -sign "$signing_key" -out "$pending/location.sig" "$pending/location.json"
  remote_ssh "umask 077; cat > $remote_pending/location.json" < "$pending/location.json"
  remote_ssh "umask 077; cat > $remote_pending/location.sig" < "$pending/location.sig" || {
    echo 'cold-cache-store: remote location upload stopped before ready' >&2; return 1;
  }
  remote_ssh "bash -s -- $remote_pending $remote_ready $evidence $archive_name $compression" <<'REMOTE'
set -euo pipefail
pending=$1; ready=$2; evidence=$3; archive_name=$4; compression=$5
[[ -d "$pending" && ! -L "$pending" && ! -e "$ready" && ! -L "$ready" ]]
[[ "$archive_name" == archive.tar || "$archive_name" == archive.tar.gz ]]
names=("$archive_name" manifest.json manifest.sig location.json location.sig)
[[ "$evidence" == 0 ]] || names+=(members.tsv)
[[ "$compression" == plain ]] || names+=(members.tsv)
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
  if (( evidence == 1 )) || [[ "$compression" == gzip-1 ]]; then
    cmp "$pending/members.tsv" <(remote_ssh "cat $remote_ready/members.tsv")
  fi
  remote_verify_ready
  [[ "$expected_sha" == "$(remote_ssh "cat $remote_ready/$archive_name" | sha256sum | awk '{print $1}')" ]] || {
    echo 'cold-cache-store: remote readback hash mismatch' >&2; return 1;
  }
  if [[ "$compression" == gzip-1 ]]; then
    local readback_tar_sha
    readback_tar_sha="$(remote_ssh "cat $remote_ready/$archive_name" | "$gzip_bin" -dc | sha256sum | awk '{print $1}')" || {
      echo 'cold-cache-store: remote compressed readback failed' >&2; return 1;
    }
    [[ "$tar_sha" == "$readback_tar_sha" ]] || {
      echo 'cold-cache-store: remote compressed payload differs from signed tar' >&2; return 1;
    }
  fi
  mv "$pending/location.json" "$remote_location"
  mv "$pending/location.sig" "$remote_location_sig"
  local final_sha
  if [[ "$compression" == gzip-1 ]]; then
    final_sha="$(tar -C "$path" -cf - . | "$gzip_bin" -1 -c | sha256sum | awk '{print $1}')"
  else
    final_sha="$(tar -C "$path" -cf - . | sha256sum | awk '{print $1}')"
  fi
  [[ "$(git -C "$root" rev-parse HEAD)" == "$revision" &&
     ( "$native" == 1 || -z "$(git -C "$root" status --porcelain --untracked-files=normal)" ) &&
     "$expected_sha" == "$final_sha" ]] || {
    echo 'cold-cache-store: source changed before guarded prune' >&2; return 1;
  }
  if (( evidence == 1 )) || [[ "$compression" == gzip-1 ]]; then
    if (( evidence == 1 )); then
      local source_status=0
      evidence_validate_source || source_status=$?
      if (( source_status != 0 )); then
        echo 'cold-cache-store: evidence changed before guarded prune' >&2
        if (( native == 1 )); then return "$source_status"; else return 1; fi
      fi
    fi
    archive_members "$path" > "$pending/final-members.tsv" || {
      echo 'cold-cache-store: final source inventory failed' >&2; return 1;
    }
    cmp "$pending/members.tsv" "$pending/final-members.tsv" || {
      echo 'cold-cache-store: source metadata changed before guarded prune' >&2; return 1;
    }
    open_files="$(lsof -n -P 2>/dev/null)" || {
      echo 'cold-cache-store: final open-file scan failed' >&2; return 1;
    }
    if printf '%s\n' "$open_files" | grep -F -- "$path" >/dev/null; then
      echo 'cold-cache-store: evidence became open before guarded prune' >&2; return 1
    fi
  fi
  if (( evidence == 1 )); then
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
    if (( native == 1 )); then
      for ancestor in "$root/artifacts" "$root/artifacts/disk-budget" "$root/artifacts/disk-budget/receipts"; do
        [[ -d "$ancestor" && ! -L "$ancestor" ]] || return 1
      done
    else
    for ancestor in "$root/.tmp" "$root/.tmp/e2e" "$root/.tmp/e2e/run" \
      "$root/.tmp/e2e/run/$run_host" "$root/.tmp/e2e/run/$run_host/artifacts" \
      "$evidence_runs"; do
      [[ -d "$ancestor" && ! -L "$ancestor" ]] || {
        echo 'cold-cache-store: evidence restore parent absent or linked' >&2; return 1;
      }
    done
    fi
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
  public_key="$(mktemp "$shared_dir/cold-keys/.pub.XXXXXX")"
  openssl pkey -in "$signing_key" -pubout -out "$public_key" 2>/dev/null
  [[ "$(stat_mode "$remote_location")" == 600 &&
     "$(stat_mode "$remote_location_sig")" == 600 ]] || {
    echo 'cold-cache-store: local location receipt permissions refused' >&2; return 1;
  }
  openssl dgst -sha256 -verify "$public_key" -signature "$remote_location_sig" \
    "$remote_location" >/dev/null || { echo 'cold-cache-store: location signature mismatch' >&2; return 1; }
  local signed_schema signed_compression tar_bytes tar_sha signed_gzip_tool signed_gzip_sha
  signed_schema="$(jq -r '.schema' "$remote_location")"
  case "$signed_schema" in
    1) compression=plain; archive_name=archive.tar ;;
    2)
      compression="$(jq -r '.compression' "$remote_location")"
      [[ "$compression" == gzip-1 && "$evidence" == 0 ]] || {
        echo 'cold-cache-store: unsupported signed compressed format' >&2; return 1;
      }
      archive_name=archive.tar.gz
      gzip_tool_identity
      signed_gzip_tool="$(jq -r '.gzip_tool' "$remote_location")"
      signed_gzip_sha="$(jq -r '.gzip_sha256' "$remote_location")"
      [[ "$gzip_bin" == "$signed_gzip_tool" && "$gzip_sha" == "$signed_gzip_sha" ]] || {
        echo 'cold-cache-store: pinned gzip identity changed' >&2; return 1;
      }
      ;;
    *) echo 'cold-cache-store: unsupported signed archive schema' >&2; return 1 ;;
  esac
  remote_verify_ready
  jq -e --arg rev "$revision" --arg root "$root" --arg lane "$lane" \
    --arg host "$remote_host" --arg dir "$remote_dir" --arg machine "$remote_machine" \
    --arg device "$remote_device" --argjson evidence "$evidence" \
    --arg relative "${relative:-}" \
    '(.schema==1 or .schema==2) and .source_revision==$rev and .source_root==$root and .lane==$lane and
     .remote_host==$host and .remote_dir==$dir and .remote_machine==$machine and
     .remote_device==$device and (.sha256|test("^[0-9a-f]{64}$")) and
     (.bytes|type=="number" and .>0 and .<1099511627776) and
     (.source_allocated_kib|type=="number" and .>=0 and .<1073741824) and
     (if .schema==2 then
       .compression=="gzip-1" and
       (.tar_sha256|test("^[0-9a-f]{64}$")) and
       (.tar_bytes|type=="number" and .>0 and .<68719476736) and
       (.gzip_tool|type=="string" and startswith("/nix/store/")) and
       (.gzip_sha256|test("^[0-9a-f]{64}$")) and
       (.metadata_sha256|test("^[0-9a-f]{64}$"))
      else true end) and
     (if $evidence==1 then
       (.metadata_sha256|test("^[0-9a-f]{64}$")) and .source_relative==$relative
      else (if .schema==2 then .source_relative==null
        else (.metadata_sha256==null and .source_relative==null) end) end)' \
    "$remote_location" >/dev/null || { echo 'cold-cache-store: location identity mismatch' >&2; return 1; }
  if (( native == 1 )); then
    local signed_native_binding
    signed_native_binding=$(jq -ecS --arg receipt "$path/receipt.json" '
      .native_artifact | select(.receipt==$receipt and
        (.receipt_blake3|test("^[0-9a-f]{64}$")) and
        (.completion_blake3|test("^[0-9a-f]{64}$")) and
        (.binary_blake3|test("^[0-9a-f]{64}$")) and
        (.inventory_blake3|test("^[0-9a-f]{64}$")))' "$remote_location") || {
      echo 'cold-cache-store: signed native artifact binding refused' >&2; return 1;
    }
    [[ "$signed_native_binding" == "$native_binding_json" ]] || {
      echo 'cold-cache-store: signed native expectation differs' >&2; return 78;
    }
    local actual_native_fp
    actual_native_fp=$(aura_receipt_source_fingerprint "$root") || return "$?"
    jq -e --arg rev "$revision" --arg fp "$actual_native_fp" '.source.revision==$rev and .source.fingerprint==$fp' <<< "$native_binding_json" >/dev/null || {
      echo 'cold-cache-store: original native source differs' >&2; return 1;
    }
  fi
  local bytes expected allocated free floor others file pid kib archive_kib extract_kib metadata_sha
  local reserve_kib acquired holder attempt
  bytes="$(jq -r '.bytes' "$remote_location")"
  expected="$(jq -r '.sha256' "$remote_location")"
  allocated="$(jq -r '.source_allocated_kib' "$remote_location")"
  metadata_sha="$(jq -r '.metadata_sha256 // empty' "$remote_location")"
  tar_bytes="$(jq -r '.tar_bytes // 0' "$remote_location")"
  tar_sha="$(jq -r '.tar_sha256 // empty' "$remote_location")"
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
  if [[ "$signed_schema" == 2 ]]; then
    local tar_kib=$(( (tar_bytes + 1023) / 1024 ))
    (( tar_kib > extract_kib )) && extract_kib="$tar_kib"
  fi
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
    --argjson schema "$signed_schema" --argjson tar_bytes "$tar_bytes" \
    --arg tar_sha "$tar_sha" --arg gzip_tool "$gzip_bin" --arg gzip_sha "$gzip_sha" \
    '.schema==$schema and .source_revision==$rev and .source_root==$root and .lane==$lane and
     .sha256==$sha and .bytes==$bytes and .source_allocated_kib==$allocated and
     (.metadata_sha256 // "")==$metadata and
     (if $schema==2 then .compression=="gzip-1" and .tar_bytes==$tar_bytes and
       .tar_sha256==$tar_sha and .gzip_tool==$gzip_tool and
       .gzip_sha256==$gzip_sha and .source_relative==null
      elif $metadata=="" then .source_relative==null else .source_relative==$relative end)' \
    "$pending/manifest.json" >/dev/null || {
      echo 'cold-cache-store: remote manifest differs from location receipt' >&2; return 1;
    }
  remote_ssh "cat $remote_ready/$archive_name" > "$pending/$archive_name"
  [[ "$expected" == "$(sha256sum "$pending/$archive_name" | awk '{print $1}')" &&
     "$bytes" == "$(wc -c < "$pending/$archive_name" | tr -d ' ')" ]] || {
    echo 'cold-cache-store: remote archive hash or size mismatch' >&2; return 1;
  }
  if (( evidence == 1 )) || [[ "$signed_schema" == 2 ]]; then
    remote_ssh "cat $remote_ready/members.tsv" > "$pending/members.tsv"
    [[ "$metadata_sha" == "$(sha256sum "$pending/members.tsv" | awk '{print $1}')" ]] || {
      echo 'cold-cache-store: evidence metadata manifest mismatch' >&2; return 1;
    }
  fi
  member_list="$(mktemp "$shared_dir/cold-keys/.members.XXXXXX")"
  kind_list="$(mktemp "$shared_dir/cold-keys/.kinds.XXXXXX")"
  if [[ "$signed_schema" == 2 ]]; then
    # Read at most one byte beyond the signed limit before a full integrity
    # pass. The early gzip producer may get SIGPIPE when head reaches the
    # bound; its integrity status is checked in the following complete pass.
    local bounded_count
    bounded_count="$({ set +o pipefail; "$gzip_bin" -dc "$pending/$archive_name" |
      head -c "$((tar_bytes + 1))" | wc -c; } | tr -d ' ')"
    [[ "$bounded_count" =~ ^[0-9]+$ && "$bounded_count" -le "$tar_bytes" ]] || {
      echo 'cold-cache-store: compressed expansion exceeds signed bound' >&2; return 1;
    }
    local expanded_bytes expanded_sha
    expanded_bytes="$("$gzip_bin" -dc "$pending/$archive_name" | wc -c | tr -d ' ')" || {
      echo 'cold-cache-store: compressed stream integrity failed' >&2; return 1;
    }
    expanded_sha="$("$gzip_bin" -dc "$pending/$archive_name" | sha256sum | awk '{print $1}')" || {
      echo 'cold-cache-store: compressed stream integrity failed' >&2; return 1;
    }
    [[ "$tar_bytes" == "$expanded_bytes" && "$tar_sha" == "$expanded_sha" ]] || {
      echo 'cold-cache-store: compressed expansion differs from signed tar' >&2; return 1;
    }
    "$gzip_bin" -dc "$pending/$archive_name" | tar -tf - > "$member_list"
    "$gzip_bin" -dc "$pending/$archive_name" | tar -tvf - | cut -c1 > "$kind_list"
  else
    tar -tf "$pending/$archive_name" > "$member_list"
    tar -tvf "$pending/$archive_name" | cut -c1 > "$kind_list"
  fi
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
  if [[ "$signed_schema" == 2 ]]; then
    "$gzip_bin" -dc "$pending/$archive_name" | tar -C "$restore_stage/payload" -pxf - || {
      echo 'cold-cache-store: staged compressed extraction failed' >&2; return 1;
    }
  else
    tar -C "$restore_stage/payload" -pxf "$pending/$archive_name" || {
    echo 'cold-cache-store: staged extraction failed' >&2; return 1;
    }
  fi
  if (( evidence == 1 )) || [[ "$signed_schema" == 2 ]]; then
    if (( evidence == 1 )); then
      local payload_status=0
      evidence_validate_payload "$restore_stage/payload" 0 || payload_status=$?
      if (( payload_status != 0 )); then
        echo 'cold-cache-store: restored evidence metadata or permissions refused' >&2
        if (( native == 1 )); then return "$payload_status"; else return 1; fi
      fi
    fi
    # Extraction writes children after creating '.', so restore its signed
    # directory mtime before comparing the full metadata inventory.
    local root_epoch root_stamp
    root_epoch="$(awk -F '\t' 'NR==1 {print $4}' "$pending/members.tsv")"
    [[ "$root_epoch" =~ ^[0-9]+$ ]] || return 1
    root_stamp="$(date -u -r "$root_epoch" +%Y%m%d%H%M.%S 2>/dev/null ||
      date -u -d "@$root_epoch" +%Y%m%d%H%M.%S)" || return 1
    TZ=UTC touch -t "$root_stamp" "$restore_stage/payload"
    archive_members "$restore_stage/payload" > "$pending/restored-members.tsv" || {
      echo 'cold-cache-store: restored member inventory failed' >&2; return 1;
    }
    cmp "$pending/members.tsv" "$pending/restored-members.tsv" || {
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
