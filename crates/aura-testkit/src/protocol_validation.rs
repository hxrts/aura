//! Protocol-level validation helpers for choreography tests.
//!
//! These assertions are intended for test code in Layer 5+ crates to enforce
//! coherence and orphan-free properties on choreography sources.

use aura_mpst::upstream::language::{
    ast::{choreography_to_global, local_to_local_r, LocalTypeR},
    parse_choreography_str, project,
};
use std::collections::BTreeMap;
use telltale_theory::coherence::check_coherent;
use telltale_theory::subtyping::{async_subtype, orphan_free};

fn strip_aura_annotations_for_parser(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    #[allow(clippy::while_let_on_iterator)]
    while let Some(ch) = chars.next() {
        let Some((closing, preserve_if_no_equals)) = (match ch {
            '[' => Some((']', true)),
            '{' => Some(('}', true)),
            _ => None,
        }) else {
            out.push(ch);
            continue;
        };

        let mut depth = 1usize;
        let mut buf = String::new();
        let mut has_equals = false;

        while let Some(next) = chars.next() {
            if next == ch {
                depth += 1;
            } else if next == closing {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            if next == '=' {
                has_equals = true;
            }
            buf.push(next);
        }

        if preserve_if_no_equals && !has_equals {
            out.push(ch);
            out.push_str(&buf);
            out.push(closing);
        }
    }

    out
}

fn normalize_legacy_parser_surface(input: &str) -> String {
    let mut output = Vec::new();
    let mut inside_choice: Option<usize> = None;

    for raw_line in input.lines() {
        let trimmed = raw_line.trim_start();
        if trimmed == "@parallel" {
            continue;
        }

        let indent = raw_line.len() - trimmed.len();
        if !trimmed.is_empty() && !trimmed.starts_with("--") {
            if let Some(choice_indent) = inside_choice {
                if indent <= choice_indent {
                    inside_choice = None;
                }
            }
        }

        let mut line = raw_line.replace("::", ".");

        if let Some(rest) = trimmed.strip_prefix("case choose ") {
            if let Some((role, _)) = rest.split_once(" of") {
                line = format!("{}choice at {}", " ".repeat(indent), role.trim());
                inside_choice = Some(indent);
            }
        } else if inside_choice.is_some() {
            let current_trimmed = line.trim_start();
            if !current_trimmed.is_empty()
                && !current_trimmed.starts_with("--")
                && !current_trimmed.starts_with('|')
                && current_trimmed.ends_with("->")
            {
                line = format!("{}| {}", " ".repeat(indent), current_trimmed);
            }
        }

        line = convert_legacy_message_payload_syntax(&line);
        output.push(line);
    }

    output.join("\n")
}

fn convert_legacy_message_payload_syntax(line: &str) -> String {
    let Some(colon_index) = line.find(':') else {
        return line.to_string();
    };

    let (prefix, suffix_with_colon) = line.split_at(colon_index + 1);
    let suffix = suffix_with_colon.trim_start();
    let Some(open_paren) = suffix.find('(') else {
        return line.to_string();
    };
    let Some(close_paren) = suffix.rfind(')') else {
        return line.to_string();
    };
    if close_paren <= open_paren {
        return line.to_string();
    }

    let message = suffix[..open_paren].trim_end();
    if message.is_empty() {
        return line.to_string();
    }

    let payload = suffix[open_paren + 1..close_paren].trim();
    let trailing = suffix[close_paren + 1..].trim_end();
    let mut rebuilt = format!("{prefix} {message} of {payload}");
    if !trailing.is_empty() {
        rebuilt.push(' ');
        rebuilt.push_str(trailing);
    }
    rebuilt
}

fn project_locals_by_role(source: &str, label: &str) -> BTreeMap<String, LocalTypeR> {
    let parser_source = normalize_legacy_parser_surface(&strip_aura_annotations_for_parser(source));
    let choreography = parse_choreography_str(&parser_source)
        .unwrap_or_else(|err| panic!("{label}: failed to parse choreography source: {err}"));

    let mut locals = BTreeMap::new();
    for role in &choreography.roles {
        let local = project(&choreography, role).unwrap_or_else(|err| {
            panic!("{label}: projection failed for role {}: {err}", role.name())
        });
        let local_r = local_to_local_r(&local).unwrap_or_else(|err| {
            panic!(
                "{label}: local conversion failed for role {}: {err}",
                role.name()
            )
        });
        locals.insert(role.name().to_string(), local_r);
    }
    locals
}

/// Assert that a choreography source is coherent under telltale-theory checks.
pub fn assert_protocol_coherent(source: &str) {
    let parser_source = normalize_legacy_parser_surface(&strip_aura_annotations_for_parser(source));
    let choreography = parse_choreography_str(&parser_source)
        .unwrap_or_else(|err| panic!("coherence: failed to parse choreography source: {err}"));
    let global = choreography_to_global(&choreography).unwrap_or_else(|err| {
        panic!("coherence: failed to convert choreography to theory global: {err}")
    });

    let bundle = check_coherent(&global);
    assert!(
        bundle.is_coherent(),
        "coherence failed: size={}, action={}, uniq_labels={}, projectable={}, good={}",
        bundle.size,
        bundle.action,
        bundle.uniq_labels,
        bundle.projectable,
        bundle.good
    );
}

/// Assert that every role projection in a choreography is orphan-free.
pub fn assert_orphan_free_for_all_roles(source: &str) {
    for (role, local) in orphan_free_status_for_all_roles(source) {
        assert!(local, "orphan-free failed for role `{role}`");
    }
}

/// Compute orphan-free status for each projected role in a choreography.
pub fn orphan_free_status_for_all_roles(source: &str) -> BTreeMap<String, bool> {
    let locals = project_locals_by_role(source, "orphan-free");
    locals
        .into_iter()
        .map(|(role, local)| (role, orphan_free(&local)))
        .collect()
}

/// Assert protocol evolution compatibility (`new` is an async subtype of `old`)
/// for all roles present in both protocol versions.
pub fn assert_async_subtype_for_shared_roles(old_source: &str, new_source: &str) {
    check_async_subtype_for_shared_roles(old_source, new_source)
        .unwrap_or_else(|err| panic!("{err}"));
}

/// Check protocol evolution compatibility (`new` is an async subtype of `old`)
/// for all roles present in both protocol versions.
pub fn check_async_subtype_for_shared_roles(
    old_source: &str,
    new_source: &str,
) -> Result<(), String> {
    let old_locals = project_locals_by_role(old_source, "old protocol");
    let new_locals = project_locals_by_role(new_source, "new protocol");

    for (role, old_local) in &old_locals {
        if let Some(new_local) = new_locals.get(role) {
            async_subtype(new_local, old_local).map_err(|err| {
                format!(
                    "async subtype failed for role `{role}`: new is not a subtype of old: {err}"
                )
            })?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{normalize_legacy_parser_surface, strip_aura_annotations_for_parser};

    #[test]
    fn normalizes_legacy_choice_and_message_syntax() {
        let source = r#"
protocol Demo =
  roles A, B

  case choose A of
    accept ->
      A -> B : Msg(crate::demo::Payload)
"#;

        let normalized = normalize_legacy_parser_surface(source);
        assert!(normalized.contains("choice at A"));
        assert!(normalized.contains("| accept ->"));
        assert!(normalized.contains("Msg of crate.demo.Payload"));
    }

    #[test]
    fn drops_parallel_marker_for_parser_surface() {
        let source = r#"
protocol Demo =
  roles A, B

  @parallel
  A -> B : Msg(crate::demo::Payload)
"#;

        let normalized = normalize_legacy_parser_surface(source);
        assert!(!normalized.contains("@parallel"));
        assert!(normalized.contains("Msg of crate.demo.Payload"));
    }

    #[test]
    fn strips_brace_style_role_annotations_for_parser_surface() {
        let source = r#"
protocol Demo =
  roles
    A { guard_capability = "demo:start" },
    B

  A -> B : Msg(crate::demo::Payload)
"#;

        let normalized = strip_aura_annotations_for_parser(source);
        assert!(!normalized.contains("guard_capability"));
        assert!(normalized.contains("roles"));
        assert!(normalized.contains("A ,"));
    }
}
