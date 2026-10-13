#!/usr/bin/env bash
# Sourced by build-budget; JSON is data, never sourced or evaluated.
aura_receipt_update() (
  # Each writer owns a short-lived subshell lock. Its EXIT trap is independent
  # of the build owner's signal/cleanup traps; atomic rename alone would lose
  # concurrent phase and cleanup updates.
  local temporary='' write_lock="${AURA_BUILD_RECEIPT}.write-lock" holder attempt acquired=0
  cleanup_writer() {
    [[ -z "$temporary" ]] || rm -f "$temporary"
    rm -f "$write_lock/pid"
    rmdir "$write_lock" 2>/dev/null || true
  }
  for attempt in $(seq 1 200); do
    [[ ! -L "$write_lock" ]] || { echo 'build-receipt: writer lock is a symlink' >&2; return 79; }
    if mkdir "$write_lock" 2>/dev/null; then acquired=1; break; fi
    holder="$(cat "$write_lock/pid" 2>/dev/null || true)"
    if [[ "$holder" =~ ^[0-9]+$ ]] && ! kill -0 "$holder" 2>/dev/null; then
      # Never reclaim here: competing reclaimers could remove a new owner's
      # lock. Each build has a fresh receipt directory, so refuse this run.
      echo 'build-receipt: stopped writer lock retained' >&2
      return 79
    fi
    sleep 0.05
  done
  if (( acquired == 0 )); then echo 'build-receipt: receipt writer lock remained busy' >&2; return 79; fi
  # Bash skips a subshell EXIT trap when this function is called by an outer
  # EXIT trap (the build and prune finalizers both do that). Keep the trap for
  # abnormal exit, but release the writer explicitly on ordinary completion.
  trap cleanup_writer EXIT
  printf '%s\n' "${BASHPID:-$$}" > "$write_lock/pid"
  if ! temporary="$(mktemp "${AURA_BUILD_RECEIPT}.XXXXXX")"; then
    cleanup_writer
    trap - EXIT
    return 1
  fi
  local status=0
  if jq "$@" "$AURA_BUILD_RECEIPT" > "$temporary"; then
    mv "$temporary" "$AURA_BUILD_RECEIPT" || status=$?
  else
    status=1
  fi
  cleanup_writer
  trap - EXIT
  return "$status"
)

aura_receipt_cargo_metadata() {
  receipt_packages='' receipt_features='' receipt_test_target='' receipt_test_filter=''
  [[ "${1:-}" == cargo ]] || return 0
  shift
  local subcommand="${1:-}" option value target_ambiguous=0 filter_safe=1
  case "$subcommand" in test|check|build|clippy) shift ;; *) return 0 ;; esac
  while (( $# > 0 )); do
    option="$1"; shift
    [[ "$option" != -- ]] || break
    case "$option" in
      -p|--package|-F|--features|--test|--bin|--example)
        (( $# > 0 )) || { filter_safe=0; break; }
        value="$1"; shift
        case "$option" in
          -p|--package) receipt_packages+="${receipt_packages:+,}$value" ;;
          -F|--features) receipt_features+="${receipt_features:+,}$value" ;;
          --test|--bin|--example)
            if [[ -n "$receipt_test_target" ]]; then target_ambiguous=1; fi
            receipt_test_target="${option#--}:$value" ;;
        esac ;;
      --package=*|-p?*) value="${option#*=}"; [[ "$option" != -p* ]] || value="${option#-p}"; receipt_packages+="${receipt_packages:+,}$value" ;;
      --features=*|-F?*) value="${option#*=}"; [[ "$option" != -F* ]] || value="${option#-F}"; receipt_features+="${receipt_features:+,}$value" ;;
      --lib|--doc|--tests|--bins|--examples)
        if [[ -n "$receipt_test_target" ]]; then target_ambiguous=1; fi
        receipt_test_target="${option#--}" ;;
      --locked|--offline|--frozen|--quiet|-q|--release|--no-default-features|--workspace|--all-targets) ;;
      --all-features) receipt_features=all ;;
      --manifest-path|--target|--target-dir|--profile|--jobs|-j|--color|--message-format|--config|-Z)
        (( $# > 0 )) || { filter_safe=0; break; }
        shift ;;
      --manifest-path=*|--target=*|--target-dir=*|--profile=*|--jobs=*|-j?*|--color=*|--message-format=*|--config=*|-Z?*) ;;
      -*) filter_safe=0 ;;
      *)
        if [[ "$subcommand" == test && -z "$receipt_test_filter" && "$filter_safe" == 1 ]]; then
          receipt_test_filter="$option"
        else
          filter_safe=0
        fi ;;
    esac
  done
  [[ "$subcommand" == test && "$target_ambiguous" == 0 ]] || receipt_test_target=''
  [[ "$subcommand" == test && "$filter_safe" == 1 ]] || receipt_test_filter=''
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
  argv="$(jq -cn --args '$ARGS.positional' -- "$@")"
  aura_receipt_cargo_metadata "$@"
  jq -n --arg root "$checkout" --arg lane "$build_lane" \
    --arg revision "$revision" --arg fingerprint "$fingerprint" \
    --arg identity "$identity" --argjson pid "$$" --argjson argv "$argv" \
    --arg packages "${AURA_BUILD_PACKAGES:-$receipt_packages}" --arg features "${AURA_BUILD_FEATURES:-$receipt_features}" \
    --arg test_target "$receipt_test_target" --arg test_filter "$receipt_test_filter" \
    --arg inventory "${AURA_BUILD_TEST_INVENTORY:-}" --arg started "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{schema:1,root:$root,lane:$lane,source:{revision:$revision, fingerprint:$fingerprint},
      metadata:{packages:(if $packages=="" then null else $packages end),
        features:(if $features=="" then null else $features end),
        test_target:(if $test_target=="" then null else $test_target end),
        test_filter:(if $test_filter=="" then null else $test_filter end),
        test_inventory:(if $inventory=="" then null else $inventory end)},
      wrapper:{pid:$pid,start_identity:$identity},child:null,command_argv:$argv,
      started_utc:$started,finished_utc:null,current_phase:"admission",active_phase:null,phases:[],
      command_exit_status:null,cleanup:null,cleanup_events:[],final_exit_status:null}' > "$AURA_BUILD_RECEIPT"
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

# Native cleanup outcomes and size observations are separate facts. A foreign
# fixture/root must never append to its caller's receipt.
aura_receipt_record_cleanup() {
  local event_root="$1" operation="$2" lane="$3" outcome="$4" reason="$5"
  local status="$6" before="$7" after="$8" free_before="$9" free_after="${10}"
  [[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || return 0
  jq -e --arg root "$event_root" '.root==$root' "$AURA_BUILD_RECEIPT" >/dev/null || return 0
  aura_receipt_update --arg stage "${AURA_BUILD_CLEANUP_STAGE:-command}" \
    --arg operation "$operation" --arg lane "$lane" --arg outcome "$outcome" --arg reason "$reason" \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --argjson status "$status" \
    --argjson before "$before" --argjson after "$after" \
    --argjson cap "${cap_kib:-null}" \
    --argjson free_before "$free_before" --argjson free_after "$free_after" \
    '.cleanup_events += [{stage:$stage,operation:$operation,lane:$lane,
      outcome:$outcome,reason:$reason,exit_status:$status,at_utc:$at,
      size_scope:(if $operation=="guarded-lane" then "lane" else "target" end),
      before_kib:$before,after_kib:$after,
      target_above_soft_cap:(if $operation=="sweep" and $after!=null and $cap!=null
        then $after>$cap else null end),
      reclaimed_kib:(if $before!=null and $after!=null and $before>=$after
        then $before-$after else null end),free_before_kib:$free_before,free_after_kib:$free_after}]'
}
