#!/usr/bin/env bash
# Validate docs link integrity.
#
# Checks:
# 1. All markdown links in non-gitignored directories that reference docs/
#    resolve to existing files. Directories checked:
#    - crates/, docs/, scripts/, tests/, examples/, scenarios/, .github/
#    - verification/ (Quint/Lean comment patterns)
#    - AGENTS.md, CLAUDE.md at root
#    - .claude/skills/ (skipped in CI since .claude/ is gitignored)
# 2. docs/000_project_overview.md contains links to all docs/*.md files
#    (except itself and SUMMARY.md)
# 3. No links in checked directories reference work/ (scratch directory)
#
# Flags:
#   --fix      Fix broken links by finding matching files and replacing in-place
#   --dry-run  Show what --fix would change without modifying files
#
# Note: .claude/skills/ is skipped in CI (detected via CI or GITHUB_ACTIONS env vars)
# because .claude/ is gitignored.

set -euo pipefail

# Parse flags
FIX_MODE=false
DRY_RUN=false
for arg in "$@"; do
  case "$arg" in
    --fix) FIX_MODE=true ;;
    --dry-run) DRY_RUN=true ;;
    --help|-h)
      echo "Usage: $0 [--fix] [--dry-run]"
      echo ""
      echo "Flags:"
      echo "  --fix      Fix broken links by finding matching files and replacing in-place"
      echo "  --dry-run  Show what --fix would change without modifying files"
      exit 0
      ;;
  esac
done

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

# Detect CI environment
IN_CI="${CI:-${GITHUB_ACTIONS:-}}"

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
declare -a broken_links=()

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
    # Store for potential fixing: src_file|target|resolved_basename
    resolved_basename="$(basename "$resolved")"
    broken_links+=("$src_file|$target|$resolved_basename")
  fi
done < <(
  # Check markdown files in crates/
  while IFS= read -r -d '' file; do
    perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
  done < <(rg -l -0 --pcre2 '\[[^\]]+\]\([^)]*docs/' crates)

  # Check markdown files in docs/ (internal cross-references)
  while IFS= read -r -d '' file; do
    perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
  done < <(rg -l -0 --pcre2 '\[[^\]]+\]\([^)]*docs/' docs)

  # Check markdown files in tests/, examples/, scenarios/, .github/
  for dir in tests examples scenarios .github; do
    if [[ -d "$ROOT/$dir" ]]; then
      while IFS= read -r -d '' file; do
        perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
      done < <(rg -l -0 --pcre2 '\[[^\]]+\]\([^)]*docs/' "$dir" 2>/dev/null || true)
    fi
  done

  # Check scripts/ for docs references in comments or strings
  # Match patterns like "docs/..." in any context
  if [[ -d "$ROOT/scripts" ]]; then
    while IFS= read -r -d '' file; do
      perl -ne 'while (/(docs\/[0-9]+_[^\s,)\"'\'']+\.md)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
    done < <(rg -l -0 'docs/' scripts)
  fi

  # Check verification/ (Quint and Lean files with docs/ references in comments)
  # Match patterns like "See: docs/..." or "See docs/..." or "File: docs/..."
  while IFS= read -r -d '' file; do
    perl -ne 'while (/(?:See:?\s*|File:\s*)(docs\/[^\s,)]+)/g) { print "$ARGV\t$.\t$1\n"; }' "$file"
  done < <(rg -l -0 'docs/' verification)

  # Check AGENTS.md and CLAUDE.md at root (use relative paths)
  for root_file in AGENTS.md CLAUDE.md; do
    if [[ -f "$ROOT/$root_file" ]]; then
      perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$root_file"
    fi
  done

  # Check .claude/skills/ (skip in CI since .claude/ is gitignored)
  # Use relative paths by stripping ROOT prefix
  if [[ -z "$IN_CI" && -d "$ROOT/.claude/skills" ]]; then
    while IFS= read -r -d '' file; do
      rel_file="${file#$ROOT/}"
      perl -ne 'while (/\[[^\]]+\]\(([^)]+)\)/g) { print "$ARGV\t$.\t$1\n"; }' "$rel_file"
    done < <(find "$ROOT/.claude/skills" -name '*.md' -print0 2>/dev/null)
  fi
)

# Fix mode: attempt to repair broken links
if [[ "$FIX_MODE" == true || "$DRY_RUN" == true ]] && [[ "${#broken_links[@]}" -gt 0 ]]; then
  echo ""
  if [[ "$DRY_RUN" == true ]]; then
    echo "=== DRY RUN: showing proposed fixes ==="
  else
    echo "=== Fixing broken links ==="
  fi

  # Build a map of doc basenames (without number prefix) to actual filenames
  declare -A doc_map=()
  while IFS= read -r -d '' doc_file; do
    doc_name="$(basename "$doc_file")"
    # Extract the name part after the number prefix (e.g., "103_journal.md" -> "journal")
    if [[ "$doc_name" =~ ^[0-9]+_(.+)\.md$ ]]; then
      name_part="${BASH_REMATCH[1]}"
      doc_map["$name_part"]="$doc_name"
    fi
  done < <(find "$docs_root" -maxdepth 1 -name '*.md' -print0 2>/dev/null)

  fixed_count=0
  declare -A file_replacements
  declare -A affected_files

  for entry in "${broken_links[@]}"; do
    src_file="${entry%%|*}"
    rest="${entry#*|}"
    old_target="${rest%%|*}"
    old_basename="${rest##*|}"

    # Extract the name part from the old basename
    if [[ "$old_basename" =~ ^[0-9]+_(.+)\.md$ ]]; then
      name_part="${BASH_REMATCH[1]}"
      if [[ -n "${doc_map[$name_part]:-}" ]]; then
        new_basename="${doc_map[$name_part]}"
        if [[ "$old_basename" != "$new_basename" ]]; then
          # Compute the new target by replacing the basename
          new_target="${old_target/$old_basename/$new_basename}"

          if [[ "$DRY_RUN" == true ]]; then
            echo "  $src_file: $old_target -> $new_target"
            affected_files[$src_file]=1
          else
            # Accumulate replacements per file
            if [[ -z "${file_replacements[$src_file]:-}" ]]; then
              file_replacements[$src_file]="s|$old_basename|$new_basename|g"
            else
              file_replacements[$src_file]="${file_replacements[$src_file]}; s|$old_basename|$new_basename|g"
            fi
          fi
          fixed_count=$((fixed_count + 1))
        fi
      else
        echo "  warning: no match found for $old_basename in $src_file"
      fi
    fi
  done

  # Apply replacements
  if [[ "$DRY_RUN" != true ]]; then
    num_files="${#file_replacements[@]}"
    if [[ "$num_files" -gt 0 ]]; then
      for src_file in "${!file_replacements[@]}"; do
        [[ -z "$src_file" ]] && continue
        sed_expr="${file_replacements[$src_file]}"
        if [[ -f "$src_file" ]]; then
          # Use temp file approach for reliable in-place editing
          tmp_file="$(mktemp)"
          sed -e "$sed_expr" "$src_file" > "$tmp_file" && mv "$tmp_file" "$src_file"
          echo "  fixed: $src_file"
        else
          echo "  warning: file not found: $src_file"
        fi
      done
    fi
  fi

  echo ""
  if [[ "$DRY_RUN" == true ]]; then
    echo "would fix $fixed_count link(s) in ${#affected_files[@]} file(s)"
    echo ""
    echo "Run with --fix to apply changes"
  else
    echo "fixed $fixed_count link(s) in ${#file_replacements[@]} file(s)"
  fi
fi

if [[ "$missing" -gt 0 ]] && [[ "$FIX_MODE" != true ]]; then
  echo ""
  echo "checked $checked docs link(s); found $missing missing target(s)"
  if [[ "$DRY_RUN" != true ]]; then
    echo "hint: run with --dry-run to see proposed fixes, or --fix to apply them"
  fi
  exit 1
fi

if [[ "$FIX_MODE" == true ]]; then
  echo ""
  echo "checked $checked docs link(s); fixed $missing broken link(s)"
else
  echo "checked $checked docs link(s); all targets exist"
fi

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

# Check that docs/SUMMARY.md links to all docs files
summary="$docs_root/SUMMARY.md"
if [[ ! -f "$summary" ]]; then
  echo "error: $summary not found" >&2
  exit 2
fi

# Extract all markdown link targets from SUMMARY.md
summary_links=$(perl -ne 'while (/\[[^\]]+\]\(([^)#]+)/g) { print "$1\n"; }' "$summary" | sort -u)

summary_missing=0
while IFS= read -r -d '' doc_file; do
  doc_name="$(basename "$doc_file")"

  # Skip SUMMARY.md itself
  case "$doc_name" in
    SUMMARY.md) continue ;;
  esac

  # Check if this file is linked in SUMMARY.md
  if ! echo "$summary_links" | grep -qF "$doc_name"; then
    summary_missing=$((summary_missing + 1))
    echo "missing from SUMMARY.md: $doc_name"
  fi
done < <(find "$docs_root" -maxdepth 1 -name '*.md' -print0 | sort -z)

if [[ "$summary_missing" -gt 0 ]]; then
  echo ""
  echo "SUMMARY.md is missing links to $summary_missing doc file(s)"
  exit 1
fi

echo "SUMMARY.md links to all docs files"

# Check for links to work/ (scratch directory)
work_links=0

# Build search paths (all non-gitignored directories)
search_paths=(docs crates verification scripts tests examples scenarios .github)
for root_file in AGENTS.md CLAUDE.md; do
  [[ -f "$ROOT/$root_file" ]] && search_paths+=("$root_file")
done
if [[ -z "$IN_CI" && -d "$ROOT/.claude/skills" ]]; then
  search_paths+=(.claude/skills)
fi

# Filter to existing paths only
existing_paths=()
for p in "${search_paths[@]}"; do
  [[ -e "$ROOT/$p" || -e "$p" ]] && existing_paths+=("$p")
done

while IFS= read -r match; do
  [[ -z "$match" ]] && continue
  work_links=$((work_links + 1))
  echo "link to work/ found: $match"
done < <(rg --no-heading -n '\[[^\]]+\]\([^)]*work/' "${existing_paths[@]}" 2>/dev/null || true)

if [[ "$work_links" -gt 0 ]]; then
  echo ""
  echo "found $work_links link(s) to work/ directory (scratch files should not be referenced)"
  exit 1
fi

echo "no links to work/ directory found"
