#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════════════════════
# Semantic Documentation Drift Detector
# ═══════════════════════════════════════════════════════════════════════════════
#
# Detects when documentation references identifiers, types, traits, or commands
# that no longer exist in the codebase. Complements docs-links.sh (link integrity)
# with semantic integrity checking.
#
# Checks:
# 1. Just command references (`just foo`) - validates against justfile recipes
# 2. Crate name references (`aura-*`) - validates crates exist
# 3. Type/trait references (PascalCase in backticks) - validates definitions exist
# 4. Effect trait references (*Effects) - validates trait definitions
# 5. File path references in code fences - validates paths exist
#
# Usage:
#   ./scripts/check/docs-semantic-drift.sh [OPTIONS]
#
# Options:
#   --verbose       Show all checked items, not just violations
#   --fix-suggestions  Show suggested fixes for violations
#   --category CAT  Only check specific category (just|crates|types|effects|paths)
#   -h, --help      Show this help
#
# Exit codes:
#   0 - No semantic drift detected
#   1 - Semantic drift found
#   2 - Script error

set -euo pipefail

# ───────────────────────────────────────────────────────────────────────────────
# Configuration
# ───────────────────────────────────────────────────────────────────────────────

ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$ROOT"

# Documentation sources to check
DOC_SOURCES=(
    "CLAUDE.md"
    "AGENTS.md"
    "docs"
)

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# State
VIOLATIONS=0
CHECKED=0
VERBOSE=false
FIX_SUGGESTIONS=false
CATEGORY_FILTER=""

# ───────────────────────────────────────────────────────────────────────────────
# CLI Parsing
# ───────────────────────────────────────────────────────────────────────────────

usage() {
    sed -n '2,/^$/p' "$0" | grep "^#" | sed 's/^# \?//'
    exit 0
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --verbose) VERBOSE=true ;;
        --fix-suggestions) FIX_SUGGESTIONS=true ;;
        --category)
            [[ -z "${2:-}" ]] && { echo "error: --category requires argument"; exit 2; }
            CATEGORY_FILTER="$2"
            shift ;;
        -h|--help) usage ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
    shift
done

# ───────────────────────────────────────────────────────────────────────────────
# Output Helpers
# ───────────────────────────────────────────────────────────────────────────────

section() { printf "\n${BOLD}${CYAN}%s${NC}\n" "$1"; }
info() { printf "${BLUE}•${NC} %s\n" "$1"; }
ok() { $VERBOSE && printf "${GREEN}✓${NC} %s\n" "$1" || true; }
violation() {
    ((VIOLATIONS++)) || true
    printf "${RED}✖${NC} %s\n" "$1"
}
hint() { $FIX_SUGGESTIONS && printf "  ${YELLOW}→${NC} %s\n" "$1" || true; }

# ───────────────────────────────────────────────────────────────────────────────
# Utility Functions
# ───────────────────────────────────────────────────────────────────────────────

# Get all markdown files from doc sources
get_doc_files() {
    for src in "${DOC_SOURCES[@]}"; do
        if [[ -f "$src" ]]; then
            echo "$src"
        elif [[ -d "$src" ]]; then
            find "$src" -name "*.md" -type f 2>/dev/null
        fi
    done
}

# Extract unique items from docs matching a pattern
# Usage: extract_from_docs "pattern" [transform_sed]
extract_from_docs() {
    local pattern="$1"
    local transform="${2:-}"
    local results=""

    while IFS= read -r file; do
        [[ -z "$file" || ! -f "$file" ]] && continue
        local matches
        # Use rg for better regex handling, fallback to grep
        if command -v rg >/dev/null 2>&1; then
            matches=$(rg -noN "$pattern" "$file" 2>/dev/null || true)
        else
            matches=$(grep -noE "$pattern" "$file" 2>/dev/null || true)
        fi
        while IFS= read -r match; do
            [[ -z "$match" ]] && continue
            local linenum="${match%%:*}"
            local content="${match#*:}"
            if [[ -n "$transform" ]]; then
                content=$(echo "$content" | sed -E "$transform")
            fi
            [[ -n "$content" ]] && results+="$content|$file:$linenum"$'\n'
        done <<< "$matches"
    done < <(get_doc_files)

    # Return unique items with their first occurrence location
    echo "$results" | awk -F'|' '!seen[$1]++ {print $1 "|" $2}'
}

# Extract just commands - specialized function for reliable detection
extract_just_commands() {
    local results=""

    while IFS= read -r file; do
        [[ -z "$file" || ! -f "$file" ]] && continue
        # Use perl for reliable backtick matching (with stdin redirection for reliability)
        local matches
        matches=$(perl -ne 'while (/`just ([a-zA-Z_][a-zA-Z0-9_-]+)`/g) { print "$.\t$1\n"; }' < "$file" 2>/dev/null || true)
        while IFS=$'\t' read -r linenum cmd; do
            [[ -z "$cmd" ]] && continue
            results+="$cmd|$file:$linenum"$'\n'
        done <<< "$matches"
    done < <(get_doc_files)

    # Return unique items with their first occurrence location
    echo "$results" | awk -F'|' '!seen[$1]++ {print $1 "|" $2}'
}

# Extract crate names - specialized function for reliable detection
extract_crate_names() {
    local results=""

    while IFS= read -r file; do
        [[ -z "$file" || ! -f "$file" ]] && continue
        # Match `aura-*` in backticks
        local matches
        matches=$(perl -ne 'while (/`(aura-[a-z0-9-]+)`/g) { print "$.\t$1\n"; }' < "$file" 2>/dev/null || true)
        while IFS=$'\t' read -r linenum crate; do
            [[ -z "$crate" ]] && continue
            results+="$crate|$file:$linenum"$'\n'
        done <<< "$matches"
    done < <(get_doc_files)

    # Return unique items with their first occurrence location
    echo "$results" | awk -F'|' '!seen[$1]++ {print $1 "|" $2}'
}

# Extract effect traits - specialized function for reliable detection
extract_effect_traits() {
    local results=""

    while IFS= read -r file; do
        [[ -z "$file" || ! -f "$file" ]] && continue
        # Match *Effects in backticks or as standalone words
        local matches
        matches=$(perl -ne 'while (/`?([A-Z][a-zA-Z]*Effects)`?/g) { print "$.\t$1\n"; }' < "$file" 2>/dev/null || true)
        while IFS=$'\t' read -r linenum trait; do
            [[ -z "$trait" ]] && continue
            results+="$trait|$file:$linenum"$'\n'
        done <<< "$matches"
    done < <(get_doc_files)

    # Return unique items with their first occurrence location
    echo "$results" | awk -F'|' '!seen[$1]++ {print $1 "|" $2}'
}

# Extract type names - specialized function for reliable detection
extract_type_names() {
    local results=""

    while IFS= read -r file; do
        [[ -z "$file" || ! -f "$file" ]] && continue
        # Match PascalCase identifiers in backticks (likely type names)
        local matches
        matches=$(perl -ne 'while (/`([A-Z][a-zA-Z0-9_]+)`/g) { print "$.\t$1\n"; }' < "$file" 2>/dev/null || true)
        while IFS=$'\t' read -r linenum typename; do
            [[ -z "$typename" ]] && continue
            results+="$typename|$file:$linenum"$'\n'
        done <<< "$matches"
    done < <(get_doc_files)

    # Return unique items with their first occurrence location
    echo "$results" | awk -F'|' '!seen[$1]++ {print $1 "|" $2}'
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Just Commands
# ═══════════════════════════════════════════════════════════════════════════════

check_just_commands() {
    [[ -n "$CATEGORY_FILTER" && "$CATEGORY_FILTER" != "just" ]] && return
    section "Just command references"

    if [[ ! -f "justfile" ]]; then
        violation "justfile not found"
        return
    fi

    # Extract all recipe names from justfile (including those with parameters)
    local recipes
    recipes=$(grep -E "^[a-zA-Z_][a-zA-Z0-9_-]*(\s+.*)?:" justfile 2>/dev/null | \
        sed -E 's/^([a-zA-Z_][a-zA-Z0-9_-]*).*/\1/' | sort -u)

    # Extract just commands from docs - only in backticks
    local just_refs
    just_refs=$(extract_just_commands)

    local found=0 missing=0
    while IFS='|' read -r cmd location; do
        [[ -z "$cmd" ]] && continue
        ((CHECKED++)) || true

        # Skip common false positives (words that might appear in prose)
        case "$cmd" in
            the|a|an|to|for|with|run|use|see|like|need|want|that|this|command|commands|enough|about|after|another) continue ;;
        esac

        if echo "$recipes" | grep -qxF "$cmd"; then
            ok "just $cmd (referenced in $location)"
            ((found++)) || true
        else
            violation "just $cmd - recipe not found in justfile (referenced in $location)"
            # Find similar recipes
            if $FIX_SUGGESTIONS; then
                local similar
                similar=$(echo "$recipes" | grep -i "${cmd:0:4}" 2>/dev/null || true)
                similar=$(echo "$similar" | head -3 | tr '\n' ' ' | sed 's/ $//' | sed 's/ /, /g')
                [[ -n "$similar" && "$similar" != " " ]] && hint "Similar recipes: $similar"
            fi
            ((missing++)) || true
        fi
    done <<< "$just_refs"

    info "Just commands: $found valid, $missing invalid"
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Crate Names
# ═══════════════════════════════════════════════════════════════════════════════

check_crate_names() {
    [[ -n "$CATEGORY_FILTER" && "$CATEGORY_FILTER" != "crates" ]] && return
    section "Crate name references"

    # Get actual crate directories
    local crates
    crates=$(find crates -maxdepth 1 -type d -name "aura-*" 2>/dev/null | \
        xargs -I{} basename {} | sort -u)

    # Extract aura-* references from docs - in backticks only for precision
    local crate_refs
    crate_refs=$(extract_crate_names)

    local found=0 missing=0
    while IFS='|' read -r crate location; do
        [[ -z "$crate" ]] && continue
        # Skip if it's part of a path or module reference
        [[ "$crate" == *"/"* || "$crate" == *"::"* ]] && continue
        # Skip partial matches that include file extensions
        [[ "$crate" == *".rs" || "$crate" == *".toml" ]] && continue
        ((CHECKED++)) || true

        if echo "$crates" | grep -qxF "$crate"; then
            ok "$crate (referenced in $location)"
            ((found++)) || true
        else
            # Check if it might be a valid crate that doesn't exist yet
            case "$crate" in
                aura-frost)
                    violation "$crate - deprecated crate (referenced in $location)"
                    hint "aura-frost is deprecated; use aura-core::crypto::tree_signing"
                    ;;
                *)
                    violation "$crate - crate directory not found (referenced in $location)"
                    ;;
            esac
            ((missing++)) || true
        fi
    done <<< "$crate_refs"

    info "Crate names: $found valid, $missing invalid"
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Effect Traits
# ═══════════════════════════════════════════════════════════════════════════════

check_effect_traits() {
    [[ -n "$CATEGORY_FILTER" && "$CATEGORY_FILTER" != "effects" ]] && return
    section "Effect trait references"

    # Extract *Effects trait names from crates/aura-core/src/effects/
    local defined_effects
    defined_effects=$(grep -rh "pub trait [A-Z][a-zA-Z]*Effects" crates/aura-core/src/effects/ 2>/dev/null | \
        sed -E 's/.*pub trait ([A-Z][a-zA-Z]*Effects).*/\1/' | sort -u)

    # Also check for traits defined elsewhere that end in Effects
    local other_effects
    other_effects=$(grep -rh "pub trait [A-Z][a-zA-Z]*Effects" crates/ 2>/dev/null | \
        grep -v "aura-core/src/effects/" | \
        sed -E 's/.*pub trait ([A-Z][a-zA-Z]*Effects).*/\1/' | sort -u)

    local all_effects
    all_effects=$(printf "%s\n%s" "$defined_effects" "$other_effects" | sort -u)

    # Extract *Effects references from docs
    local effect_refs
    effect_refs=$(extract_effect_traits)

    local found=0 missing=0 skipped=0
    while IFS='|' read -r effect location; do
        [[ -z "$effect" ]] && continue

        # Skip example/placeholder effect names
        case "$effect" in
            MyEffects|MyProtocolEffects|CustomEffects|ExampleEffects|TestEffects|MockEffects|DummyEffects|FakeEffects|StubEffects|PingPongEffects|AllEffects|CliEffects|ConfigEffects|DatabaseEffects|RandEffects|BloomEffects)
                ((skipped++)) || true; continue ;;
        esac

        ((CHECKED++)) || true

        if echo "$all_effects" | grep -qxF "$effect"; then
            ok "$effect (referenced in $location)"
            ((found++)) || true
        else
            violation "$effect - trait not defined (referenced in $location)"
            # Suggest similar
            if $FIX_SUGGESTIONS; then
                local similar
                similar=$(echo "$all_effects" | grep -i "${effect:0:6}" 2>/dev/null || true)
                similar=$(echo "$similar" | head -3 | tr '\n' ' ' | sed 's/ $//' | sed 's/ /, /g')
                [[ -n "$similar" && "$similar" != " " ]] && hint "Similar traits: $similar"
            fi
            ((missing++)) || true
        fi
    done <<< "$effect_refs"

    info "Effect traits: $found valid, $missing invalid, $skipped skipped"
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Type/Struct/Enum References
# ═══════════════════════════════════════════════════════════════════════════════

check_type_references() {
    [[ -n "$CATEGORY_FILTER" && "$CATEGORY_FILTER" != "types" ]] && return
    section "Type/struct/enum references"

    # Build a list of all public type definitions in the codebase
    # This is expensive, so we cache it
    local type_cache="/tmp/aura-types-cache-$$"
    trap "rm -f $type_cache" EXIT

    info "Building type index (this may take a moment)..."

    # Extract struct, enum, trait, type alias definitions
    grep -rh "pub struct \|pub enum \|pub trait \|pub type " crates/ --include="*.rs" 2>/dev/null | \
        sed -E 's/.*(pub struct |pub enum |pub trait |pub type )([A-Z][a-zA-Z0-9_]*).*/\2/' | \
        sort -u > "$type_cache"

    local type_count
    type_count=$(wc -l < "$type_cache" | tr -d ' ')
    info "Indexed $type_count public types"

    # Key types that MUST exist (critical domain types)
    local critical_types=(
        "AuthorityId"
        "ContextId"
        "DeviceId"
        "ChannelId"
        "JournalState"
        "AttestedOp"
        "RelationalFact"
        "Biscuit"
    )

    # Extract PascalCase identifiers in backticks from docs
    local type_refs
    type_refs=$(extract_type_names)

    local found=0 missing=0 skipped=0
    while IFS='|' read -r typename location; do
        [[ -z "$typename" ]] && continue

        # Skip common false positives
        case "$typename" in
            # Generic words that happen to be PascalCase
            OK|README|SUMMARY|TODO|FIXME|NOTE|WARNING|IMPORTANT|TIP|CAUTION|None|Some)
                ((skipped++)) || true; continue ;;
            # Markdown/formatting artifacts
            Category|Command|Purpose|Layer|Crates|Metric|Count|Status|File|See|Example)
                ((skipped++)) || true; continue ;;
            # Common abbreviations that aren't types
            API|CLI|TUI|UI|CI|CD|PR|OS|IO|ID|UUID|DKG|MFA|OTA|RTT|HTTP|HTTPS|URL|JSON|CBOR|TOML|YAML|WASM|HSM|RGB|CRDT|BFT)
                ((skipped++)) || true; continue ;;
            # Effect system constants mentioned in prose
            Effects)
                ((skipped++)) || true; continue ;;
            # Standard Rust traits/types
            Send|Sync|Clone|Copy|Debug|Display|Default|Drop|Eq|Ord|Hash|Iterator|Future|Pin|Box|Arc|Rc|Vec|Option|Result|String|Sized|Unpin|From|Into|AsRef|AsMut|Deref|DerefMut|Index|IndexMut|Add|Sub|Mul|Div|Neg|Not|BitAnd|BitOr|BitXor|Shl|Shr|PartialEq|PartialOrd|Serialize|Deserialize|Error|Read|Write|Seek|BufRead|Infallible|PhantomData)
                ((skipped++)) || true; continue ;;
            # Standard Lean types
            Nat|Int|Bool|Prop|Type|List|Array|String|Char|Float|Unit|Decidable|DecidableEq|Repr|Inhabited|Nonempty|Subtype|Sigma|PSigma|Prod|Sum|Or|And|Iff|Not|True|False|Eq|Ne|HEq|BEq|LE|LT|Monoid|Group|Ring|Field|Module|Semiring|AddCommGroup|CommRing|CommGroup)
                ((skipped++)) || true; continue ;;
            # Quint keywords/builtins
            AND|OR|IFF|IMPLIES|MATCH|IMPORT|EXPORT|Set|Map|Tuple|Record|Variant)
                ((skipped++)) || true; continue ;;
            # Quint invariant/property names (specification terms, not code types)
            Invariant*|Property*|Temporal*|Check*|Availability*|Has*|AllInvariants|*Progress*|*Bound*|*Threshold)
                ((skipped++)) || true; continue ;;
            # Quint spec identifier patterns (T_*, Fallback_*, K_*, GST, etc.) - must come before *_*
            GST|T_fallback|Fallback_Start|K_boot)
                ((skipped++)) || true; continue ;;
            # ALL_CAPS constants (not types) - must come after specific underscore patterns
            *_*)
                if [[ "$typename" =~ ^[A-Z][A-Z0-9_]+$ ]]; then
                    ((skipped++)) || true; continue
                fi ;;
            # Common example/placeholder type names
            My*|Custom*|Example*|Test*|Mock*|Dummy*|Fake*|Stub*)
                ((skipped++)) || true; continue ;;
            # Very short names (1-2 chars) are usually not our types
            [A-Z]|[A-Z][A-Z0-9])
                ((skipped++)) || true; continue ;;
            # Documentation domain terms (roles, states, modes) - single words only
            Member|Participant|Moderator|Observer|Admin|Guest|Owner|User|Role|Level|Mode)
                ((skipped++)) || true; continue ;;
            # Access level terms
            Full|Partial|Limited|Restricted|Public|Private|Internal)
                ((skipped++)) || true; continue ;;
            # State machine states (commonly documented but are enum variants)
            Initializing|Active|Waiting|Completed|Started|Pending|Running|Stopped|Failed|Idle|Ready|Processing|Done|Expired|TimedOut|Cancelled|AllDone|Observable)
                ((skipped++)) || true; continue ;;
            # Clock/time variants (documented in prose but are enum variants)
            PhysicalClock|LogicalClock|OrderClock|Range|Physical|Logical|Order)
                ((skipped++)) || true; continue ;;
            # Common enum variant names that appear in documentation
            Op|All|Any|Deny|Allow|Accept|Reject|Push|Pull|Pushing|Pulling|Local|Remote|Neighbor|External|Dev|Ci|Prod|Production|Debug)
                ((skipped++)) || true; continue ;;
            # Sync/anti-entropy states
            DigestExchanged|PeerCompleted|AllCompleted)
                ((skipped++)) || true; continue ;;
            # Protocol session/phase names
            Protocol|Session|Channel|Descriptor|Justfile|Term)
                ((skipped++)) || true; continue ;;
            # Crypto primitive names (documented but often from external crates)
            OsRng|SignShare|SingleSigner|SigningNonces|SigningCommitments|Frost)
                ((skipped++)) || true; continue ;;
            # Standard Rust types from std that may be documented
            SystemTime|Duration|Uuid|NonZeroU16|NonZeroU32|NonZeroU64|Ordering|Infallible)
                ((skipped++)) || true; continue ;;
            # Verification/spec property names (from Quint/Lean specs, not code types)
            UniqueCommitPerInstance|CommitRequiresThreshold|ProgressUnderSynchrony|RetryBound|ByzantineThreshold|EquivocationDetected|NonceUnique|FactsOrdered|FactsMonotonic|EventualConvergence|NoCapabilityWidening|Validity|Equivocation|Liveness|NoFaults|Monotonicity|NonInterference|ContextIsolation|AuthorizationSoundness|NoDeadlock)
                ((skipped++)) || true; continue ;;
            # Simulator/testing fault and event names
            MessageDelay|MessageDrop|MessageCorruption|NodeCrash|FlowBudgetExhaustion|JournalCorruption|SchedulerStep)
                ((skipped++)) || true; continue ;;
            # AMP/channel operation names (documented but are action names not types)
            CreateChannel|CloseChannel|SendMessage|ChannelCreated|ChannelClosed|MessageReceived|ChangePolicy|RotateEpoch|AddLeaf|RemoveLeaf|EpochBump|MessageRead)
                ((skipped++)) || true; continue ;;
            # Policy/error variant names
            LegacyPermissive|BoundExceeded|AuthorizationDenied|InsufficientBudget|JournalCommitFailed|PrestateStale|NewerRequest|ExplicitCancel|Timeout|Precedence|CeremonySuperseded)
                ((skipped++)) || true; continue ;;
            # Tree/topology terms
            Leaf|Branch|Consensus)
                ((skipped++)) || true; continue ;;
            # Generic effect/action names
            Effect|Assert|Retract|Ack)
                ((skipped++)) || true; continue ;;
            # Conceptual identifier types (documented but use different impl names)
            AccountId|GuardianId|EventId|HomeId|NeighborhoodId|SessionId|ReplicaId|LogIndex)
                ((skipped++)) || true; continue ;;
            # Conceptual/planned handler types
            HsmCryptoHandler|NoOpSecureEnclaveHandler|UnsupportedHsmHandler|RealBiometricHandler|StatefulTimeHandler)
                ((skipped++)) || true; continue ;;
            # Documented effect types that are conceptual examples
            CliEffects|ConfigEffects|DatabaseEffects|BloomEffects|RandEffects|RelationalContextEffects)
                ((skipped++)) || true; continue ;;
            # AMP/channel types (may be Lean/spec types rather than Rust types)
            AmpChannelCheckpoint|AmpChannelPolicy|AmpCommittedChannelEpochBump|AmpProposedChannelEpochBump)
                ((skipped++)) || true; continue ;;
            # Capability/authorization conceptual terms
            Capability|MeetSemilattice|CapGuard)
                ((skipped++)) || true; continue ;;
            # Choreography/protocol conceptual terms
            StoreMetadata|ChargeBudget|RecordLeakage|LeakageTracker|OutputConditionPolicy|ChargeBeforeSend)
                ((skipped++)) || true; continue ;;
            # Runtime/service conceptual terms
            RuntimeTaskRegistry|ServiceRegistry|SessionLifecycleChoreography|VMConfig|NativeCooperative|WasmCooperative)
                ((skipped++)) || true; continue ;;
            # Rendezvous conceptual terms
            Rendezvous|RendezvousReceipt|ChannelEstablished)
                ((skipped++)) || true; continue ;;
            # Verification/Lean types (documented but may only exist in Lean/Quint)
            ComparePolicy|FlowChargeInput|FlowChargeResult|TimestampCompareInput|TimestampCompareResult|PrestateHash|WitnessVote)
                ((skipped++)) || true; continue ;;
            # Generic conceptual terms
            Generic|Unanimous|Percentage|Routine|SuspiciousActivity|ConfirmedCompromise|MessageBubble|SessionDelegation)
                ((skipped++)) || true; continue ;;
            # Ownership model categories (architectural concepts, not Rust types)
            Pure|MoveOwned|ActorOwned|Observed|Submitted|Succeeded)
                ((skipped++)) || true; continue ;;
            # Planned error variant names (documented design, not yet implemented)
            RuntimeUnavailable|ConnectivityRequired|Precondition)
                ((skipped++)) || true; continue ;;
            # OTA/module lifecycle types (planned flow coverage, not yet implemented)
            PublishSyntheticOtaRelease|StageOtaCandidate|TriggerBootloaderHandoff|ConfirmCandidateHealth|RollbackOtaCandidate|OtaReleasePublished|OtaArtifactAvailable|OtaStaged|OtaCompatibilityBlocked|OtaCandidateLaunched|OtaHealthConfirmed|OtaRolledBack|PublishCandidateOtaRelease|ApproveOtaCutover|OtaCandidatePublished|OtaPromotionStateChanged|OtaRehearsalPassed)
                ((skipped++)) || true; continue ;;
            # Module lifecycle types (planned flow coverage, not yet implemented)
            PublishSyntheticModuleRelease|StageModuleCandidate|PrepareModuleAdmission|CommitModuleCutover|RollbackModuleCutover|ModuleReleasePublished|ModuleArtifactAvailable|ModuleVerified|ModuleStaged|ModuleAdmissionPrepared|ModuleCutoverCommitted|ModuleRolledBack|PublishCandidateModuleRelease|ApproveModuleCutover|ModuleCandidatePublished|ModulePromotionStateChanged|ModuleHealthConfirmed|ModuleRehearsalPassed)
                ((skipped++)) || true; continue ;;
            # Harness command types (documented design, not yet implemented as Rust types)
            SendKeys|SendKey|ClickButton|FillInput|FillField)
                ((skipped++)) || true; continue ;;
        esac

        # Skip if it's clearly from a Lean/Quint skill file (external concepts)
        case "$location" in
            .claude/skills/verification/lean/*|.claude/skills/verification/quint/*|.claude/*lean*|.claude/*quint*)
                # Only check Aura-specific types in these files
                case "$typename" in
                    Aura*|Authority*|Context*|Channel*|Journal*|Fact*|Guard*|Flow*|Biscuit*|Consensus*|Session*|Protocol*)
                        ;; # Continue checking these
                    *)
                        ((skipped++)) || true; continue ;;
                esac
                ;;
        esac

        ((CHECKED++)) || true

        if grep -qxF "$typename" "$type_cache"; then
            ok "$typename (referenced in $location)"
            ((found++)) || true
        else
            # Check if it's a critical type that's missing
            local is_critical=false
            for ct in "${critical_types[@]}"; do
                [[ "$typename" == "$ct" ]] && is_critical=true && break
            done

            if $is_critical; then
                violation "$typename - CRITICAL type not found! (referenced in $location)"
            else
                # For non-critical types, check if it might be a renamed type
                local similar
                similar=$(grep -i "^${typename:0:5}" "$type_cache" 2>/dev/null || true)
                similar=$(echo "$similar" | head -3 | tr '\n' ' ' | sed 's/ $//' | sed 's/ /, /g')

                violation "$typename - type not found (referenced in $location)"
                { $FIX_SUGGESTIONS && [[ -n "$similar" && "$similar" != " " ]] && hint "Similar types: $similar"; } || true
            fi
            ((missing++)) || true
        fi
    done <<< "$type_refs"

    info "Types: $found valid, $missing invalid, $skipped skipped"
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Code Fence File Paths
# ═══════════════════════════════════════════════════════════════════════════════

check_code_fence_paths() {
    [[ -n "$CATEGORY_FILTER" && "$CATEGORY_FILTER" != "paths" ]] && return
    section "Code fence file path references"

    # Extract file paths from code fences that look like:
    # ```rust
    # // File: crates/aura-core/src/foo.rs
    # or
    # // In crates/aura-core/src/foo.rs:
    local path_refs
    path_refs=$(extract_from_docs '(File:|In |See )crates/[a-zA-Z0-9_/-]+\.(rs|toml)' \
        's/.*(crates\/[a-zA-Z0-9_\/-]+\.(rs|toml)).*/\1/')

    local found=0 missing=0
    while IFS='|' read -r filepath location; do
        [[ -z "$filepath" ]] && continue
        ((CHECKED++)) || true

        if [[ -f "$filepath" ]]; then
            ok "$filepath (referenced in $location)"
            ((found++)) || true
        else
            violation "$filepath - file not found (referenced in $location)"
            # Try to find similar paths
            if $FIX_SUGGESTIONS; then
                local basename
                basename=$(basename "$filepath")
                local similar
                similar=$(find crates -name "$basename" -type f 2>/dev/null || true)
                similar=$(echo "$similar" | head -3 | tr '\n' ' ' | sed 's/ $//' | sed 's/ /, /g')
                [[ -n "$similar" && "$similar" != " " ]] && hint "Found similar: $similar"
            fi
            ((missing++)) || true
        fi
    done <<< "$path_refs"

    info "Code fence paths: $found valid, $missing invalid"
}

# ═══════════════════════════════════════════════════════════════════════════════
# CHECK: Deprecated/Removed Patterns
# ═══════════════════════════════════════════════════════════════════════════════

check_deprecated_patterns() {
    [[ -n "$CATEGORY_FILTER" ]] && return
    section "Known deprecated pattern references"

    # Known deprecated patterns that should not appear in docs
    local deprecated_patterns=(
        "DeviceMetadata|Device metadata struct was removed"
        "DeviceRegistry|Device registry was removed - derive from LeafNode"
        "aura-frost|aura-frost deprecated - use aura-core::crypto::tree_signing"
        "journal_ops|Graph-based journal_ops removed - use fact-based AttestedOps"
        "GraphOp|Graph operations removed - use AttestedOp"
    )

    local found_deprecated=0
    for pattern_info in "${deprecated_patterns[@]}"; do
        local pattern="${pattern_info%%|*}"
        local message="${pattern_info#*|}"

        while IFS= read -r file; do
            [[ -z "$file" || ! -f "$file" ]] && continue
            local matches
            matches=$(grep -n "$pattern" "$file" 2>/dev/null || true)
            while IFS= read -r match; do
                [[ -z "$match" ]] && continue
                ((CHECKED++)) || true
                local linenum="${match%%:*}"
                violation "Deprecated reference: $pattern in $file:$linenum"
                hint "$message"
                ((found_deprecated++)) || true
            done <<< "$matches"
        done < <(get_doc_files)
    done

    if [[ $found_deprecated -eq 0 ]]; then
        info "No deprecated patterns found"
    fi
}

# ═══════════════════════════════════════════════════════════════════════════════
# Main
# ═══════════════════════════════════════════════════════════════════════════════

main() {
    printf "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}\n"
    printf "${BOLD}${CYAN}  Semantic Documentation Drift Detector${NC}\n"
    printf "${BOLD}${CYAN}═══════════════════════════════════════════════════════════════${NC}\n"

    # Verify we're in the right directory
    if [[ ! -f "Cargo.toml" || ! -d "crates" ]]; then
        echo "error: must be run from Aura workspace root" >&2
        exit 2
    fi

    # Count doc files
    local doc_count
    doc_count=$(get_doc_files | wc -l | tr -d ' ')
    info "Scanning $doc_count documentation files..."

    # Run checks
    check_just_commands
    check_crate_names
    check_effect_traits
    check_type_references
    check_code_fence_paths
    check_deprecated_patterns

    # Summary
    section "Summary"
    printf "  Checked:    %d references\n" "$CHECKED"
    printf "  Violations: %d\n" "$VIOLATIONS"

    if [[ $VIOLATIONS -eq 0 ]]; then
        printf "\n${GREEN}✔ No semantic drift detected${NC}\n"
        exit 0
    else
        printf "\n${RED}✖ Semantic drift detected - documentation may be stale${NC}\n"
        printf "\nRun with ${YELLOW}--fix-suggestions${NC} to see potential fixes\n"
        exit 1
    fi
}

main "$@"
