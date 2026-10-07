#!/usr/bin/env bash
# Run the multi-agent home-invitation accept-chain targets under a reduced
# 1.5 MiB thread stack so async state-machine growth on the
# accept -> materialize -> channel-join -> commit_channel_membership chain
# fails here instead of at the default 2 MiB limit.
set -euo pipefail

stack_bytes="${AURA_ACCEPT_CHAIN_STACK_BYTES:-1572864}"

RUST_MIN_STACK="$stack_bytes" cargo test -p hxrts-aura-agent \
  --test home_two_member_moderation \
  --test home_moderation_workflows \
  --test home_invitation_readiness_hook \
  --no-fail-fast
