#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/aura-kani-pin.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/bin"
cat > "$fixture/bin/cargo" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$*" == "install --locked --force --version 0.68.0 kani-verifier --root $AURA_KANI_ROOT" ]]
printf '%s\n' "$*" > "$AURA_KANI_ROOT/install-args"
[[ "$KANI_PIN_CASE" != install-failure ]] || exit 37
mkdir -p "$AURA_KANI_ROOT/bin"
cp "$KANI_PIN_BINARY" "$AURA_KANI_ROOT/bin/cargo-kani"
chmod +x "$AURA_KANI_ROOT/bin/cargo-kani"
MOCK
cat > "$fixture/binary" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$1" == --version ]]; then
  [[ "$KANI_PIN_CASE" != broken-cache ]] || exit 39
  version=0.68.0
  [[ "$KANI_PIN_CASE" != wrong-install ]] || version=0.69.0
  [[ ! -e "$AURA_KANI_ROOT/old-cache" ]] || version=0.67.0
  echo "Kani Rust Verifier $version"
else
  [[ "$1" == setup ]]
  [[ "$KANI_PIN_CASE" != setup-failure ]] || exit 41
  mkdir -p "$KANI_HOME/kani-0.68.0"
fi
MOCK
chmod +x "$fixture/bin/cargo" "$fixture/binary"
export PATH="$fixture/bin:$PATH" KANI_PIN_BINARY="$fixture/binary"
for KANI_PIN_CASE in fresh cached foreign install-failure wrong-install setup-failure broken-cache; do
  export KANI_PIN_CASE AURA_KANI_ROOT="$fixture/$KANI_PIN_CASE" AURA_KANI_HOME="$fixture/$KANI_PIN_CASE/home"
  mkdir -p "$AURA_KANI_ROOT/bin"
  if [[ "$KANI_PIN_CASE" == cached || "$KANI_PIN_CASE" == broken-cache ]]; then cp "$fixture/binary" "$AURA_KANI_ROOT/bin/cargo-kani"; fi
  if [[ "$KANI_PIN_CASE" == foreign ]]; then
    printf '#!/usr/bin/env bash\necho "Kani Rust Verifier 0.67.0"\n' > "$AURA_KANI_ROOT/bin/cargo-kani"
    chmod +x "$AURA_KANI_ROOT/bin/cargo-kani"
  fi
  status=0
  bash "$here/kani-ensure.sh" > "$fixture/$KANI_PIN_CASE.log" 2>&1 || status=$?
  case "$KANI_PIN_CASE" in
    fresh|cached|foreign) [[ "$status" == 0 ]]; [[ -d "$AURA_KANI_HOME/kani-0.68.0" ]] ;;
    install-failure) [[ "$status" == 37 ]] ;;
    wrong-install|broken-cache) [[ "$status" == 2 ]] ;;
    setup-failure) [[ "$status" == 41 ]] ;;
  esac
  [[ "$KANI_PIN_CASE" != cached ]] || [[ ! -e "$AURA_KANI_ROOT/install-args" ]]
  printf 'Kani pinned installer fixture %s PASS\n' "$KANI_PIN_CASE"
done
