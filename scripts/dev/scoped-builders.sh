#!/usr/bin/env bash
# Sourced helper: list builder or consumer processes that belong to one Aura
# checkout, so concurrent builds in sibling worktrees do not block each other.
# A process belongs to ROOT when its arguments name ROOT/target/ (rustc
# out-dirs, test binaries, harness binaries) or when the nearest enclosing
# checkout (directory holding .git) of its working directory is ROOT.

aura_checkout_of() {
  local dir="$1"
  while [[ -n "$dir" && "$dir" != / ]]; do
    if [[ -e "$dir/.git" ]]; then printf '%s\n' "$dir"; return 0; fi
    dir="${dir%/*}"
  done
  return 1
}

# aura_scoped_processes ROOT NAME_REGEX [EXCLUDED_PID]
# Prints "name(pid) " for each matching process owned by ROOT.
aura_scoped_processes() {
  local root="$1" pattern="$2" own="${3:-}" pid name args cwd owner
  while read -r pid name args; do
    # ps truncates comm, so the name is the first argument's basename.
    [[ "$pid" =~ ^[0-9]+$ && "$pid" != "$own" ]] || continue
    name="${name##*/}"
    [[ "$name" =~ ^($pattern)$ ]] || continue
    if [[ " $args " == *"$root/target/"* ]]; then
      printf '%s(%s) ' "$name" "$pid"
      continue
    fi
    cwd="$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -n 1 || true)"
    [[ -n "$cwd" ]] || continue
    owner="$(aura_checkout_of "$cwd" || true)"
    if [[ "$owner" == "$root" ]]; then printf '%s(%s) ' "$name" "$pid"; fi
  done < <(ps -axo pid=,args= 2>/dev/null)
}
