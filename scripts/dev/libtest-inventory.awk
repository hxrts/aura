# Extract only libtest terminal identities. A --nocapture test can print its
# first diagnostic on the header line, then more diagnostics before its status.
# Keep the expected name pending until one exact terminal token arrives.
BEGIN {
  while ((getline name < expected_file) > 0) {
    if (name != "") expected[name] = 1
  }
  close(expected_file)
}
function emit(kind, name) {
  if (seen[name]++) ambiguous = 1
  results[++count] = kind "\t" name
  if (kind == "passed") passed_count++
  else if (kind == "failed") failed_count++
  else ignored_count++
}
/^test .+ \.\.\.$/ {
  if (pending != "") ambiguous = 1
  split_status = 1
  pending = $0
  sub(/^test /, "", pending)
  sub(/ \.\.\.$/, "", pending)
  if (!(pending in expected)) ambiguous = 1
  next
}
/^test .+ \.\.\. / {
  if (!match($0, / \.\.\. /)) next
  name = substr($0, 6, RSTART - 6)
  outcome = substr($0, RSTART + RLENGTH)
  if (!(name in expected) || pending != "") ambiguous = 1
  if (outcome == "ok") emit("passed", name)
  else if (outcome == "FAILED") emit("failed", name)
  else if (outcome ~ /^ignored(,.*)?$/) emit("ignored", name)
  else {
    pending = name
    split_status = 1
  }
  next
}
/^(ok|FAILED|ignored(,.*)?)$/ {
  if (pending == "") { ambiguous = 1; next }
  if ($0 == "ok") emit("passed", pending)
  else if ($0 == "FAILED") emit("failed", pending)
  else emit("ignored", pending)
  pending = ""
  next
}
/^test result: (ok|FAILED)\. [0-9]+ passed; [0-9]+ failed; [0-9]+ ignored([;.]|$)/ {
  summary = $0
  sub(/^test result: (ok|FAILED)\. /, "", summary)
  split(summary, parts, "; ")
  split(parts[1], field, " "); summary_passed += field[1]
  split(parts[2], field, " "); summary_failed += field[1]
  split(parts[3], field, " "); summary_ignored += field[1]
  summary_seen++
  next
}
END {
  if (pending != "" || ambiguous ||
      (split_status && (!summary_seen || summary_passed != passed_count ||
                        summary_failed != failed_count || summary_ignored != ignored_count))) exit 78
  for (i = 1; i <= count; i++) print results[i]
}
