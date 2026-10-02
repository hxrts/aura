#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
test_root="$(mktemp -d "${TMPDIR:-/tmp}/aura-install-test.XXXXXX")"
trap 'rm -rf "$test_root"' EXIT
mkdir -p "$test_root/bin" "$test_root/fakebin"
printf 'old\n' > "$test_root/bin/aura"
printf 'new\n' > "$test_root/source"
cat > "$test_root/fakebin/codesign" <<'EOF'
#!/usr/bin/env bash
[[ ! -f "$FAIL_SIGN_FILE" ]]
EOF
chmod +x "$test_root/fakebin/codesign"
export PATH="$test_root/fakebin:$PATH"
export FAIL_SIGN_FILE="$test_root/fail-sign"
touch "$FAIL_SIGN_FILE"
if bash "$repo_root/scripts/dev/install-aura-binary.sh" "$test_root/source" "$test_root/bin/aura" >/dev/null 2>&1; then
  echo 'expected signing failure' >&2
  exit 1
fi
[[ "$(cat "$test_root/bin/aura")" == old ]]
rm "$FAIL_SIGN_FILE"
bash "$repo_root/scripts/dev/install-aura-binary.sh" "$test_root/source" "$test_root/bin/aura" >/dev/null
[[ "$(cat "$test_root/bin/aura")" == new && -x "$test_root/bin/aura" ]]
if find "$test_root/bin" -name '.aura-install.*' | rg -q .; then
  echo 'temporary install file survived' >&2
  exit 1
fi
echo 'install-aura-binary safety tests passed'
