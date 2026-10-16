#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-ownership-fast-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'if [[ "${AURA_KEEP_POLICY_FIXTURE:-0}" != 1 ]]; then rm -rf "$fixture"; else echo "fixture=$fixture" >&2; fi' EXIT
project="$fixture/project"
mkdir -p "$project/crates/demo/src" "$project/target/debug" "$fixture/fakebin" \
  "$project/crates/aura-core/src" "$project/crates/aura-mpst/src" \
  "$project/crates/aura-macros/src" "$fixture/cargo-home/registry/src/index.fixture" \
  "$fixture/cargo-home/registry/cache/index.fixture"
git init -q "$project"
printf '[package]\nname="fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf 'pub fn owned() {}\n' > "$project/crates/demo/src/lib.rs"
for crate in core mpst macros; do
  printf '[package]\nname="hxrts-aura-%s"\nversion="0.1.0"\n' "$crate" > "$project/crates/aura-$crate/Cargo.toml"
  printf 'pub fn fixture() {}\n' > "$project/crates/aura-$crate/src/lib.rs"
done
printf 'flake\n' > "$project/flake.nix"
printf 'lock\n' > "$project/flake.lock"
mkdir -p "$fixture/archive/fixture-registry-1.0.0"
printf '[package]\nname="fixture-registry"\nversion="1.0.0"\n' > "$fixture/archive/fixture-registry-1.0.0/Cargo.toml"
printf 'pub fn fixture() {}\n' > "$fixture/archive/fixture-registry-1.0.0/lib.rs"
tar -cf "$fixture/cargo-home/registry/cache/index.fixture/fixture-registry-1.0.0.crate" \
  -C "$fixture/archive" fixture-registry-1.0.0
cp -R "$fixture/archive/fixture-registry-1.0.0" "$fixture/cargo-home/registry/src/index.fixture/"
printf 'ok\n' > "$fixture/cargo-home/registry/src/index.fixture/fixture-registry-1.0.0/.cargo-ok"
archive_hash="$(shasum -a 256 "$fixture/cargo-home/registry/cache/index.fixture/fixture-registry-1.0.0.crate" | awk '{print $1}')"
printf '[[package]]\nname = "fixture-registry"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "%s"\n' "$archive_hash" > "$project/Cargo.lock"
cat > "$fixture/metadata.json" <<EOF
{"packages":[
{"id":"core","name":"hxrts-aura-core","version":"0.1.0","source":null,"manifest_path":"$project/crates/aura-core/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"mpst","name":"hxrts-aura-mpst","version":"0.1.0","source":null,"manifest_path":"$project/crates/aura-mpst/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"macros","name":"hxrts-aura-macros","version":"0.1.0","source":null,"manifest_path":"$project/crates/aura-macros/Cargo.toml","targets":[{"kind":["bin"]}]},
{"id":"registry","name":"fixture-registry","version":"1.0.0","source":"registry+https://github.com/rust-lang/crates.io-index","manifest_path":"$fixture/cargo-home/registry/src/index.fixture/fixture-registry-1.0.0/Cargo.toml","targets":[{"kind":["lib"]}]}
],"resolve":{"nodes":[
{"id":"core","deps":[]},
{"id":"mpst","deps":[{"pkg":"core","dep_kinds":[{"kind":null}]}]},
{"id":"macros","deps":[{"pkg":"mpst","dep_kinds":[{"kind":null}]},{"pkg":"registry","dep_kinds":[{"kind":null}]}]},
{"id":"registry","deps":[]}
]}}
EOF
printf '/target/\n/artifacts/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml Cargo.lock flake.nix flake.lock crates .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
cat > "$fixture/fakebin/cargo" <<'EOF'
#!/usr/bin/env bash
if [[ "$*" == -V || "$*" == -Vv ]]; then printf 'cargo fixture 1.93\n'; exit 0; fi
if [[ "$*" == 'metadata --locked --offline --format-version 1' ]]; then cat "$FIXTURE_METADATA"; exit 0; fi
[[ "$*" == 'build -p hxrts-aura-macros --bin ownership_lints' ]] || exit 2
printf 'compile\n' >> "$COMPILE_COUNTER"
EOF
cat > "$project/target/debug/ownership_lints" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$1" >> "$LINT_COUNTER"
[[ "${FAIL_MODE:-}" != "$1" ]]
EOF
chmod +x "$fixture/fakebin/cargo" "$project/target/debug/ownership_lints"
export PATH="$fixture/fakebin:$PATH" COMPILE_COUNTER="$fixture/compile-count" LINT_COUNTER="$fixture/lint-count" \
  FIXTURE_METADATA="$fixture/metadata.json" CARGO_HOME="$fixture/cargo-home"
source "$repo_root/scripts/dev/build-receipt.sh"
export AURA_BUILD_PACKAGES=hxrts-aura-macros
aura_receipt_init "$project" ownership-lint-compile bash ownership-lint-compile
run_fast() {
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
    --compile-receipt "$compile_receipt" crates/demo/src/lib.rs
}
compile_fast() {
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --compile-stage --certify-compile-inputs --root "$project" \
    crates/demo/src/lib.rs
}
compile_fast > "$fixture/output"
compile_receipt="$AURA_BUILD_RECEIPT"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
jq -e '.metadata.policy_binary_hash!=null and
  .metadata.policy_compile_graph_certified==true and
  [.phases[].phase]==["compile"]' "$compile_receipt" >/dev/null
aura_receipt_finish 0
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
run_fast > "$fixture/output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
[[ "$(wc -l < "$LINT_COUNTER" | tr -d ' ')" == 6 ]]
jq -e '.metadata.source_scope==["crates/demo/src/lib.rs"] and
  (.metadata.policy_modes | length)==6 and
  .metadata.policy_compile_receipt!=null and
  ([.phases[].phase] == ["execution","execution","execution","execution","execution","execution"]) and
  ([.phases[].exit_status] | all(.==0))' "$AURA_BUILD_RECEIPT" >/dev/null
# A new app source revision may execute the same certified lint binary. The
# compile receipt retains its original source; only the execution scope moves.
printf 'pub fn unrelated() {}\n' > "$project/crates/demo/src/unrelated.rs"
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
run_fast > "$fixture/output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == 1 ]]
jq -e --arg original "$(jq -r '.source.fingerprint' "$compile_receipt")" \
  '.source.fingerprint!=$original and
   .metadata.policy_compile_source_fingerprint==$original' "$AURA_BUILD_RECEIPT" >/dev/null
rm "$project/crates/demo/src/unrelated.rs"
if bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
  --compile-receipt "$compile_receipt" \
  ../outside.rs > "$fixture/output" 2>&1; then exit 1; fi
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
if FAIL_MODE=optional-owner-boundary run_fast > "$fixture/output" 2>&1; then exit 1; fi
jq -e '[.phases[] | select(.phase=="execution" and .exit_status!=0)] | length==1' \
  "$AURA_BUILD_RECEIPT" >/dev/null
mkdir -p "$fixture/shared"
AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=2 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --root "$project" \
    crates/demo/src/lib.rs > "$fixture/packet-output"
packet_compile="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '1p')"
packet_execute="$(sed -n 's/^Build receipt: //p' "$fixture/packet-output" | sed -n '2p')"
[[ -f "$packet_compile" && -f "$packet_execute" && "$packet_compile" != "$packet_execute" ]]
jq -e '.lane=="ownership-lint-compile" and .final_exit_status==0 and
  ([.phases[] | .phase] | index("compile")!=null) and
  ([.phases[] | .phase] | index("command")!=null)' "$packet_compile" >/dev/null
jq -e --arg parent "$packet_compile" '.lane=="ownership-lint-exec" and
  .final_exit_status==0 and .metadata.policy_compile_receipt==$parent and
  ([.phases[] | select(.phase=="execution")] | length)==6' "$packet_execute" >/dev/null
if AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=999999 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/build-budget.sh" --root "$project" \
    --lane ownership-lint-exec --no-prune -- \
    bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage \
      --root "$project" --compile-receipt "$packet_compile" crates/demo/src/lib.rs \
    > "$fixture/refused-output" 2>&1; then exit 1; fi
refused="$(sed -n 's/^Build receipt: //p' "$fixture/refused-output" | tail -1)"
jq -e '.admission.admitted!=true and .final_exit_status!=0 and
  ([.phases[] | select(.phase=="execution")] | length)==0' "$refused" >/dev/null
jq -e '.final_exit_status==0 and
  ([.phases[] | select(.phase=="compile")] | length)==1' "$packet_compile" >/dev/null
prior_compiles="$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')"
AURA_BUILD_SHARED_DIR="$fixture/shared" AURA_BUILD_MIN_FREE_GIB=2 \
  AURA_BUILD_EMERGENCY_FREE_GIB=1 AURA_BUILD_RESERVE_GIB=1 \
  bash "$repo_root/scripts/dev/run-ownership-fast.sh" --root "$project" \
    --compile-receipt "$packet_compile" crates/demo/src/lib.rs \
    > "$fixture/resumed-output"
[[ "$(wc -l < "$COMPILE_COUNTER" | tr -d ' ')" == "$prior_compiles" ]]
resumed="$(sed -n 's/^Build receipt: //p' "$fixture/resumed-output" | tail -1)"
jq -e --arg parent "$packet_compile" '.lane=="ownership-lint-exec" and
  .final_exit_status==0 and .metadata.policy_compile_receipt==$parent' "$resumed" >/dev/null
source "$repo_root/scripts/dev/policy-dependency-inputs.sh"
baseline="$(policy_ownership_compile_inputs "$project" reuse)"
printf 'outside src\n' > "$project/crates/aura-core/README.md"
[[ "$baseline" != "$(policy_ownership_compile_inputs "$project" reuse)" ]]
rm "$project/crates/aura-core/README.md"
mkdir "$project/crates/aura-mpst/new-empty-directory"
[[ "$baseline" != "$(policy_ownership_compile_inputs "$project" reuse)" ]]
rmdir "$project/crates/aura-mpst/new-empty-directory"
printf 'changed\n' >> "$project/Cargo.lock"
[[ "$baseline" != "$(policy_ownership_compile_inputs "$project" reuse)" ]]
sed -i.bak '$d' "$project/Cargo.lock"; rm "$project/Cargo.lock.bak"
archive="$fixture/cargo-home/registry/cache/index.fixture/fixture-registry-1.0.0.crate"
cp "$archive" "$fixture/archive-original.crate"
printf 'tamper\n' >> "$archive"
if policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
cp "$fixture/archive-original.crate" "$archive"
printf 'tamper\n' >> "$fixture/cargo-home/registry/src/index.fixture/fixture-registry-1.0.0/lib.rs"
[[ "$baseline" != "$(policy_ownership_compile_inputs "$project" reuse)" ]]
if policy_ownership_compile_inputs "$project" certify > "$fixture/output"; then exit 1; fi
printf 'pub fn fixture() {}\n' > "$fixture/cargo-home/registry/src/index.fixture/fixture-registry-1.0.0/lib.rs"
ln -s src/lib.rs "$project/crates/aura-macros/linked.rs"
if policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
rm "$project/crates/aura-macros/linked.rs"
jq '(.packages[] | select(.name=="hxrts-aura-core") | .targets) += [{kind:["custom-build"]}]' \
  "$fixture/metadata.json" > "$fixture/metadata-build.json"
if FIXTURE_METADATA="$fixture/metadata-build.json" \
  policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
jq '(.resolve.nodes[] | select(.id=="macros") | .features)=["new-feature"]' \
  "$fixture/metadata.json" > "$fixture/metadata-feature.json"
[[ "$baseline" != "$(FIXTURE_METADATA="$fixture/metadata-feature.json" \
  policy_ownership_compile_inputs "$project" reuse)" ]]
jq '.packages += [{id:"foreign",name:"foreign",version:"0.1.0",source:null,
  manifest_path:"/tmp/foreign/Cargo.toml",targets:[{kind:["lib"]}]}] |
  .resolve.nodes += [{id:"foreign",deps:[],features:[]}] |
  (.resolve.nodes[] | select(.id=="macros") | .deps) +=
    [{pkg:"foreign",dep_kinds:[{kind:null}]}]' "$fixture/metadata.json" \
  > "$fixture/metadata-path.json"
if FIXTURE_METADATA="$fixture/metadata-path.json" \
  policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/fakebin/native-cc"
chmod +x "$fixture/fakebin/native-cc"
[[ "$baseline" != "$(CC=native-cc policy_ownership_compile_inputs "$project" reuse)" ]]
if SDKROOT="$fixture/archive" policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
if CFLAGS_aarch64_apple_darwin="-I$fixture/archive" \
  policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
if policy_no_link_components "$project/crates/../crates/aura-core/Cargo.toml"; then exit 1; fi
jq --arg manifest "$project/crates/../crates/aura-core/Cargo.toml" \
  '(.packages[] | select(.name=="hxrts-aura-core") | .manifest_path)=$manifest' \
  "$fixture/metadata.json" > "$fixture/metadata-traversal.json"
if FIXTURE_METADATA="$fixture/metadata-traversal.json" \
  policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
printf '#!/usr/bin/env bash\nexit 0\n' > "$fixture/fakebin/native-cc"
native_before="$(CC=native-cc policy_ownership_compile_inputs "$project" reuse)"
printf '# changed executable bytes\n' >> "$fixture/fakebin/native-cc"
[[ "$native_before" != "$(CC=native-cc policy_ownership_compile_inputs "$project" reuse)" ]]
# A malformed exported-name producer must refuse certification instead of
# silently producing an incomplete native selector inventory.
export() {
  if [[ "$1" == -p ]]; then printf 'invalid declaration\n'; else builtin export "$@"; fi
}
if policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
unset -f export
mv "$project/crates" "$project/crates-real"
ln -s crates-real "$project/crates"
if policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
rm "$project/crates"; mv "$project/crates-real" "$project/crates"
mv "$fixture/cargo-home/registry/cache" "$fixture/cargo-home/registry/cache-real"
ln -s cache-real "$fixture/cargo-home/registry/cache"
if policy_ownership_compile_inputs "$project" reuse > "$fixture/output"; then exit 1; fi
rm "$fixture/cargo-home/registry/cache"
mv "$fixture/cargo-home/registry/cache-real" "$fixture/cargo-home/registry/cache"
# A locked archive with an early unsafe member must be refused before tar is
# ever allowed to extract. Listing failures likewise cannot become a success
# through an early-exit pipe status.
cat > "$fixture/fakebin/tar" <<'FAKE_TAR'
#!/usr/bin/env bash
[[ "$1" != -xf ]] || printf 'extract\n' >> "$TAR_EXTRACT_COUNTER"
exec /usr/bin/tar "$@"
FAKE_TAR
chmod +x "$fixture/fakebin/tar"
export TAR_EXTRACT_COUNTER="$fixture/tar-extract-count"
cp "$project/Cargo.lock" "$fixture/lock-original"
mkdir -p "$fixture/unsafe/fixture-registry-1.0.0"
ln -s /tmp/escape "$fixture/unsafe/fixture-registry-1.0.0/escape"
for number in $(seq 1 100); do
  printf 'valid\n' > "$fixture/unsafe/fixture-registry-1.0.0/file-$number"
done
/usr/bin/tar -cf "$archive" -C "$fixture/unsafe" fixture-registry-1.0.0/escape fixture-registry-1.0.0
unsafe_hash="$(shasum -a 256 "$archive" | awk '{print $1}')"
sed "s/$archive_hash/$unsafe_hash/" "$fixture/lock-original" > "$project/Cargo.lock"
if policy_ownership_compile_inputs "$project" certify > "$fixture/output"; then exit 1; fi
[[ ! -e "$TAR_EXTRACT_COUNTER" ]]
head -c 128 "$fixture/archive-original.crate" > "$archive"
truncated_hash="$(shasum -a 256 "$archive" | awk '{print $1}')"
sed "s/$archive_hash/$truncated_hash/" "$fixture/lock-original" > "$project/Cargo.lock"
if policy_ownership_compile_inputs "$project" certify > "$fixture/output" 2>&1; then exit 1; fi
[[ ! -e "$TAR_EXTRACT_COUNTER" ]]
cp "$fixture/archive-original.crate" "$archive"
cp "$fixture/lock-original" "$project/Cargo.lock"
[[ "$baseline" == "$(policy_ownership_compile_inputs "$project" reuse)" ]]
# Default runs stay on the cheap exact-source path and do not certify or hash
# the registry graph. They refuse a new source revision with the same binary.
aura_receipt_init "$project" ownership-lint-compile bash ownership-lint-compile
bash "$repo_root/scripts/dev/run-ownership-fast.sh" --compile-stage --root "$project" \
  crates/demo/src/lib.rs > "$fixture/output"
default_compile="$AURA_BUILD_RECEIPT"
aura_receipt_finish 0
jq -e '.metadata.policy_compile_graph_certified==false and
  .metadata.policy_compile_graph_hash==null' "$default_compile" >/dev/null
# An older exact-source receipt without the optional field retains its
# original source/scope guard instead of being treated as certified.
jq 'del(.metadata.policy_compile_graph_certified,.metadata.policy_compile_graph_hash)' \
  "$default_compile" > "$fixture/legacy-receipt.json"
mv "$fixture/legacy-receipt.json" "$default_compile"
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
  --compile-receipt "$default_compile" crates/demo/src/lib.rs > "$fixture/output"
printf 'new app source\n' > "$project/crates/demo/src/new.rs"
aura_receipt_init "$project" ownership-lint-exec bash ownership-lint-exec
if bash "$repo_root/scripts/dev/run-ownership-fast.sh" --execution-stage --root "$project" \
  --compile-receipt "$default_compile" crates/demo/src/lib.rs > "$fixture/output" 2>&1; then exit 1; fi
rm "$project/crates/demo/src/new.rs"
echo 'ownership fast: guarded receipts, cross-source reuse, compiler graph integrity, six modes and refusal cases passed'
