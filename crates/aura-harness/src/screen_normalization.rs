use regex::Regex;

pub fn normalize_screen(raw: &str) -> String {
    let canonical = latest_frame_slice(raw);
    let timestamp = Regex::new(r"\b\d{2}:\d{2}:\d{2}\b").unwrap_or_else(|error| panic!("{error}"));

    let normalized = timestamp.replace_all(&canonical, "<time>");
    normalized.to_string()
}

fn latest_frame_slice(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    // TUI snapshots may contain stale remnants from prior renders. Anchor to the
    // final visible nav header to keep matching deterministic for wait_for.
    if let Some(start) = lines
        .iter()
        .enumerate()
        .rfind(|(_, line)| line.contains("Neighborhood") && line.contains("Chat"))
        .map(|(idx, _)| idx)
    {
        return lines[start..].join("\n");
    }

    raw.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_preserves_hyphenated_tokens() {
        let raw = "status 12:34:56 msg-a-1";
        let normalized = normalize_screen(raw);
        assert!(normalized.contains("<time>"));
        assert!(normalized.contains("msg-a-1"));
    }

    #[test]
    fn normalization_uses_latest_frame_anchor() {
        let raw = "\
Neighborhood Chat old-frame\n\
old content marker\n\
Neighborhood Chat new-frame\n\
new content marker";
        let normalized = normalize_screen(raw);
        assert!(!normalized.contains("old content marker"));
        assert!(normalized.contains("new content marker"));
    }
}
