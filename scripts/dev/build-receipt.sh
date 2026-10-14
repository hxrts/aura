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

# Only Rust test identifiers and source locations may leave the protected
# output log. Values, assertion operands and arbitrary diagnostic lines stay
# in that log, even when a failing test prints them before its terminal line.
aura_receipt_safe_failure_identity() {
  local name='' location='' log=''
  [[ -f "$AURA_BUILD_RECEIPT" ]] || return 0
  name="$(jq -r '[.phases[]?.failed_tests[]?][0] // empty' "$AURA_BUILD_RECEIPT")"
  log="$(jq -r '[.phases[]? | select(.failed_count>0) | .log][0] // .artifacts.command_log // empty' "$AURA_BUILD_RECEIPT")"
  [[ -f "$log" && ! -L "$log" ]] || return 0
  if [[ -z "$name" ]]; then
    name="$(sed -nE 's/^test ([A-Za-z_][A-Za-z0-9_:]*) \.\.\. FAILED.*/\1/p' "$log" | sed -n '1p')"
  fi
  [[ "$name" =~ ^[A-Za-z_][A-Za-z0-9_:]{0,95}$ ]] || return 0
  location="$(sed -nE 's/.*panicked at ([A-Za-z0-9_./-]+\.rs):([0-9]+)(:[0-9]+)?.*/\1:\2/p' "$log" | sed -n '1p')"
  [[ "$location" =~ ^[A-Za-z0-9_./-]+\.rs:[0-9]+$ && ${#location} -le 100 ]] || location=''
  printf 'test %s failed%s; details in protected log\n' "$name" "${location:+ at $location}"
}

aura_receipt_source_fingerprint() {
  local checkout="$1" revision path
  revision="$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
  [[ -n "$revision" ]] || return 1
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
}

aura_receipt_init() {
  local checkout="$1" build_lane="$2" directory fingerprint revision identity argv path command_log
  shift 2
  command -v jq >/dev/null || { echo 'build-budget: jq required for receipts' >&2; return 2; }
  mkdir -p "$checkout/artifacts/disk-budget/receipts"
  directory="$(mktemp -d "$checkout/artifacts/disk-budget/receipts/run.XXXXXX")"
  export AURA_BUILD_RECEIPT="$directory/receipt.json"
  command_log="$(mktemp "$directory/command.XXXXXX.log")"
  export AURA_BUILD_COMMAND_LOG="$command_log"
  revision="$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)"
  # Include tracked edits and new nonignored modules; cache artifacts are ignored.
  fingerprint="$(aura_receipt_source_fingerprint "$checkout" || true)"
  identity="$(ps -p "$$" -o lstart= 2>/dev/null || true)"
  argv="$(jq -cn --args '$ARGS.positional' -- "$@")"
  aura_receipt_cargo_metadata "$@"
  jq -n --arg root "$checkout" --arg lane "$build_lane" \
    --arg revision "$revision" --arg fingerprint "$fingerprint" \
    --arg identity "$identity" --argjson pid "$$" --argjson argv "$argv" \
    --arg packages "${AURA_BUILD_PACKAGES:-$receipt_packages}" --arg features "${AURA_BUILD_FEATURES:-$receipt_features}" \
    --arg test_target "$receipt_test_target" --arg test_filter "$receipt_test_filter" \
    --arg inventory "${AURA_BUILD_TEST_INVENTORY:-}" --arg started "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg command_log "$command_log" \
    '{schema:2,root:$root,lane:$lane,source:{revision:$revision, fingerprint:$fingerprint},
      metadata:{packages:(if $packages=="" then null else $packages end),
        features:(if $features=="" then null else $features end),
        test_target:(if $test_target=="" then null else $test_target end),
        test_filter:(if $test_filter=="" then null else $test_filter end),
        test_inventory:(if $inventory=="" then null else $inventory end),
        phase_commands:[]},
      wrapper:{pid:$pid,start_identity:$identity},child:null,command_argv:$argv,
      started_utc:$started,finished_utc:null,current_phase:"admission",active_phase:null,phases:[],
      command_exit_status:null,cleanup:null,cleanup_events:[],final_exit_status:null,
      artifacts:{command_log:$command_log},failure_excerpt:null,notification:null}' > "$AURA_BUILD_RECEIPT"
  printf 'Build receipt: %s\n' "$AURA_BUILD_RECEIPT"
}

aura_receipt_finish() {
  local exit_status="$1" notification temporary finished safe_failure=''
  [[ -n "${AURA_BUILD_RECEIPT:-}" && -f "$AURA_BUILD_RECEIPT" ]] || return 0
  if (( exit_status != 0 )); then safe_failure="$(aura_receipt_safe_failure_identity)"; fi
  finished="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  notification="$(dirname "$AURA_BUILD_RECEIPT")/completion.json"
  aura_receipt_update --argjson status "$exit_status" \
    --arg finished "$finished" --arg notification "$notification" --arg safe_failure "$safe_failure" \
    '.final_exit_status=$status | .finished_utc=$finished |
      .last_observed_phase=.current_phase | .current_phase="finished" |
      .notification=$notification |
      .metadata |= (
        .packages=(.packages // ([.phase_commands[].packages | select(.!=null)] | unique |
          if length==1 then .[0] else null end)) |
        .features=(.features // ([.phase_commands[].features | select(.!=null)] | unique |
          if length==1 then .[0] else null end)) |
        .test_target=(.test_target // ([.phase_commands[].test_target | select(.!=null)] | unique |
          if length==1 then .[0] else null end)) |
        .test_filter=(.test_filter // ([.phase_commands[].test_filter | select(.!=null)] | unique |
          if length==1 then .[0] else null end)) |
        .test_inventory=(.test_inventory // ([.phase_commands[].expected_tests | select(.!=null)] | unique |
          if length==1 then .[0] else null end))) |
      .failure_excerpt=(if $status==0 then null else
        (if $safe_failure!="" then $safe_failure
         elif .active_phase!=null then "phase interrupted; details in protected log"
         elif .admission.admitted==false then "build admission refused; details in receipt"
         elif any(.phases[]?; .exit_status!=0) then "phase failed; details in protected log"
         elif .command_exit_status!=null and .command_exit_status!=0 then
           "command failed; details in protected log"
         else "build failed; details in receipt" end) end)'
  temporary="$(mktemp "${notification}.XXXXXX")"
  jq -n --arg receipt "$AURA_BUILD_RECEIPT" --arg finished "$finished" \
    --argjson status "$exit_status" \
    '{receipt:$receipt,finished_utc:$finished,exit_status:$status}' > "$temporary"
  mv "$temporary" "$notification"
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
