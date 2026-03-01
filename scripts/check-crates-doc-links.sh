#!/usr/bin/env bash
# Validate that markdown-style docs links in crates/ resolve to existing files.
#
# Scope:
# - Scans all files under crates/
# - Extracts markdown links: [label](target)
# - Filters targets that resolve into docs/
# - Fails if any referenced docs file does not exist

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 2
fi

docs_root="$ROOT/docs"
if [[ ! -d "$docs_root" ]]; then
  echo "error: docs directory not found at $docs_root" >&2
  exit 2
fi

normalize_path() {
  local path="$1"
  local -a parts stack
  IFS='/' read -r -a parts <<< "$path"
  stack=()

  for part in "${parts[@]}"; do
    case "$part" in
      ""|".") continue ;;
      "..")
        if [[ "${#stack[@]}" -gt 0 ]]; then
          unset "stack[${#stack[@]}-1]"
        fi
        ;;
      *)
        stack+=("$part")
        ;;
    esac
  done

  local out=""
  for part in "${stack[@]}"; do
    out+="/$part"
  done
  if [[ -z "$out" ]]; then
    out="/"
  fi
  printf '%s\n' "$out"
}

checked=0
missing=0

while IFS= read -r record; do
  [[ -z "$record" ]] && continue

  src_file="${record%%$'\t'*}"
  rest="${record#*$'\t'}"
  src_line="${rest%%$'\t'*}"
  raw_target="${rest#*$'\t'}"

  target="$(printf '%s' "$raw_target" | sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
  # Strip optional markdown title: [x](path "title")
  target="${target%%[[:space:]]*}"
  # Strip angle brackets: [x](<path>)
  if [[ "$target" == \<*\> ]]; then
    target="${target#<}"
    target="${target%>}"
  fi

  [[ -z "$target" ]] && continue
  case "$target" in
    http://*|https://*|mailto:*|\#*) continue ;;
    *) ;;
  esac

  path_part="${target%%#*}"
  [[ -z "$path_part" ]] && continue

  # Only validate links that point to docs/.
  if [[ "$path_part" != *docs/* && "$path_part" != docs/* && "$path_part" != /docs/* ]]; then
    continue
  fi

  if [[ "$path_part" == /docs/* ]]; then
    resolved="$(normalize_path "$ROOT/${path_part#/}")"
  elif [[ "$path_part" == docs/* ]]; then
    resolved="$(normalize_path "$ROOT/$path_part")"
  else
    resolved="$(normalize_path "$ROOT/$(dirname "$src_file")/$path_part")"
  fi

  checked=$((checked + 1))
  # Enforce that docs links actually resolve under repo docs/.
  case "$resolved" in
    "$docs_root"/*) ;;
    *)
      missing=$((missing + 1))
      echo "invalid docs link: $src_file:$src_line -> $target (resolved outside docs/: ${resolved#$ROOT/})"
      continue
      ;;
  esac

  if [[ ! -f "$resolved" ]]; then
    missing=$((missing + 1))
    echo "missing docs link: $src_file:$src_line -> $target (resolved: ${resolved#$ROOT/})"
  fi
done < <(
  while IFS= read -r -d '' file; do
    perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
  done < <(rg -l -0 --pcre2 '\[[^\]]+\]\([^)]*docs/' crates)
)

if [[ "$missing" -gt 0 ]]; then
  echo ""
  echo "checked $checked docs link(s); found $missing missing target(s)"
  exit 1
fi

echo "checked $checked docs link(s); all targets exist"
