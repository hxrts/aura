#!/usr/bin/env bash
# Sourced by build-budget; JSON is data, never sourced or evaluated.
aura_receipt_update() {
  local temporary
  temporary="$(mktemp "${AURA_BUILD_RECEIPT}.XXXXXX")" || return
  if jq "$@" "$AURA_BUILD_RECEIPT" > "$temporary"; then
    mv "$temporary" "$AURA_BUILD_RECEIPT"
  else
    rm -f "$temporary"
    return 1
  fi
}

aura_receipt_init() {
  local checkout="$1" build_lane="$2" directory fingerprint revision identity argv path
  shift 2
  command -v jq >/dev/null || { echo 'build-budget: jq required for receipts' >&2; return 2; }
  mkdir -p "$checkout/artifacts/disk-budget/receipts"
  directory="$(mktemp -d "$checkout/artifacts/disk-budget/receipts/run.XXXXXX")"
  export AURA_BUILD_RECEIPT="$directory/receipt.json"
  revision="$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
  fingerprint=''
  if [[ -n "$revision" ]]; then
    # Include new nonignored modules, as well as tracked/staged edits, deletions
    # and executable-bit changes. Ignored caches and work notes are excluded.
    fingerprint="$(
      {
        printf '%s\n' "$revision"
        git -C "$checkout" diff HEAD --binary
        while IFS= read -r -d '' path; do
          printf '%s\0' "$path"
          if [[ -L "$checkout/$path" ]]; then readlink "$checkout/$path";
          elif [[ -f "$checkout/$path" ]]; then sha256sum "$checkout/$path";
          else printf 'missing\n'; fi
        done < <(git -C "$checkout" ls-files --others --exclude-standard -z)
      } | sha256sum | awk '{print $1}'
    )"
  fi
  identity="$(ps -p "$$" -o lstart= 2>/dev/null || true)"
  argv="$(jq -cn --args '$ARGS.positional' "$@")"
  jq -n --arg root "$checkout" --arg lane "$build_lane" \
    --arg revision "$revision" --arg fingerprint "$fingerprint" \
    --arg identity "$identity" --argjson pid "$$" --argjson argv "$argv" \
    --arg packages "${AURA_BUILD_PACKAGES:-}" --arg features "${AURA_BUILD_FEATURES:-}" \
    --arg inventory "${AURA_BUILD_TEST_INVENTORY:-}" --arg started "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{schema:1,root:$root,lane:$lane,source:{revision:$revision, fingerprint:$fingerprint},
      metadata:{packages:(if $packages=="" then null else $packages end),
        features:(if $features=="" then null else $features end),
        test_inventory:(if $inventory=="" then null else $inventory end)},
      wrapper:{pid:$pid,start_identity:$identity},child:null,command_argv:$argv,
      started_utc:$started,finished_utc:null,current_phase:"admission",active_phase:null,phases:[],
      command_exit_status:null,cleanup:null,final_exit_status:null}' > "$AURA_BUILD_RECEIPT"
  printf 'Build receipt: %s\n' "$AURA_BUILD_RECEIPT"
}

aura_receipt_finish() {
  local exit_status="$1"
  [[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || return 0
  aura_receipt_update --argjson status "$exit_status" \
    --arg finished "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '.final_exit_status=$status | .finished_utc=$finished |
      .last_observed_phase=.current_phase | .current_phase="finished"'
  printf 'Build complete: exit=%s receipt=%s\n' "$exit_status" "$AURA_BUILD_RECEIPT"
}
