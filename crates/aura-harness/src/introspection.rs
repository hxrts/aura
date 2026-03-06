//! Screen state introspection and pattern extraction.
//!
//! Parses terminal screen output to extract UI state snapshots including toasts,
//! channels, contacts, command status, and modal states for assertion matching.

use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    Success,
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToastSnapshot {
    pub level: ToastLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelSnapshot {
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContactSnapshot {
    pub name: String,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionSnapshot {
    pub value: String,
}

pub fn extract_authority_id(screen: &str) -> Option<String> {
    let right_panel = screen
        .lines()
        .filter_map(right_panel_text)
        .collect::<Vec<_>>()
        .join(" ");
    let compact = right_panel
        .chars()
        .filter(|ch| {
            ch.is_ascii_alphanumeric() || *ch == '-' || *ch == ':' || *ch == '(' || *ch == ')'
        })
        .collect::<String>()
        .to_ascii_lowercase();
    let authority_pattern =
        r"authority-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";

    if let Some(captures) = Regex::new(&format!(r"authority:?({authority_pattern})\(local\)"))
        .ok()?
        .captures(&compact)
    {
        return captures.get(1).map(|value| value.as_str().to_string());
    }

    let authority_id_regex = Regex::new(authority_pattern).ok()?;
    let matches = authority_id_regex
        .find_iter(&compact)
        .map(|value| value.as_str().to_string())
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        return matches.into_iter().next();
    }
    None
}

pub fn extract_channels(screen: &str) -> Vec<ChannelSnapshot> {
    let mut channels = Vec::new();
    let Ok(channel_regex) = Regex::new(r"^(?P<selected>➤\s*)?#\s*(?P<name>.+)$") else {
        return channels;
    };
    for line in screen.lines() {
        let Some(left) = left_panel_text(line) else {
            continue;
        };
        let Some(captures) = channel_regex.captures(left.trim()) else {
            continue;
        };
        let name = captures
            .name("name")
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        channels.push(ChannelSnapshot {
            name,
            selected: captures.name("selected").is_some(),
        });
    }
    channels
}

pub fn extract_contacts(screen: &str) -> Vec<ContactSnapshot> {
    let mut contacts = Vec::new();
    let Ok(contact_regex) = Regex::new(r"^(?P<selected>➤\s*)?(?:○|●)\s+(?P<name>.+)$") else {
        return contacts;
    };
    for line in screen.lines() {
        let Some(left) = left_panel_text(line) else {
            continue;
        };
        let Some(captures) = contact_regex.captures(left.trim()) else {
            continue;
        };
        let name = captures
            .name("name")
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        contacts.push(ContactSnapshot {
            name,
            selected: captures.name("selected").is_some(),
        });
    }
    contacts
}

pub fn extract_current_selection(screen: &str) -> Option<SelectionSnapshot> {
    for line in screen.lines() {
        let left = left_panel_text(line)?;
        let trimmed = left.trim();
        if let Some(value) = trimmed.strip_prefix('➤') {
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            return Some(SelectionSnapshot {
                value: value.to_string(),
            });
        }
    }
    None
}

pub fn extract_toast(screen: &str) -> Option<ToastSnapshot> {
    for line in screen.lines().rev() {
        let stripped = line.trim().trim_matches('│').trim().to_string();
        if stripped.is_empty() {
            continue;
        }
        let (level, marker) = if stripped.contains('✓') {
            (ToastLevel::Success, '✓')
        } else if stripped.contains('✗') {
            (ToastLevel::Error, '✗')
        } else if stripped.contains('ℹ') {
            (ToastLevel::Info, 'ℹ')
        } else {
            continue;
        };
        let marker_index = stripped.find(marker)?;
        // Avoid message-row checkmarks like "authorit 05:30 ✓" in the chat transcript.
        if marker_index > 2 && !stripped.contains("[Esc] dismiss") {
            continue;
        }
        let message_segment = &stripped[marker_index + marker.len_utf8()..];
        let message = message_segment
            .replace("[Esc] dismiss", "")
            .trim()
            .to_string();
        if message.is_empty() {
            continue;
        }
        return Some(ToastSnapshot { level, message });
    }
    None
}

pub fn extract_command_consistency(message: &str) -> Option<String> {
    if let Some(consistency) = extract_command_field(message, "consistency") {
        if let Some(normalized) = normalize_consistency(&consistency) {
            return Some(normalized);
        }
    }
    Regex::new(r"\((accepted|replicated|enforced|partial-timeout)\)\s*$")
        .ok()?
        .captures(message)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

pub fn extract_command_status(message: &str) -> Option<String> {
    extract_command_field(message, "status").and_then(|status| normalize_status(&status))
}

pub fn extract_command_reason(message: &str) -> Option<String> {
    extract_command_field(message, "reason").and_then(|reason| normalize_reason(&reason))
}

fn extract_command_field(message: &str, field: &str) -> Option<String> {
    let alias = match field {
        "status" => Some("s"),
        "reason" => Some("r"),
        "consistency" => Some("c"),
        _ => None,
    };

    for key in [Some(field), alias].into_iter().flatten() {
        let pattern = format!(r"(?:^|\s|\[){}\s*=\s*([a-z0-9_-]+)", regex::escape(key));
        if let Some(value) = Regex::new(&pattern)
            .ok()
            .and_then(|regex| regex.captures(message))
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_string())
        {
            return Some(value);
        }
    }
    None
}

fn normalize_status(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    for status in ["ok", "denied", "invalid", "failed"] {
        if status.starts_with(value.as_str()) {
            return Some(status.to_string());
        }
    }
    None
}

fn normalize_consistency(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    for consistency in ["accepted", "replicated", "enforced", "partial-timeout"] {
        if consistency.starts_with(value.as_str()) {
            return Some(consistency.to_string());
        }
    }
    None
}

fn normalize_reason(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    for reason in [
        "none",
        "missing_active_context",
        "permission_denied",
        "not_member",
        "not_found",
        "invalid_argument",
        "invalid_state",
        "muted",
        "banned",
        "internal",
    ] {
        if reason.starts_with(value.as_str()) {
            return Some(reason.to_string());
        }
    }
    None
}

fn left_panel_text(line: &str) -> Option<&str> {
    let segments: Vec<&str> = line.split('│').collect();
    if segments.len() < 3 {
        return None;
    }
    Some(segments.get(1)?.trim())
}

fn right_panel_text(line: &str) -> Option<&str> {
    let segments: Vec<&str> = line.split('│').collect();
    if segments.len() < 5 {
        return None;
    }
    Some(segments.get(3)?.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_authority_id_from_wrapped_panel() {
        let screen = "\
│ Settings                 │ │ Authority: authority-fe389724-d3aa-             │\n\
│   Guardian Threshold     │ │ fe4a-08c9-86282c52e184 (Local)                  │\n";
        assert_eq!(
            extract_authority_id(screen).as_deref(),
            Some("authority-fe389724-d3aa-fe4a-08c9-86282c52e184")
        );
    }

    #[test]
    fn extracts_local_authority_when_multiple_authorities_are_visible() {
        let screen = "\
│ Settings                 │ │ Related: authority-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee │\n\
│   Authority              │ │ Authority: authority-fe389724-d3aa-                      │\n\
│                          │ │ fe4a-08c9-86282c52e184 (Local)                           │\n";
        assert_eq!(
            extract_authority_id(screen).as_deref(),
            Some("authority-fe389724-d3aa-fe4a-08c9-86282c52e184")
        );
    }

    #[test]
    fn ambiguous_authority_list_without_local_marker_returns_none() {
        let screen = "\
│ Settings                 │ │ Authority: authority-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee │\n\
│   Authority              │ │ Peer: authority-fe389724-d3aa-fe4a-08c9-86282c52e184      │\n";
        assert_eq!(extract_authority_id(screen), None);
    }

    #[test]
    fn extracts_channels_and_selection() {
        let screen = "\
│ Channels (2)             │ │ Messages                                        │\n\
│   # DM: Bob              │ │                                                 │\n\
│ ➤ # slash-lab            │ │                                                 │\n";
        let channels = extract_channels(screen);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "DM: Bob");
        assert!(!channels[0].selected);
        assert_eq!(channels[1].name, "slash-lab");
        assert!(channels[1].selected);

        let selection = match extract_current_selection(screen) {
            Some(selection) => selection,
            None => panic!("selection should exist"),
        };
        assert_eq!(selection.value, "# slash-lab");
    }

    #[test]
    fn extracts_contacts() {
        let screen = "\
│ Contacts (2)             │ │ Details                                         │\n\
│ ➤ ○ Bob                  │ │ Nickname: Bob                                   │\n\
│   ● Carol                │ │ Nickname: Carol                                 │\n";
        let contacts = extract_contacts(screen);
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].name, "Bob");
        assert!(contacts[0].selected);
        assert_eq!(contacts[1].name, "Carol");
    }

    #[test]
    fn extracts_toast_and_consistency() {
        let screen = "\
│ ℹ kick applied (enforced)                                   [Esc] dismiss │\n";
        let toast = match extract_toast(screen) {
            Some(toast) => toast,
            None => panic!("toast should parse"),
        };
        assert_eq!(toast.level, ToastLevel::Info);
        assert_eq!(toast.message, "kick applied (enforced)");
        assert_eq!(
            extract_command_consistency(&toast.message).as_deref(),
            Some("enforced")
        );
    }

    #[test]
    fn extracts_toast_without_dismiss_hint() {
        let screen = "\
│ ✗ [s=denied r=permission_denied c=none] /pin: Permission denied: Only moderators can pin │\n";
        let toast = match extract_toast(screen) {
            Some(toast) => toast,
            None => panic!("toast should parse"),
        };
        assert_eq!(toast.level, ToastLevel::Error);
        assert!(toast.message.contains("s=denied"));
        assert_eq!(
            extract_command_status(&toast.message).as_deref(),
            Some("denied")
        );
        assert_eq!(
            extract_command_reason(&toast.message).as_deref(),
            Some("permission_denied")
        );
    }

    #[test]
    fn ignores_message_row_checkmark() {
        let screen = "\
│                          │ │                           │ authorit  05:30 ✓ │ │\n";
        assert!(extract_toast(screen).is_none());
    }

    #[test]
    fn extracts_command_metadata_fields() {
        let message = "updated [s=ok r=none c=replicated]";
        assert_eq!(extract_command_status(message).as_deref(), Some("ok"));
        assert_eq!(extract_command_reason(message).as_deref(), Some("none"));
        assert_eq!(
            extract_command_consistency(message).as_deref(),
            Some("replicated")
        );
    }

    #[test]
    fn normalizes_truncated_command_metadata_fields() {
        let message = "updated [s=den r=permission_de c=acce]";
        assert_eq!(extract_command_status(message).as_deref(), Some("denied"));
        assert_eq!(
            extract_command_reason(message).as_deref(),
            Some("permission_denied")
        );
        assert_eq!(
            extract_command_consistency(message).as_deref(),
            Some("accepted")
        );
    }
}
