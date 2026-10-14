#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=$(cd "$here/../../.." && pwd -P)
source "$here/render-single-host-config.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
auth=$(printf 'a%.0s' {1..64})
local_config="$tmp/local.toml"
peer_config="$tmp/peer.toml"
render_single_host_config "$root/configs/harness/lan-host-a.toml" "$local_config" local "$auth"
render_single_host_config "$root/configs/harness/lan-host-b.toml" "$peer_config" peer "$auth"
for config in "$local_config" "$peer_config"; do
  [[ $(rg -c '^\[\[instances\]\]$' "$config") == 1 ]]
  [[ $(rg -c '^mode = "local"$' "$config") == 1 ]]
  [[ $(rg -c '^fixed_ports = true$' "$config") == 1 ]]
  [[ $(rg -c '^  "AURA_BOOTSTRAP_BROKER_AUTH_TOKEN=' "$config") == 1 ]]
  [[ $(rg -c '^  "AURA_BOOTSTRAP_BROKER_INVITATION_TOKEN=' "$config") == 1 ]]
  if rg -q 'browser|web|tui2|:53101|:4173|__HOST_ADDR__' "$config"; then
    echo "diagnostic config contains an excluded instance or unresolved address" >&2
    exit 1
  fi
done
rg -q '^id = "alex-tui"$' "$local_config"
rg -q '^id = "barbara-tui"$' "$peer_config"
rg -q '^bind_address = "127.0.0.1:43101"$' "$local_config"
rg -q '^bind_address = "127.0.0.1:43201"$' "$peer_config"
rg -q '^port = 19433$' "$local_config"
rg -q '^port = 19434$' "$peer_config"
rg -q 'AURA_BOOTSTRAP_BROKER_BIND=127.0.0.1:43190' "$local_config"
rg -q 'AURA_BOOTSTRAP_BROKER_URL=http://127.0.0.1:43190' "$peer_config"
[[ $(stat -f %Lp "$local_config") == 600 && $(stat -f %Lp "$peer_config") == 600 ]]
# The LAN templates still include their browser instances for the final two-host run.
rg -q '^id = "alex-web"$' "$root/configs/harness/lan-host-a.toml"
rg -q '^id = "barbara-web"$' "$root/configs/harness/lan-host-b.toml"
echo 'single-host native-only config: pass'
