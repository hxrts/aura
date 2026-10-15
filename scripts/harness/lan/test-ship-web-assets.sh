#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
fixture=$(mktemp -d)
trap 'rm -rf "$fixture"' EXIT
repo="$fixture/repo"
public=target/dx/aura-web/release/web/public
mkdir -p "$repo/$public/assets" "$repo/crates/aura-web/public/assets" "$fixture/old" "$fixture/copied"
printf 'fixed-commit stylesheet\n' > "$repo/crates/aura-web/public/assets/tailwind.css"
ln -s "$repo/crates/aura-web/public/assets/tailwind.css" "$repo/$public/assets/tailwind.css"

# The old link-preserving transfer leaves an unusable absolute link on a host
# whose checkout has a different path.
rsync -a --links "$repo/$public/" "$fixture/old/"
[[ -L "$fixture/old/assets/tailwind.css" ]]
bash "$here/ship-web-assets.sh" "$repo" "$public" "$fixture/copied/"
[[ -f "$fixture/copied/assets/tailwind.css" && ! -L "$fixture/copied/assets/tailwind.css" ]]
cmp "$repo/crates/aura-web/public/assets/tailwind.css" "$fixture/copied/assets/tailwind.css"

printf 'foreign\n' > "$fixture/foreign"
ln -s "$fixture/foreign" "$repo/$public/assets/foreign.css"
if bash "$here/ship-web-assets.sh" "$repo" "$public" "$fixture/copied/" 2> "$fixture/refusal"; then
  echo 'ship web assets accepted an escaping link' >&2; exit 1
fi
rg -q 'escapes trusted roots' "$fixture/refusal"
[[ ! -e "$fixture/copied/assets/foreign.css" ]]

rm "$repo/$public/assets/foreign.css"
ln -s "$fixture/missing" "$repo/$public/assets/broken.css"
if bash "$here/ship-web-assets.sh" "$repo" "$public" "$fixture/copied/" 2> "$fixture/refusal"; then
  echo 'ship web assets accepted a broken link' >&2; exit 1
fi
rg -q 'broken build-output link' "$fixture/refusal"
echo 'ship web asset link validation: ok'
