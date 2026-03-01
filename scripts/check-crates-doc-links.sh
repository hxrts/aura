#!/usr/bin/env bash
# Validate docs link integrity.
#
# Checks:
# 1. All markdown links in crates/ that reference docs/ resolve to existing files
# 2. docs/000_project_overview.md contains links to all docs/*.md files
#    (except itself and SUMMARY.md)
# 3. No links in docs/ or crates/ reference work/ (scratch directory)

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

# Check that docs/000_project_overview.md links to all docs files
overview="$docs_root/000_project_overview.md"
if [[ ! -f "$overview" ]]; then
  echo "error: $overview not found" >&2
  exit 2
fi

# Extract all markdown link targets from the overview
overview_links=$(perl -ne 'while (/\[[^\]]+\]\(([^)#]+)/g) { print "$1\n"; }' "$overview" | sort -u)

overview_missing=0
while IFS= read -r -d '' doc_file; do
  doc_name="$(basename "$doc_file")"

  # Skip self and SUMMARY.md
  case "$doc_name" in
    000_project_overview.md|SUMMARY.md) continue ;;
  esac

  # Check if this file is linked in the overview
  if ! echo "$overview_links" | grep -qF "$doc_name"; then
    overview_missing=$((overview_missing + 1))
    echo "missing from 000_project_overview.md: $doc_name"
  fi
done < <(find "$docs_root" -maxdepth 1 -name '*.md' -print0 | sort -z)

if [[ "$overview_missing" -gt 0 ]]; then
  echo ""
  echo "000_project_overview.md is missing links to $overview_missing doc file(s)"
  exit 1
fi

echo "000_project_overview.md links to all docs files"

# Check for links to work/ (scratch directory)
work_links=0
while IFS= read -r match; do
  [[ -z "$match" ]] && continue
  work_links=$((work_links + 1))
  echo "link to work/ found: $match"
done < <(rg --no-heading -n '\[[^\]]+\]\([^)]*work/' docs crates 2>/dev/null || true)

if [[ "$work_links" -gt 0 ]]; then
  echo ""
  echo "found $work_links link(s) to work/ directory (scratch files should not be referenced)"
  exit 1
fi

echo "no links to work/ directory found"
