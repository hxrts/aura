//! Terminal screen normalization for deterministic assertions.
//!
//! Strips ANSI codes, normalizes whitespace, and extracts authoritative screen
//! regions for consistent snapshot comparison across terminal emulators.

use regex::Regex;
use std::sync::OnceLock;

const NAV_HEADER_MARKERS: [&str; 5] = [
    "Neighborhood",
    "Chat",
    "Contacts",
    "Notifications",
    "Settings",
];

#[must_use]
pub fn has_nav_header(raw: &str) -> bool {
    let canonical = raw.replace('\r', "");
    canonical
        .lines()
        .any(|line| nav_anchor_offset(line).is_some())
}

#[must_use]
pub fn authoritative_screen(raw: &str) -> String {
    let canonical = raw.replace('\r', "");
    let lines: Vec<&str> = canonical.lines().collect();
    if lines.is_empty() {
        return String::new();
    }

    let anchor = lines
        .iter()
        .enumerate()
        .rev()
        .find_map(|(idx, line)| nav_anchor_offset(line).map(|offset| (idx, offset)));

    match anchor {
        Some((start, offset)) => {
            let mut frame_lines: Vec<String> = lines[start..]
                .iter()
                .map(|line| (*line).to_string())
                .collect();
            if let Some(first_line) = frame_lines.first_mut() {
                let anchored = first_line[offset..].to_string();
                *first_line = anchored;
            }
            frame_lines.join("\n")
        }
        None => canonical,
    }
}

pub fn normalize_screen(raw: &str) -> String {
    let canonical = authoritative_screen(raw);
    let timestamp = timestamp_pattern();
    let spinner = spinner_pattern();
    let sequence = sequence_pattern();

    let normalized = timestamp.replace_all(&canonical, "<time>");
    let normalized = spinner.replace_all(&normalized, "${prefix}<spin>${suffix}");
    let normalized = sequence.replace_all(&normalized, "#<n>");
    normalized.into_owned()
}

fn timestamp_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"\b\d{2}:\d{2}:\d{2}\b").unwrap_or_else(|error| panic!("{error}"))
    })
}

fn spinner_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"(?P<prefix>\s)[/|\\-](?P<suffix>\s)").unwrap_or_else(|error| panic!("{error}"))
    })
}

fn sequence_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"#\d+\b").unwrap_or_else(|error| panic!("{error}")))
}

fn nav_anchor_offset(line: &str) -> Option<usize> {
    let mut cursor = line.find(NAV_HEADER_MARKERS[0])?;
    for marker in NAV_HEADER_MARKERS.iter().skip(1) {
        let remainder = &line[cursor..];
        let marker_pos = remainder.find(marker)?;
        cursor += marker_pos;
    }
    line.find(NAV_HEADER_MARKERS[0])
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
    fn normalization_replaces_spinner_and_sequence_tokens() {
        let raw = "tick 12:34:56 / #99";
        let normalized = normalize_screen(raw);
        assert!(normalized.contains("<time>"));
        assert!(normalized.contains("<spin>"));
        assert!(normalized.contains("#<n>"));
    }

    #[test]
    fn authoritative_screen_uses_latest_nav_header_anchor() {
        let raw = "\
old stale row\n\
Neighborhood Chat Contacts Notifications Settings\n\
frame one\n\
Neighborhood Chat Contacts Notifications Settings\n\
frame two";

        let authoritative = authoritative_screen(raw);
        assert_eq!(
            authoritative,
            "Neighborhood Chat Contacts Notifications Settings\nframe two"
        );
    }

    #[test]
    fn authoritative_screen_strips_prefix_before_nav_header() {
        let raw = "\
old prefix Neighborhood Chat Contacts Notifications Settings\n\
frame row";
        let authoritative = authoritative_screen(raw);
        assert_eq!(
            authoritative,
            "Neighborhood Chat Contacts Notifications Settings\nframe row"
        );
    }

    #[test]
    fn detects_nav_header_presence() {
        assert!(has_nav_header(
            "Neighborhood Chat Contacts Notifications Settings\nrow"
        ));
        assert!(!has_nav_header("2026-01-01T00:00:00Z INFO log line"));
    }
}
