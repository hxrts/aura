use regex::Regex;

pub fn normalize_screen(raw: &str) -> String {
    let timestamp = Regex::new(r"\b\d{2}:\d{2}:\d{2}\b").unwrap_or_else(|error| panic!("{error}"));
    let spinner = Regex::new(r"[|/\\-]").unwrap_or_else(|error| panic!("{error}"));
    let counter = Regex::new(r"#\d+").unwrap_or_else(|error| panic!("{error}"));

    let normalized = timestamp.replace_all(raw, "<time>");
    let normalized = spinner.replace_all(&normalized, "<spin>");
    let normalized = counter.replace_all(&normalized, "#<n>");
    normalized.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_removes_volatile_tokens() {
        let raw = "status 12:34:56 / #42";
        let normalized = normalize_screen(raw);
        assert!(normalized.contains("<time>"));
        assert!(normalized.contains("<spin>"));
        assert!(normalized.contains("#<n>"));
    }
}
