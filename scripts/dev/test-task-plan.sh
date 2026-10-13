#!/usr/bin/env bash
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
fixture="$(mktemp -d "${TMPDIR:-/tmp}/aura-task-plan-test.XXXXXX")"
trap 'rm -rf "$fixture"' EXIT
plan="$fixture/plan.md"
mkdir -p "$fixture/evidence"
printf '%s\n' 'Durable original evidence.' > "$fixture/evidence/proof.md"
cat > "$plan" <<'EOF'
# Current plan
- [ ] 1. Preserve complete acceptance.
  - Acceptance: original source and disposal, including all linked evidence.
  - **Work log**
    - Old receipt one. [proof](evidence/proof.md), [remote](https://example.com), [anchor](#scope).
      Continued first receipt.
    - Old receipt two.
    - Current receipt three.
    - Current receipt four.
  - Dependency: task 2.
- [x] 2. Separate adjacent task.
  - Exact adjacent content.
EOF
cp "$plan" "$fixture/original.md"
tool=(bash "$repo_root/scripts/dev/task-plan.sh" --plan "$plan")
"${tool[@]}" show 1 > "$fixture/show.md"
rg -q 'Acceptance: original source' "$fixture/show.md"
if rg -q 'Separate adjacent task' "$fixture/show.md"; then exit 1; fi
"${tool[@]}" archive 1 2 > "$fixture/archive-result.log"
rg -q 'Acceptance: original source' "$plan"
rg -q 'Current receipt three' "$plan"
rg -q 'Dependency: task 2' "$plan"
if rg -q 'Old receipt one' "$plan"; then exit 1; fi
rg -q 'Old receipt one' "$fixture"/task-archives/*.md
rg -q 'Continued first receipt' "$fixture"/task-archives/*.md
rg -q 'Historical receipts:.*task-archives/' "$plan"
first_archive="$(sed -n 's/.*](\(task-archives\/[^)]*\)).*/\1/p' "$plan")"
rg -F -q '[proof](../evidence/proof.md)' "$fixture/$first_archive"
rg -F -q '[remote](https://example.com), [anchor](#scope)' "$fixture/$first_archive"
[[ -s "$(dirname "$fixture/$first_archive")/../evidence/proof.md" ]]
backup="$(sed -n 's/.*original backup: //p' "$fixture/archive-result.log")"
[[ "$backup" == "$fixture/.task-plan/"*/before.md ]]
assert_no_obsolete_sibling() {
  [[ -z "$(find "$fixture" -maxdepth 1 -name '.task-plan.*' -print -quit)" ]]
}
mkdir "$fixture/.task-plan.obsolete"
if assert_no_obsolete_sibling; then exit 1; fi
rmdir "$fixture/.task-plan.obsolete"
assert_no_obsolete_sibling
cmp -s "$backup" "$fixture/original.md"
printf '%s\n' 'New evidence with literal $(do not execute) and `backticks`.' 'Continuation.' > "$fixture/entry.md"
"${tool[@]}" log 1 "$fixture/entry.md" > "$fixture/log-result.log"
rg -F -q '    - New evidence with literal $(do not execute) and `backticks`.' "$plan"
rg -q '^      Continuation\.$' "$plan"
awk '/Current receipt four/ { prior=NR } /New evidence/ { added=NR } /Dependency: task 2/ { boundary=NR } END { exit !(prior<added && added<boundary) }' "$plan"
"${tool[@]}" archive 1 2 > "$fixture/rearchive-result.log"
second_archive="$(sed -n 's/.*](\(task-archives\/[^)]*\)).*/\1/p' "$plan")"
[[ "$first_archive" != "$second_archive" ]]
rg -F -q "archive](../$first_archive)" "$fixture/$second_archive"
[[ -s "$(dirname "$fixture/$second_archive")/../$first_archive" ]]
"${tool[@]}" log 2 "$fixture/entry.md" > "$fixture/new-section-result.log"
"${tool[@]}" show 2 > "$fixture/show-two.md"
rg -q '^  - \*\*Work log\*\*$' "$fixture/show-two.md"
cp "$plan" "$fixture/unchanged.md"
if "${tool[@]}" log 999 "$fixture/entry.md" > "$fixture/missing.log" 2>&1; then exit 1; fi
cmp -s "$plan" "$fixture/unchanged.md"
printf '%s\n' '- [ ] 2. Duplicate target.' >> "$plan"
cp "$plan" "$fixture/duplicate.md"
if "${tool[@]}" log 2 "$fixture/entry.md" > "$fixture/duplicate.log" 2>&1; then exit 1; fi
cmp -s "$plan" "$fixture/duplicate.md"
cat > "$plan" <<'EOF'
- [x] 72. Completed numeric task.
  - Acceptance: retain original contract.
##### Task 72 owned addendum
  - Retain this additional contract.
### Next independent section
- [ ] **S6. Unnumbered live task.**
  - Its independent acceptance.
- [ ] 109. Another numeric task.
EOF
"${tool[@]}" show 72 > "$fixture/bounded-show.md"
rg -q 'Retain this additional contract' "$fixture/bounded-show.md"
if rg -q 'independent|S6|109' "$fixture/bounded-show.md"; then exit 1; fi
"${tool[@]}" log 72 "$fixture/entry.md" > "$fixture/bounded-log.log"
awk '/New evidence/ { added=NR } /^### Next/ { boundary=NR } END { exit !(added && added<boundary) }' "$plan"
sed '/^### Next independent section/d' "$plan" > "$fixture/no-heading.md"
mv "$fixture/no-heading.md" "$plan"
"${tool[@]}" show 72 > "$fixture/peer-bounded-show.md"
if rg -q 'S6|independent acceptance|109' "$fixture/peer-bounded-show.md"; then exit 1; fi
echo 'task plan fixtures passed: show, archive, backup, literal insertion, missing/duplicate refusal'
