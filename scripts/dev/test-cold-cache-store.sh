#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-cold-cache-test.XXXXXX")"
fixture="$(cd "$fixture" && pwd -P)"
trap 'rm -rf "$fixture"' EXIT
project="$fixture/project"
store="$fixture/store"
fakebin="$fixture/fakebin"
mkdir -p "$project/target/dylint" "$store" "$fakebin"
git init -q "$project"
printf '[package]\nname="cold-cache-fixture"\nversion="0.1.0"\n' > "$project/Cargo.toml"
printf '/target/\n' > "$project/.gitignore"
git -C "$project" add Cargo.toml .gitignore
git -C "$project" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
  -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm initial
printf 'preserved compiler artifact\n' > "$project/target/dylint/cache"
cat > "$fakebin/stat" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == -f && "$2" == %d ]]; then
  [[ "$3" != "$AURA_COLD_CACHE_STORE" ]] || { printf '%s\n' "${STORE_DEVICE:-2}"; exit; }
  printf '1\n'
elif [[ "$1" == -f && "$2" == %T ]]; then printf 'apfs\n'
elif [[ "$1" == -f && "$2" == %u ]]; then printf '%s\n' "$(id -u)"
elif [[ "$1" == -f && "$2" == %Lp ]]; then printf '%s\n' "${STORE_MODE:-755}"
else exit 2; fi
EOF
cat > "$fakebin/lsof" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$fakebin/ps" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
cat > "$fakebin/df" <<'EOF'
#!/usr/bin/env bash
if [[ "$1" == -P ]]; then
  printf 'Filesystem 512-blocks Used Available Capacity Mounted on\n'
  if [[ "$2" == "$AURA_COLD_CACHE_STORE" ]]; then
    printf '/dev/disk-store 100 1 99 1%% /store\n'
  else
    printf '/dev/disk-source 100 1 99 1%% /source\n'
  fi
else
  /bin/df "$@"
fi
EOF
cat > "$fakebin/diskutil" <<'EOF'
#!/usr/bin/env bash
[[ "$1" == info && "$2" == -plist ]] || exit 2
case "$3" in
  /dev/disk-source) pool="${SOURCE_POOL:-source-container}" ;;
  /dev/disk-store) pool="${STORE_POOL-store-container}" ;;
  *) exit 2 ;;
esac
[[ -n "$pool" ]] || exit 1
printf '<?xml version="1.0"?><plist version="1.0"><dict><key>APFSContainerReference</key><string>%s</string></dict></plist>\n' "$pool"
EOF
cat > "$fakebin/plutil" <<'EOF'
#!/usr/bin/env bash
[[ "$*" == '-extract APFSContainerReference raw -o - -' ]] || exit 2
sed -n 's,.*<string>\([^<]*\)</string>.*,\1,p'
EOF
cat > "$fakebin/uname" <<'EOF'
#!/usr/bin/env bash
[[ "$1" == -s ]] || exit 2
printf 'Darwin\n'
EOF
chmod +x "$fakebin"/*
export PATH="$fakebin:$PATH" AURA_COLD_CACHE_STORE="$store"
export AURA_BUILD_SHARED_DIR="$fixture/shared"
export AURA_BUILD_MIN_FREE_GIB=0
cold() { bash "$repo_root/scripts/dev/cold-cache-store.sh" --root "$project" --lane dylint "$@"; }
expect_fail() {
  if cold "$@" > "$fixture/output" 2>&1; then
    echo "unexpected cold-store success: $*" >&2; exit 1
  fi
}
STORE_DEVICE=1 expect_fail --export
[[ -f "$project/target/dylint/cache" ]]
STORE_MODE=777 expect_fail --export
[[ -f "$project/target/dylint/cache" ]]
STORE_POOL=source-container expect_fail --export
rg -q 'shares the source APFS free-space pool' "$fixture/output"
[[ -f "$project/target/dylint/cache" ]]
STORE_POOL='' expect_fail --export
rg -q 'cannot verify independent APFS container capacity' "$fixture/output"
[[ -f "$project/target/dylint/cache" ]]
expect_fail --export --compression gzip-1
rg -q 'compressed archive requires remote ext4' "$fixture/output"
[[ -f "$project/target/dylint/cache" ]]
ln -s "$fixture" "$store/aura-cold-cache"
expect_fail --export
rm "$store/aura-cold-cache"
printf '# dirty\n' >> "$project/Cargo.toml"
expect_fail --export
git -C "$project" checkout -- Cargo.toml
cold --export > "$fixture/output"
[[ ! -e "$project/target/dylint" ]]
snapshot="$(find "$store" -name archive.tar -print -quit)"
[[ -f "$snapshot" && "$(stat -f %T "$store")" == apfs ]]
manifest="${snapshot%/archive.tar}/manifest.json"
cp "$manifest" "$fixture/signed-manifest"
printf ' ' >> "$manifest"
expect_fail --restore
cp "$fixture/signed-manifest" "$manifest"
AURA_BUILD_MIN_FREE_GIB=99999 expect_fail --restore
cold --restore > "$fixture/output"
[[ "$(cat "$project/target/dylint/cache")" == 'preserved compiler artifact' ]]
expect_fail --restore
rm -rf "$project/target/dylint"
printf 'tamper' >> "$snapshot"
expect_fail --restore
[[ ! -e "$project/target/dylint" ]]
mkdir -p "$fixture/evil"
ln -s /tmp "$fixture/evil/escape"
tar -C "$fixture/evil" -cf "$snapshot" .
sha="$(sha256sum "$snapshot" | awk '{print $1}')"
bytes="$(wc -c < "$snapshot" | tr -d ' ')"
jq --arg sha "$sha" --argjson bytes "$bytes" \
  '.sha256=$sha | .bytes=$bytes' "$manifest" > "$fixture/new-manifest"
mv "$fixture/new-manifest" "$manifest"
key="$(find "$AURA_BUILD_SHARED_DIR/cold-keys" -name '*.pem' -print -quit)"
openssl dgst -sha256 -sign "$key" -out "${snapshot%/archive.tar}/manifest.sig" "$manifest"
expect_fail --restore
[[ ! -e "$project/target/dylint" ]]
echo 'cold cache: distinct-device and APFS-pool, clean-source, guarded export/restore, occupied lane, tamper and linked-member refusal passed'
