//! Aura Annotation Extraction
//!
//! Simple annotation extraction utilities for Aura choreographic effects.
//! Follows the rumpsteak-aura demo pattern for clean, simple annotation processing.

/// Aura-specific effect that can be generated from annotations
#[derive(Debug, Clone, PartialEq)]
pub enum AuraEffect {
    /// Capability-based authorization requirement
    GuardCapability {
        /// The required capability string
        capability: String,
        /// The role that needs this capability
        role: String,
    },
    /// Flow budget cost for operations
    FlowCost {
        /// The cost in flow units
        cost: u64,
        /// The role that pays this cost
        role: String,
    },
    /// Journal facts to be recorded
    JournalFacts {
        /// The facts to record
        facts: String,
        /// The role that records these facts
        role: String,
    },
    /// Journal merge operation
    JournalMerge {
        /// The role that triggers the merge
        role: String,
    },
    /// Audit log entry
    AuditLog {
        /// The action to log
        action: String,
        /// The role that performs the action
        role: String,
    },
    /// Leakage tracking annotation
    Leakage {
        /// Observer classes that can see this operation
        observers: Vec<String>,
        /// The role that leaks information
        role: String,
    },
}

/// Errors that can occur during annotation extraction
#[derive(Debug, thiserror::Error)]
pub enum AuraExtractionError {
    /// Failed to parse an annotation
    #[error("Annotation parse error: {0}")]
    AnnotationParseError(String),

    /// Invalid annotation value
    #[error("Invalid annotation value: {0}")]
    InvalidAnnotationValue(String),

    /// Unsupported feature
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
}

/// Extract Aura annotations from a choreography string
///
/// Simple pattern-based extraction following the rumpsteak-aura demo approach.
/// This works with the extension system rather than trying to reparse the AST.
pub fn extract_aura_annotations(
    choreography_str: &str,
) -> Result<Vec<AuraEffect>, AuraExtractionError> {
    let mut effects = Vec::new();

    // Simple annotation detection for demonstration
    detect_annotations_in_text(choreography_str, &mut effects)?;

    Ok(effects)
}

/// Generate Aura choreography code from extracted effects
///
/// This function takes the namespace, roles, and extracted effects and generates
/// the appropriate code structures for integration with the Aura effect system.
pub fn generate_aura_choreography_code(
    namespace: &str,
    roles: &[&str],
    aura_effects: &[AuraEffect],
) -> String {
    let mut code = String::new();

    // Generate module header
    code.push_str(&format!("pub mod {} {{\n", namespace));

    // Generate role enum
    code.push_str("    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    code.push_str("    pub enum Role {\n");
    for role in roles {
        code.push_str(&format!("        {},\n", role));
    }
    code.push_str("    }\n\n");

    // Generate effect implementations
    for effect in aura_effects {
        match effect {
            AuraEffect::GuardCapability { capability, role } => {
                code.push_str(&format!(
                    "    // Guard capability '{}' for role {}\n",
                    capability, role
                ));
            }
            AuraEffect::FlowCost { cost, role } => {
                code.push_str(&format!("    // Flow cost {} for role {}\n", cost, role));
            }
            AuraEffect::JournalFacts { facts, role } => {
                code.push_str(&format!(
                    "    // Journal facts '{}' for role {}\n",
                    facts, role
                ));
            }
            AuraEffect::JournalMerge { role } => {
                code.push_str(&format!("    // Journal merge for role {}\n", role));
            }
            AuraEffect::AuditLog { action, role } => {
                code.push_str(&format!(
                    "    // Audit log '{}' for role {}\n",
                    action, role
                ));
            }
            AuraEffect::Leakage { observers, role } => {
                code.push_str(&format!(
                    "    // Leakage to observers {:?} for role {}\n",
                    observers, role
                ));
            }
        }
    }

    code.push_str("}\n");
    code
}

/// Detect annotations in choreography text
fn detect_annotations_in_text(
    choreography_str: &str,
    effects: &mut Vec<AuraEffect>,
) -> Result<(), AuraExtractionError> {
    // Simple pattern matching following the rumpsteak-aura demo approach

    for line in choreography_str.lines() {
        // Check if this line has a role annotation (contains '[' and has a send arrow '->')
        let has_role_annotation = line.contains('[') && line.contains("->");

        if line.contains("guard_capability") {
            if let Some(cap) = extract_capability_from_line(line) {
                effects.push(AuraEffect::GuardCapability {
                    capability: cap,
                    role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
                });
            }
        }

        // Handle flow_cost - apply default of 100 if annotation bracket exists but no flow_cost specified
        if line.contains("flow_cost") {
            if let Some(cost) = extract_flow_cost_from_line(line) {
                effects.push(AuraEffect::FlowCost {
                    cost,
                    role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
                });
            }
        } else if has_role_annotation {
            // Line has role annotation but no explicit flow_cost - apply default
            if let Some(role) = extract_role_from_line(line) {
                effects.push(AuraEffect::FlowCost {
                    cost: 100, // Default flow cost
                    role,
                });
            }
        }

        if line.contains("journal_facts") {
            if let Some(facts) = extract_journal_facts_from_line(line) {
                effects.push(AuraEffect::JournalFacts {
                    facts,
                    role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
                });
            }
        }

        // Handle leak annotation: [leak: (External, Neighbor)]
        if line.contains("leak:") || line.contains("leak =") {
            if let Some(observers) = extract_leakage_observers_from_line(line) {
                effects.push(AuraEffect::Leakage {
                    observers,
                    role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
                });
            }
        }

        // Handle journal_merge annotation: [journal_merge = true] or [journal_merge]
        if line.contains("journal_merge") {
            // Extract role and add JournalMerge effect
            effects.push(AuraEffect::JournalMerge {
                role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
            });
        }

        // Handle audit_log annotation: [audit_log = "action_name"]
        if line.contains("audit_log") {
            if let Some(action) = extract_audit_log_from_line(line) {
                effects.push(AuraEffect::AuditLog {
                    action,
                    role: extract_role_from_line(line).unwrap_or_else(|| "UnknownRole".to_string()),
                });
            }
        }
    }

    Ok(())
}

/// Extract role from a line - simple implementation
fn extract_role_from_line(line: &str) -> Option<String> {
    // Try to find role before brackets like "Alice[guard_capability = ...]"
    if let Some(bracket_pos) = line.find('[') {
        let before_bracket = line[..bracket_pos].trim();
        if !before_bracket.is_empty() {
            return Some(before_bracket.to_string());
        }
    }
    None
}

fn extract_capability_from_line(line: &str) -> Option<String> {
    // Extract capability value from line like: guard_capability = "send_message"
    if let Some(start) = line.find("guard_capability") {
        if let Some(quote_start) = line[start..].find('"') {
            let quote_start = start + quote_start + 1;
            if let Some(quote_end) = line[quote_start..].find('"') {
                return Some(line[quote_start..quote_start + quote_end].to_string());
            }
        }
    }
    None
}

fn extract_flow_cost_from_line(line: &str) -> Option<u64> {
    // Extract cost value from line like: flow_cost = 100 or Alice[flow_cost = 100]
    if let Some(start) = line.find("flow_cost") {
        if let Some(equals) = line[start..].find('=') {
            let after_equals = start + equals + 1;
            // Look for digits after the equals sign
            let remaining = &line[after_equals..];

            // Handle both bracketed and non-bracketed values
            let value_str = remaining
                .trim()
                .split(|c: char| c == ']' || c.is_whitespace())
                .next()?;
            if let Ok(cost) = value_str.trim().parse::<u64>() {
                return Some(cost);
            }

            // Fallback: look for tokens in remaining string
            for token in remaining.split_whitespace() {
                if let Ok(cost) = token.trim_end_matches(&[',', ']'][..]).parse::<u64>() {
                    return Some(cost);
                }
            }
        }
    }
    None
}

fn extract_journal_facts_from_line(line: &str) -> Option<String> {
    // Extract facts value from line like: journal_facts = "message_sent"
    if let Some(start) = line.find("journal_facts") {
        if let Some(quote_start) = line[start..].find('"') {
            let quote_start = start + quote_start + 1;
            if let Some(quote_end) = line[quote_start..].find('"') {
                return Some(line[quote_start..quote_start + quote_end].to_string());
            }
        }
    }
    None
}

/// Extract audit log action from a line
fn extract_audit_log_from_line(line: &str) -> Option<String> {
    // Extract action value from line like: audit_log = "action_name"
    if let Some(start) = line.find("audit_log") {
        if let Some(quote_start) = line[start..].find('"') {
            let quote_start = start + quote_start + 1;
            if let Some(quote_end) = line[quote_start..].find('"') {
                return Some(line[quote_start..quote_start + quote_end].to_string());
            }
        }
    }
    None
}

/// Extract leakage observers from a line
fn extract_leakage_observers_from_line(line: &str) -> Option<Vec<String>> {
    // Look for pattern like "leak: (External, Neighbor)" or "leak = (External)" or "leak = \"External\""
    let after_leak = if let Some(start) = line.find("leak:") {
        &line[start + 5..]
    } else if let Some(start) = line.find("leak =") {
        &line[start + 6..]
    } else if let Some(start) = line.find("leak=") {
        &line[start + 5..]
    } else {
        return None;
    };

    // Find the parentheses
    if let Some(paren_start) = after_leak.find('(') {
        if let Some(paren_end) = after_leak.find(')') {
            let observers_str = &after_leak[paren_start + 1..paren_end];

            // Split by comma and trim
            let observers: Vec<String> = observers_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !observers.is_empty() {
                return Some(observers);
            }
        }
    }

    // Also handle quoted string format: leak = "External"
    if let Some(quote_start) = after_leak.find('"') {
        if let Some(quote_end) = after_leak[quote_start + 1..].find('"') {
            let observer = after_leak[quote_start + 1..quote_start + 1 + quote_end].trim();
            if !observer.is_empty() {
                return Some(vec![observer.to_string()]);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aura_effect_types() {
        let guard_effect = AuraEffect::GuardCapability {
            capability: "test_capability".to_string(),
            role: "TestRole".to_string(),
        };

        match guard_effect {
            AuraEffect::GuardCapability { capability, role } => {
                assert_eq!(capability, "test_capability");
                assert_eq!(role, "TestRole");
            }
            _ => panic!("Wrong effect type"),
        }
    }

    #[test]
    fn test_flow_cost_effect() {
        let cost_effect = AuraEffect::FlowCost {
            cost: 150,
            role: "Coordinator".to_string(),
        };

        match cost_effect {
            AuraEffect::FlowCost { cost, role } => {
                assert_eq!(cost, 150);
                assert_eq!(role, "Coordinator");
            }
            _ => panic!("Wrong effect type"),
        }
    }

    #[test]
    fn test_extract_capability_from_line() {
        let line = r#"Alice[guard_capability = "send_message"] -> Bob: Message;"#;
        let capability = extract_capability_from_line(line);
        assert_eq!(capability, Some("send_message".to_string()));
    }

    #[test]
    fn test_extract_flow_cost_from_line() {
        let line = "Alice[flow_cost = 100] -> Bob: Message;";
        let cost = extract_flow_cost_from_line(line);
        assert_eq!(cost, Some(100));
    }

    #[test]
    fn test_extract_journal_facts_from_line() {
        let line = r#"Alice[journal_facts = "message_sent"] -> Bob: Message;"#;
        let facts = extract_journal_facts_from_line(line);
        assert_eq!(facts, Some("message_sent".to_string()));
    }

    #[test]
    fn test_extract_role_from_line() {
        let line = r#"Alice[guard_capability = "send_message"] -> Bob: Message;"#;
        let role = extract_role_from_line(line);
        assert_eq!(role, Some("Alice".to_string()));
    }

    #[test]
    fn test_default_flow_cost() {
        // Line with annotation but no flow_cost should get default of 100
        let choreography = r#"
            Alice[guard_capability = "send_message"] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        // Should have both guard capability and default flow cost
        let has_guard = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::GuardCapability { .. }));
        let has_flow_cost = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::FlowCost { cost: 100, .. }));

        assert!(has_guard, "Should have guard capability effect");
        assert!(has_flow_cost, "Should have default flow cost of 100");
    }

    #[test]
    fn test_explicit_flow_cost_overrides_default() {
        // Explicit flow_cost should override the default
        let choreography = r#"
            Alice[guard_capability = "send_message", flow_cost = 250] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        // Should have explicit flow cost of 250, not default 100
        let has_explicit_cost = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::FlowCost { cost: 250, .. }));
        let has_default_cost = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::FlowCost { cost: 100, .. }));

        assert!(has_explicit_cost, "Should have explicit flow cost of 250");
        assert!(
            !has_default_cost,
            "Should not have default flow cost when explicit cost provided"
        );
    }

    #[test]
    fn test_no_annotation_no_flow_cost() {
        // Line without annotation should not get flow cost
        let choreography = r#"
            Alice -> Bob: SimpleMessage;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        // Should not have any flow cost effects
        let has_flow_cost = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::FlowCost { .. }));

        assert!(
            !has_flow_cost,
            "Should not have flow cost when no annotation bracket present"
        );
    }

    #[test]
    fn test_journal_merge_annotation() {
        let choreography = r#"
            Alice[journal_merge = true] -> Bob: MergeRequest;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        let has_journal_merge = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::JournalMerge { role } if role == "Alice"));

        assert!(has_journal_merge, "Should extract journal_merge annotation");
    }

    #[test]
    fn test_audit_log_annotation() {
        let choreography = r#"
            Alice[audit_log = "message_sent"] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        let has_audit_log = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::AuditLog { action, role } if action == "message_sent" && role == "Alice"));

        assert!(has_audit_log, "Should extract audit_log annotation");
    }

    #[test]
    fn test_leak_annotation_parentheses() {
        let choreography = r#"
            Alice[leak: (External, Neighbor)] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        let has_leakage = effects.iter().any(|e| {
            matches!(e, AuraEffect::Leakage { observers, role }
                if observers.contains(&"External".to_string())
                && observers.contains(&"Neighbor".to_string())
                && role == "Alice")
        });

        assert!(
            has_leakage,
            "Should extract leak annotation with parentheses"
        );
    }

    #[test]
    fn test_leak_annotation_quoted() {
        let choreography = r#"
            Alice[leak = "External"] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();

        let has_leakage = effects.iter().any(|e| {
            matches!(e, AuraEffect::Leakage { observers, role }
                if observers.contains(&"External".to_string())
                && role == "Alice")
        });

        assert!(
            has_leakage,
            "Should extract leak annotation with quoted string"
        );
    }
}
