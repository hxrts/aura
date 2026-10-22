#!/usr/bin/env bash
# Immutable data export only. No application, admission, test or review authority.
set -euo pipefail
owner=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
source "$owner/build-receipt.sh"
fail() { printf 'source-handoff: %s\n' "$*" >&2; return 78; }
bytes() { local result; result=$(b3sum "$1") || return; printf '%s\n' "${result%% *}"; }
canonical_request() {
 local request=$1 canonical=$2
 [[ -f $request && ! -L $request ]] || return 78
 jq -se '
 length==1 and (.[0]|type=="object") and (.[0]|keys)==["configuration","evidence","helpers","inventories","schema"] and .[0].schema==1 and
 (.[0].configuration|type=="object") and
 (.[0].configuration.argv|type=="array" and length>0 and all(.[];type=="string")) and
 ([.[0].inventories,.[0].helpers,.[0].evidence]|all(.[];type=="array")) and
 ([.[0].inventories[],.[0].helpers[],.[0].evidence[]]|all(.[];
 type=="object" and keys==["name","path"] and (.name|type=="string" and test("^[A-Za-z0-9][A-Za-z0-9_.-]*$")) and
 (.path|type=="string" and length>0 and (startswith("/")|not) and (test("[\\\\\u0000-\u001f]")|not) and
 (split("/")|all(.[];length>0 and .!="." and .!=".."))))) and
 ([.[0].inventories[],.[0].helpers[],.[0].evidence[]]|map(.name)|length== (unique|length)) and
 ([.[0].inventories[],.[0].helpers[],.[0].evidence[]]|map(.path)|length== (unique|length))' "$request" >/dev/null || return 78
 jq -Sc . "$request" > "$canonical" || return
 cmp -s "$request" "$canonical" || { fail 'request must be one canonical JSON object (jq -Sc), without duplicate keys'; return 78; }
}
source_state() {
 local root=$1 capture=$2 path record
 git -C "$root" rev-parse HEAD > "$capture/head" || return
 git -C "$root" write-tree > "$capture/index" || return
 git -C "$root" -c core.fsmonitor=false diff --quiet --no-ext-diff --no-textconv || return 78
 git -C "$root" ls-files --others --exclude-standard -z > "$capture/untracked" || return
 [[ ! -s $capture/untracked ]] || return 78
 git -C "$root" ls-files -v -z > "$capture/index-flags" || return
 while IFS= read -r -d '' record; do [[ $record == 'H '* ]] || return 78; done < "$capture/index-flags"
 aura_receipt_source_fingerprint "$root" > "$capture/legacy" || return
}
regular_input() {
 local input_root=$1 path=$2 parent
 [[ -f $input_root/$path && ! -L $input_root/$path ]] || return 78
 parent=$(cd "$(dirname "$input_root/$path")" && pwd -P) || return
 [[ $parent/$(basename "$path") == "$input_root/$path" ]] || return 78
}
# All packet names are generated here; user paths remain JSON data.
manifest() (
 cd "$1" || exit
 find . -type f ! -path ./BLAKE3SUMS -print > "$2" || exit
 LC_ALL=C sort "$2" | while IFS= read -r f; do
  b3sum "$f" || exit
 done
)
export_packet() (
 root=$(cd "$1" && pwd -P) || exit
 base=$2; target=$3; input_root=$(cd "$4" && pwd -P) || exit
 request=$5; output=$6
 [[ $base =~ ^[0-9a-f]{40}$ && $target =~ ^[0-9a-f]{40}$ ]] || exit 78
 base_kind=$(git -C "$root" cat-file -t "$base") || exit
 target_kind=$(git -C "$root" cat-file -t "$target") || exit
 [[ $base_kind == commit && $target_kind == tree ]] || exit 78
 [[ ! -e $output && ! -L $output ]] || exit 78
 parent=$(cd "$(dirname "$output")" && pwd -P) || exit
 # Private construction stays outside source; publish only a complete packet.
 case "$parent/" in "$root/"|"$root/"*) fail 'output must be outside source'; exit 78;; esac
 work=$(mktemp -d "$parent/.aura-source-handoff.XXXXXX") || exit
 trap 'rm -rf "$work"' EXIT
 mkdir "$work/state-before" "$work/state-after" "$work/packet" || exit
 p=$work/packet
 canonical_request "$request" "$p/request.json" || exit
 source_state "$root" "$work/state-before" || exit
 read -r actual_target < "$work/state-before/index"
 [[ $target == "$actual_target" ]] || exit 78
 base_tree=$(git -C "$root" rev-parse "$base^{tree}") || exit
 git -C "$root" ls-tree -rz --full-tree "$base_tree" > "$p/base-tree.raw" || exit
 git -C "$root" ls-tree -rz --full-tree "$target" > "$p/target-tree.raw" || exit
 git -C "$root" -c diff.algorithm=myers diff --binary --full-index --no-ext-diff --no-textconv --no-renames --no-color --src-prefix=a/ --dst-prefix=b/ --unified=3 --inter-hunk-context=0 "$base_tree" "$target" -- > "$p/source.patch" || exit
 git -C "$root" diff-tree --no-commit-id --no-renames --raw -r -z --no-abbrev "$base_tree" "$target" > "$work/changes.raw" || exit
 mkdir "$p/blobs" "$p/inputs" || exit
 : > "$work/changes.jsonl"
 while IFS= read -r -d '' record; do
  IFS= read -r -d '' path || exit 78
  read -r old_mode new_mode old_blob new_blob status <<< "${record#:}"
  [[ $old_blob =~ ^[0-9a-f]{40}$ && $new_blob =~ ^[0-9a-f]{40}$ ]] || exit 78
  # Tree entries/submodules are not silently represented as regular source blobs.
  for pair in "$old_mode:$old_blob" "$new_mode:$new_blob"; do
   mode=${pair%%:*}; blob=${pair#*:}
   case $mode in 000000) continue;; 100644|100755|120000) ;; *) exit 78;; esac
   if [[ ! -f $p/blobs/$blob ]]; then git -C "$root" cat-file blob "$blob" > "$p/blobs/$blob" || exit; fi
  done
  jq -cn --arg path "$path" --arg old_mode "$old_mode" --arg new_mode "$new_mode" --arg old_blob "$old_blob" --arg new_blob "$new_blob" --arg status "$status" '{path:$path,before:{mode:$old_mode,blob:$old_blob},after:{mode:$new_mode,blob:$new_blob},status:$status}' >> "$work/changes.jsonl" || exit
 done < "$work/changes.raw"
 jq -s . "$work/changes.jsonl" > "$p/changes.json" || exit
 for category in inventories helpers evidence; do
  mkdir "$p/inputs/$category" || exit
  jq -r --arg c "$category" '.[$c][]|.name,.path' "$p/request.json" > "$work/selected" || exit
  while IFS= read -r name; do
   IFS= read -r path || exit 78
   regular_input "$input_root" "$path" || exit
   cp "$input_root/$path" "$p/inputs/$category/$name" || exit
  done < "$work/selected"
 done
 read -r head < "$work/state-before/head"
 read -r legacy < "$work/state-before/legacy"
 jq -nSc --arg base "$base" --arg base_tree "$base_tree" --arg target "$target" --arg head "$head" --arg legacy "$legacy" '{format:"aura-source-handoff-blake3-v1",base_commit:$base,base_tree:$base_tree,target_tree:$target,producer_head:$head,producer_legacy_source:{algorithm:"sha256",digest:$legacy},semantic_review:"required; no test acceptance implied"}' > "$p/source.json" || exit
 source_state "$root" "$work/state-after" || exit
 for field in head index legacy; do cmp -s "$work/state-before/$field" "$work/state-after/$field" || exit 78; done
 cmp -s "$request" "$p/request.json" || exit 78
 for category in inventories helpers evidence; do
  jq -r --arg c "$category" '.[$c][]|.name,.path' "$p/request.json" > "$work/selected" || exit
  while IFS= read -r name; do
   IFS= read -r path || exit 78
   regular_input "$input_root" "$path" || exit
   cmp -s "$input_root/$path" "$p/inputs/$category/$name" || exit 78
  done < "$work/selected"
 done
 manifest "$p" "$work/packet-files" > "$work/BLAKE3SUMS" || exit
 mv "$work/BLAKE3SUMS" "$p/BLAKE3SUMS" || exit
 digest=$(bytes "$p/BLAKE3SUMS") || exit
 [[ ! -e $output && ! -L $output ]] || exit 78
 mv "$p" "$output" || exit
 printf '%s\n' "$digest"
)
verify_packet() (
 root=$1; input_root=$2; request=$3; packet=$4; expected=$5
 [[ $expected =~ ^[0-9a-f]{64}$ && -d $packet && ! -L $packet ]] || exit 78
 packet=$(cd "$packet" && pwd -P) || exit
 [[ -f $packet/BLAKE3SUMS && ! -L $packet/BLAKE3SUMS ]] || exit 78
 actual=$(bytes "$packet/BLAKE3SUMS") || exit
 [[ $actual == "$expected" ]] || exit 78
 work=$(mktemp -d /tmp/aura-source-handoff-verify.XXXXXX) || exit
 trap 'rm -rf "$work"' EXIT
 find "$packet" ! -type d ! -type f -print > "$work/nonregular" || exit
 [[ ! -s $work/nonregular ]] || exit 78
 # Recreate the canonical manifest, including unexpected extra regular files.
 manifest "$packet" "$work/packet-files" > "$work/actual-manifest" || exit
 cmp -s "$work/actual-manifest" "$packet/BLAKE3SUMS" || exit 78
 (cd "$packet" && b3sum --check BLAKE3SUMS >/dev/null) || exit 78
 base=$(jq -er '.base_commit' "$packet/source.json") || exit
 target=$(jq -er '.target_tree' "$packet/source.json") || exit
 export_packet "$root" "$base" "$target" "$input_root" "$request" "$work/recreated" > "$work/recreated-digest" || exit
 cmp -s "$work/recreated/BLAKE3SUMS" "$packet/BLAKE3SUMS" || exit 78
 printf 'source-handoff: verified immutable inputs; semantic review remains required\n'
)
[[ $# -gt 0 ]] || exit 2
case $1 in
 export) shift; [[ $# == 6 ]] || exit 2; export_packet "$@";;
 verify) shift; [[ $# == 5 ]] || exit 2; verify_packet "$@";;
 *) exit 2;;
esac
