#!/usr/bin/env bash
# Find terminology slated for rename.
#
# Two-pass workflow:
#   Pass 1 (inventory): report matches only (no edits)
#   Pass 2 (rewrite): apply curated literal replacements constrained by allowlist + rules
#
# Usage:
#   ./scripts/dev/terminology.sh [--mode inventory|rewrite] [--json] [--check]
#   ./scripts/dev/terminology.sh --mode rewrite --allowlist FILE --rules FILE [--apply] [--check]
#
# Rewrite rules format (TSV):
#   term<TAB>from_literal<TAB>to_literal<TAB>path_glob
#
# Allowlist format:
#   one repo-relative file path per line
#   blank lines and # comments are ignored

set -euo pipefail

MODE="inventory"
JSON_OUTPUT=false
CHECK_MODE=false
APPLY_REWRITE=false
MAX_MATCHES_PER_TERM=200
ALLOWLIST_FILE=""
RULES_FILE=""

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) is required" >&2
  exit 2
fi

usage() {
  cat <<'USAGE'
Usage:
  scripts/dev/terminology.sh [options]

Options:
  --mode <inventory|rewrite>   Select mode (default: inventory)
  --json                       Emit machine-readable JSON (inventory mode only)
  --check                      Exit 1 when actionable matches are found
  --max <n>                    Max matches emitted per term in inventory output (default: 200)
  --allowlist <file>           Allowlisted file paths (required for rewrite mode)
  --rules <file>               Curated rewrite rules TSV (required for rewrite mode)
  --apply                      Apply rewrite rules (rewrite mode); default is dry-run
  -h, --help                   Show this help
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:-}"
      shift 2
      ;;
    --json)
      JSON_OUTPUT=true
      shift
      ;;
    --check)
      CHECK_MODE=true
      shift
      ;;
    --max)
      MAX_MATCHES_PER_TERM="${2:-}"
      shift 2
      ;;
    --allowlist)
      ALLOWLIST_FILE="${2:-}"
      shift 2
      ;;
    --rules)
      RULES_FILE="${2:-}"
      shift 2
      ;;
    --apply)
      APPLY_REWRITE=true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ "$MODE" != "inventory" && "$MODE" != "rewrite" ]]; then
  echo "error: --mode must be one of: inventory, rewrite" >&2
  exit 1
fi

if ! [[ "$MAX_MATCHES_PER_TERM" =~ ^[0-9]+$ ]]; then
  echo "error: --max must be a non-negative integer" >&2
  exit 1
fi

SEARCH_ROOTS=()
for d in docs crates configs scenarios examples verification; do
  [[ -d "$d" ]] && SEARCH_ROOTS+=("$d")
done
if [[ ${#SEARCH_ROOTS[@]} -eq 0 ]]; then
  echo "error: expected at least one known source directory (docs/crates/configs/scenarios/examples/verification)" >&2
  exit 1
fi

EXCLUDE_GLOBS=(
  "docs/book/**"
  "target/**"
  ".git/**"
  "work/**"
  "scripts/dev/terminology.sh"
)

RG_ARGS=(--line-number --with-filename --no-heading)
for g in "${EXCLUDE_GLOBS[@]}"; do
  RG_ARGS+=(--glob "!$g")
done

is_excluded_path() {
  local path="$1"
  case "$path" in
    docs/book/*|target/*|.git/*|work/*|scripts/dev/terminology.sh) return 0 ;;
    *) return 1 ;;
  esac
}

json_escape() {
  local s="$1"
  s="${s//\\/\\\\}"
  s="${s//\"/\\\"}"
  s="${s//$'\t'/\\t}"
  s="${s//$'\r'/\\r}"
  s="${s//$'\n'/\\n}"
  printf '%s' "$s"
}

TERM_NAMES=()
TERM_PATTERNS=()
TERM_CONTEXTS=()
TERM_SPECIAL_CASES=()

add_term() {
  TERM_NAMES+=("$1")
  TERM_PATTERNS+=("$2")
  TERM_CONTEXTS+=("$3")
  TERM_SPECIAL_CASES+=("${4:-}")
}

# Old -> new mapping inventory (Phase 1)
add_term "PublicGoodSpace" "PublicGoodSpace|public_good_space|PUBLIC_GOOD_SPACE" "Storage terminology"
add_term "Owner role" "ResidentRole::Owner|\\brole\\b.{0,32}\\bowner\\b|\\bowner\\b.{0,32}\\brole\\b" "Context-aware role usage" "Includes policy strings like role(\"owner\"); review with auth-policy owners."
add_term "Admin/Steward role" "ResidentRole::Admin|Steward|steward" "Context-aware role usage" "High-volume term; triage docs prose vs type/enum renames."
add_term "Resident role" "ResidentRole::Resident" "Role enum usage"
add_term "Interior access" "TraversalDepth::Interior|::Interior|\"interior\"|_interior\\b|\\binterior_|Interior\\b|\\binterior (depth|access)\\b" "Context-aware access usage" "Excludes 'interior mutability' (Rust term) - manual review needed."
add_term "Frontage access" "TraversalDepth::Frontage|::Frontage|\"frontage\"|_frontage\\b|\\bfrontage_|Frontage\\b|\\bfrontage (depth|access)\\b" "Context-aware access usage"
add_term "Street access" "TraversalDepth::Street|::Street|\"street\"|_street\\b|\\bstreet_|Street\\b|\\bstreet(-level| depth| view)\\b" "Context-aware access usage"
add_term "HomePeer" "HomePeer|home_peer|HOME_PEER" "Authority peer terminology"
add_term "NeighborhoodPeer" "NeighborhoodPeer|neighborhood_peer|NEIGHBORHOOD_PEER" "Authority peer terminology"
add_term "Adjacency" "Adjacency|adjacency|_adjacency|adjacency_|ADJACENCY" "Context-aware topology terminology" "May remain in graph-edge APIs; treat as manual review term."
add_term "TraversalDepth" "TraversalDepth|traversal_depth|TRAVERSAL_DEPTH" "Access-level enum name"
add_term "Donation" "Donation|donation|_donation|donation_|DONATION" "Context-sensitive storage terminology" "Manual review required where 'donation' is user-facing copy vs protocol field."
add_term "PinnedContent" "PinnedContent|pinned_content|PINNED_CONTENT" "Content naming terminology"

run_inventory_mode() {
  local total_matches=0
  local terms_with_matches=0

  if $JSON_OUTPUT; then
    printf '{\n'
    printf '  "mode": "inventory",\n'
    printf '  "terms": [\n'
  else
    printf 'Terminology Inventory (pass 1 - report only)\n'
    printf 'Search roots: %s\n' "${SEARCH_ROOTS[*]}"
  fi

  local i
  for ((i=0; i<${#TERM_NAMES[@]}; i++)); do
    local name="${TERM_NAMES[$i]}"
    local pattern="${TERM_PATTERNS[$i]}"
    local context="${TERM_CONTEXTS[$i]}"
    local special_case="${TERM_SPECIAL_CASES[$i]}"
    local -a matches=()
    local line
    while IFS= read -r line; do
      matches+=("$line")
    done < <(rg "${RG_ARGS[@]}" --regexp "$pattern" "${SEARCH_ROOTS[@]}" 2>/dev/null || true)

    local count="${#matches[@]}"
    total_matches=$((total_matches + count))
    if [[ "$count" -gt 0 ]]; then
      terms_with_matches=$((terms_with_matches + 1))
    fi

    if $JSON_OUTPUT; then
      printf '    {\n'
      printf '      "term": "%s",\n' "$(json_escape "$name")"
      printf '      "pattern": "%s",\n' "$(json_escape "$pattern")"
      printf '      "context": "%s",\n' "$(json_escape "$context")"
      printf '      "special_case": "%s",\n' "$(json_escape "$special_case")"
      printf '      "count": %d,\n' "$count"
      printf '      "matches": [\n'

      local emitted=0
      for line in "${matches[@]}"; do
        [[ "$emitted" -ge "$MAX_MATCHES_PER_TERM" ]] && break
        local file="${line%%:*}"
        local rem="${line#*:}"
        local line_no="${rem%%:*}"
        local text="${rem#*:}"
        printf '        {"file":"%s","line":%s,"text":"%s"}' \
          "$(json_escape "$file")" \
          "$(json_escape "$line_no")" \
          "$(json_escape "$text")"
        emitted=$((emitted + 1))
        if [[ "$emitted" -lt "$count" && "$emitted" -lt "$MAX_MATCHES_PER_TERM" ]]; then
          printf ','
        fi
        printf '\n'
      done
      printf '      ],\n'
      printf '      "truncated": %s\n' "$([[ "$count" -gt "$MAX_MATCHES_PER_TERM" ]] && echo "true" || echo "false")"
      printf '    }'
      if [[ "$i" -lt $((${#TERM_NAMES[@]} - 1)) ]]; then
        printf ','
      fi
      printf '\n'
    else
      printf '\n== %s ==\n' "$name"
      printf 'context: %s\n' "$context"
      if [[ -n "$special_case" ]]; then
        printf 'special-case: %s\n' "$special_case"
      fi
      printf 'pattern: %s\n' "$pattern"
      printf 'count: %d\n' "$count"
      if [[ "$count" -gt 0 ]]; then
        local emitted=0
        for line in "${matches[@]}"; do
          [[ "$emitted" -ge "$MAX_MATCHES_PER_TERM" ]] && break
          printf '%s\n' "$line"
          emitted=$((emitted + 1))
        done
        if [[ "$count" -gt "$MAX_MATCHES_PER_TERM" ]]; then
          printf '... truncated %d additional matches\n' "$((count - MAX_MATCHES_PER_TERM))"
        fi
      fi
    fi
  done

  if $JSON_OUTPUT; then
    printf '  ],\n'
    printf '  "summary": {\n'
    printf '    "total_matches": %d,\n' "$total_matches"
    printf '    "terms_with_matches": %d\n' "$terms_with_matches"
    printf '  }\n'
    printf '}\n'
  else
    printf '\nSummary\n'
    printf 'total_matches: %d\n' "$total_matches"
    printf 'terms_with_matches: %d\n' "$terms_with_matches"
  fi

  if $CHECK_MODE && [[ "$total_matches" -gt 0 ]]; then
    return 1
  fi
  return 0
}

run_rewrite_mode() {
  if $JSON_OUTPUT; then
    echo "error: --json is only supported in inventory mode" >&2
    return 1
  fi
  if [[ -z "$ALLOWLIST_FILE" || -z "$RULES_FILE" ]]; then
    echo "error: rewrite mode requires --allowlist and --rules" >&2
    return 1
  fi
  if [[ ! -f "$ALLOWLIST_FILE" ]]; then
    echo "error: allowlist file not found: $ALLOWLIST_FILE" >&2
    return 1
  fi
  if [[ ! -f "$RULES_FILE" ]]; then
    echo "error: rules file not found: $RULES_FILE" >&2
    return 1
  fi

  local -a allowlist=()
  while IFS= read -r line; do
    allowlist+=("$line")
  done < <(grep -vE '^\s*(#|$)' "$ALLOWLIST_FILE" || true)
  if [[ ${#allowlist[@]} -eq 0 ]]; then
    echo "error: allowlist is empty after filtering comments/blank lines" >&2
    return 1
  fi

  local -a files=()
  local f
  for f in "${allowlist[@]}"; do
    if [[ ! -f "$f" ]]; then
      echo "warn: skipping missing allowlisted file: $f" >&2
      continue
    fi
    case "$f" in
      docs/*|crates/*|configs/*|scenarios/*|examples/*|verification/*) ;;
      *)
        echo "warn: skipping non-supported allowlisted file: $f" >&2
        continue
        ;;
    esac
    if is_excluded_path "$f"; then
      echo "warn: skipping excluded allowlisted file: $f" >&2
      continue
    fi
    files+=("$f")
  done

  if [[ ${#files[@]} -eq 0 ]]; then
    echo "error: no eligible files remained after allowlist filtering" >&2
    return 1
  fi

  local pending=0
  local applied=0

  echo "Curated Rewrite (pass 2)"
  echo "rules: $RULES_FILE"
  echo "allowlist: $ALLOWLIST_FILE"
  echo "apply: $APPLY_REWRITE"

  while IFS=$'\t' read -r term from to glob; do
    [[ -z "${term:-}" ]] && continue
    [[ "${term:0:1}" == "#" ]] && continue
    if [[ -z "${from:-}" || -z "${to:-}" || -z "${glob:-}" ]]; then
      echo "warn: malformed rule (expected 4 TSV fields): $term $from $to $glob" >&2
      continue
    fi

    local file
    for file in "${files[@]}"; do
      case "$file" in
        $glob) ;;
        *) continue ;;
      esac

      local hit_count
      hit_count=$(rg --fixed-strings --count-matches --no-heading -- "$from" "$file" 2>/dev/null || true)
      [[ -z "$hit_count" ]] && hit_count=0
      if [[ "$hit_count" -eq 0 ]]; then
        continue
      fi

      pending=$((pending + hit_count))
      printf '[%s] %s: %s -> %s (%d)\n' "$term" "$file" "$from" "$to" "$hit_count"

      if $APPLY_REWRITE; then
        FROM="$from" TO="$to" perl -i -pe 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$file"
        applied=$((applied + hit_count))
      fi
    done
  done < "$RULES_FILE"

  echo "pending_matches: $pending"
  if $APPLY_REWRITE; then
    echo "applied_replacements: $applied"
  else
    echo "dry_run: true (use --apply to write changes)"
  fi

  if $CHECK_MODE && [[ "$pending" -gt 0 ]]; then
    return 1
  fi
  return 0
}

if [[ "$MODE" == "inventory" ]]; then
  run_inventory_mode
else
  run_rewrite_mode
fi
