#!/usr/bin/env bash

aura_web_redirect_logs() {
  local repo_root="$1"
  local default_log="$2"

  if [[ ! -t 1 ]]; then
    return 0
  fi

  if [[ "${AURA_WEB_ALLOW_TTY:-0}" == "1" ]]; then
    return 0
  fi

  if [[ -n "${AURA_WEB_LOG_REDIRECTED:-}" ]]; then
    return 0
  fi

  export AURA_WEB_LOG_REDIRECTED=1
  export AURA_WEB_LOG_FILE="${AURA_WEB_LOG_FILE:-$default_log}"
  mkdir -p "$(dirname "$AURA_WEB_LOG_FILE")"
  : >"$AURA_WEB_LOG_FILE"
  exec >>"$AURA_WEB_LOG_FILE" 2>&1
}
