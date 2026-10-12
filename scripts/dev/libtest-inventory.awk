# Extract only libtest terminal identities. A --nocapture test can print
# diagnostics after its name and before its status. Keep the name pending until
# one exact terminal token arrives; never infer a result from panic or summary.
function emit(kind, name) {
  if (seen[name]++) ambiguous = 1
  results[++count] = kind "\t" name
}
/^test .+ \.\.\. (ok|FAILED|ignored(,.*)?)$/ {
  if (pending != "") ambiguous = 1
  name = $0
  sub(/^test /, "", name)
  sub(/ \.\.\. (ok|FAILED|ignored(,.*)?)$/, "", name)
  if ($0 ~ / \.\.\. ok$/) emit("passed", name)
  else if ($0 ~ / \.\.\. FAILED$/) emit("failed", name)
  else emit("ignored", name)
  pending = ""
  next
}
/^test .+ \.\.\. ?$/ {
  if (pending != "") ambiguous = 1
  pending = $0
  sub(/^test /, "", pending)
  sub(/ \.\.\. ?$/, "", pending)
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
END {
  if (pending != "" || ambiguous) exit 78
  for (i = 1; i <= count; i++) print results[i]
}
