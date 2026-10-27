#!/usr/bin/env bash
set -euo pipefail
owner=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
source "$owner/build-receipt.sh"
f=$(mktemp -d /tmp/aura-source-handoff-test.XXXXXX)
trap 'chmod -R u+w "$f"; rm -rf "$f"' EXIT
mkdir "$f/producer" "$f/inputs" "$f/packets"
git -C "$f/producer" init -q
printf original > "$f/producer/source.rs"
printf remove > "$f/producer/deleted"
git -C "$f/producer" add .
git -C "$f/producer" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm base
base=$(git -C "$f/producer" rev-parse HEAD)
git clone -q "$f/producer" "$f/consumer"
printf changed > "$f/producer/source.rs"
printf '\000\001\377binary\000' > "$f/producer/binary"
printf executable > "$f/producer/exec.sh"; chmod 755 "$f/producer/exec.sh"
ln -s source.rs "$f/producer/link"
rm "$f/producer/deleted"
git -C "$f/producer" add -A
target=$(git -C "$f/producer" write-tree)
printf 'one::actual_fixture\n' > "$f/inputs/inventory"
printf '#!/bin/bash\nexit 0\n' > "$f/inputs/helper"
printf '{"exit_status":101,"scope":"original failure remains failure"}\n' > "$f/inputs/evidence"
jq -nSc '{schema:1,configuration:{argv:["cargo","test","--lib"],incremental:"0",target:"native",features:[],toolchain:"explicit-test-only",wrapper:null},inventories:[{name:"native",path:"inventory"}],helpers:[{name:"runner",path:"helper"}],evidence:[{name:"BLAKE3SUMS",path:"evidence"}]}' > "$f/request.json"
exported=$(aura_receipt_source_handoff_export "$f/producer" "$base" "$target" "$f/inputs" "$f/request.json" "$f/packets/original")
[[ $exported =~ ^[0-9a-f]{64}$ ]]
git -C "$f/consumer" apply --index "$f/packets/original/source.patch"
[[ $(git -C "$f/consumer" write-tree) == "$target" ]]
aura_receipt_source_handoff_verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/original" "$exported" >/dev/null
pass=1
refuse() {
 local status=0
 "$@" > "$f/negative.log" 2>&1 || status=$?
 [[ $status == 78 ]] || { printf 'expected logical refusal 78, got %s: %s\n' "$status" "$*" >&2; cat "$f/negative.log" >&2; exit 1; }
 pass=$((pass+1))
}
verify() { aura_receipt_source_handoff_verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/original" "$exported"; }
# Unstaged and staged source changes both refuse original reuse.
printf foreign >> "$f/consumer/source.rs"; refuse verify
git -C "$f/consumer" checkout -- source.rs
printf foreign >> "$f/consumer/source.rs"; git -C "$f/consumer" add source.rs; refuse verify
git -C "$f/consumer" reset -q --hard "$base"
git -C "$f/consumer" apply --index "$f/packets/original/source.patch"
for input in inventory helper evidence; do
 cp "$f/inputs/$input" "$f/saved"
 printf foreign >> "$f/inputs/$input"; refuse verify
 cp "$f/saved" "$f/inputs/$input"
done
mv "$f/inputs/helper" "$f/saved-helper"; refuse verify
ln -s "$f/saved-helper" "$f/inputs/helper"; refuse verify
rm "$f/inputs/helper"; mv "$f/saved-helper" "$f/inputs/helper"
cp "$f/request.json" "$f/saved-request"
jq -Sc '.configuration.incremental="1"' "$f/saved-request" > "$f/request.json"; refuse verify
cp "$f/saved-request" "$f/request.json"
refuse aura_receipt_source_handoff_export "$f/producer" HEAD "$target" "$f/inputs" "$f/request.json" "$f/packets/ambiguous-base"
base_tree=$(git -C "$f/producer" rev-parse "$base^{tree}")
refuse aura_receipt_source_handoff_export "$f/producer" "$base" "$base_tree" "$f/inputs" "$f/request.json" "$f/packets/wrong-tree"
blob=$(find "$f/packets/original/blobs" -type f | head -1)
cp "$blob" "$f/blob-saved"; printf corrupt >> "$blob"; refuse verify
cp "$f/blob-saved" "$blob"
# Duplicate JSON keys, trailing records, path escapes and colliding names refuse.
for expression in '.helpers[0].path="../helper"' '.helpers[0].path="/absolute"' '.helpers[0].name=".."' '.helpers[0].name="native"'; do
 jq -Sc "$expression" "$f/saved-request" > "$f/request.json"; refuse verify
done
cp "$f/saved-request" "$f/request.json"; printf '\n{}\n' >> "$f/request.json"; refuse verify
sed 's/"schema":1/"schema":1,"schema":1/' "$f/saved-request" > "$f/request.json"; refuse verify
cp "$f/saved-request" "$f/request.json"
# Nested BLAKE3SUMS is ordinary bound data, not excluded from the manifest.
grep -q 'inputs/evidence/BLAKE3SUMS' "$f/packets/original/BLAKE3SUMS"
printf corruption >> "$f/packets/original/inputs/evidence/BLAKE3SUMS"; refuse verify
cp "$f/inputs/evidence" "$f/packets/original/inputs/evidence/BLAKE3SUMS"
# A rewritten self-consistent packet cannot substitute its own expected digest.
cp -R "$f/packets/original" "$f/packets/rewritten"
printf foreign >> "$f/packets/rewritten/inputs/helpers/runner"
(cd "$f/packets/rewritten"; find . -type f ! -path ./BLAKE3SUMS -print | LC_ALL=C sort | while IFS= read -r path; do b3sum "$path"; done) > "$f/rehashed"
cp "$f/rehashed" "$f/packets/rewritten/BLAKE3SUMS"
refuse aura_receipt_source_handoff_verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/rewritten" "$exported"
# Even a caller-held malformed manifest cannot authorize reads outside packet.
mkdir "$f/bin"
real_b3=$(command -v b3sum)
cat > "$f/bin/b3sum" <<'FAKE'
#!/usr/bin/env bash
if [[ ${1:-} == --check ]]; then touch "$CHECK_MARKER"; exit 91; fi
exec "$REAL_B3" "$@"
FAKE
chmod +x "$f/bin/b3sum"
for path in /absolute ../escape; do
 cp "$f/packets/original/BLAKE3SUMS" "$f/good-manifest"
 printf '%s  %s\n' "$exported" "$path" >> "$f/packets/original/BLAKE3SUMS"
 malformed=$($real_b3 "$f/packets/original/BLAKE3SUMS"); malformed=${malformed%% *}
 refuse env PATH="$f/bin:$PATH" REAL_B3="$real_b3" CHECK_MARKER="$f/external-check" bash "$owner/source-handoff.sh" verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/original" "$malformed"
 [[ ! -e $f/external-check ]]
 cp "$f/good-manifest" "$f/packets/original/BLAKE3SUMS"
done
# Read-only packet verification must not create a packet-local scratch file.
chmod -R a-w "$f/packets/original"
verify >/dev/null; pass=$((pass+1))
chmod -R u+w "$f/packets/original"
printf extra > "$f/packets/original/unbound"; refuse verify; rm "$f/packets/original/unbound"
ln -s "$f/inputs/helper" "$f/packets/original/symlink"; refuse verify; rm "$f/packets/original/symlink"
git -C "$f/consumer" update-index --assume-unchanged source.rs; refuse verify
git -C "$f/consumer" update-index --no-assume-unchanged source.rs
# Complete producer output followed by failure must not publish a partial export.
real_git=$(command -v git)
cat > "$f/bin/git" <<'FAKE'
#!/usr/bin/env bash
for arg in "$@"; do
 if [[ $arg == ls-tree ]]; then "$REAL_GIT" "$@"; exit 41; fi
done
exec "$REAL_GIT" "$@"
FAKE
chmod +x "$f/bin/git"
status=0
if PATH="$f/bin:$PATH" REAL_GIT="$real_git" REAL_B3="$real_b3" aura_receipt_source_handoff_export "$f/producer" "$base" "$target" "$f/inputs" "$f/request.json" "$f/packets/failed" > "$f/failed.out" 2>&1; then exit 1; else status=$?; fi
[[ $status == 41 && ! -e $f/packets/failed ]]; pass=$((pass+1))
rm "$f/bin/git"
cat > "$f/bin/b3sum" <<'FAKE'
#!/usr/bin/env bash
"$REAL_B3" "$@" || exit
exit 42
FAKE
status=0
if PATH="$f/bin:$PATH" REAL_B3="$real_b3" aura_receipt_source_handoff_export "$f/producer" "$base" "$target" "$f/inputs" "$f/request.json" "$f/packets/hash-failed" > "$f/hash-failed.out" 2>&1; then exit 1; else status=$?; fi
[[ $status == 42 && ! -e $f/packets/hash-failed ]]; pass=$((pass+1))
# Signed/committed producer applicability: same target alone cannot replace HEAD.
git -C "$f/producer" -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm target
committed=$(aura_receipt_source_handoff_export "$f/producer" "$base" "$target" "$f/inputs" "$f/request.json" "$f/packets/committed")
refuse aura_receipt_source_handoff_verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/committed" "$committed"
git -C "$f/consumer" fetch -q "$f/producer" HEAD
git -C "$f/consumer" reset -q --hard FETCH_HEAD
aura_receipt_source_handoff_verify "$f/consumer" "$f/inputs" "$f/request.json" "$f/packets/committed" "$committed" >/dev/null
pass=$((pass+1))
printf 'PASS %s source handoff controls; actual separate-Git producer/consumer, raw binary+mode+delete+symlink delta, changed/ambiguous inputs and original producer failure retained. No test acceptance or semantic review claim.\n' "$pass"
