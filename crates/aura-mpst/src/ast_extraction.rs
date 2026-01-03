//! Aura Annotation Extraction
//!
//! Simple annotation extraction utilities for Aura choreographic effects.
//! Follows the rumpsteak-aura demo pattern for clean, simple annotation processing.

use crate::ids::RoleId;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{bracketed, parenthesized, Ident, LitBool, LitInt, LitStr, Token};

/// Aura-specific effect that can be generated from annotations
#[derive(Debug, Clone, PartialEq)]
pub enum AuraEffect {
    /// Capability-based authorization requirement
    GuardCapability {
        /// The required capability string
        capability: String,
        /// The role that needs this capability
        role: RoleId,
    },
    /// Flow budget cost for operations
    FlowCost {
        /// The cost in flow units
        cost: u64,
        /// The role that pays this cost
        role: RoleId,
    },
    /// Journal facts to be recorded
    JournalFacts {
        /// The facts to record
        facts: String,
        /// The role that records these facts
        role: RoleId,
    },
    /// Journal merge operation
    JournalMerge {
        /// The role that triggers the merge
        role: RoleId,
    },
    /// Audit log entry
    AuditLog {
        /// The action to log
        action: String,
        /// The role that performs the action
        role: RoleId,
    },
    /// Leakage tracking annotation
    Leakage {
        /// Observer classes that can see this operation
        observers: Vec<String>,
        /// The role that leaks information
        role: RoleId,
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

#[derive(Debug)]
struct AnnotationList {
    items: Vec<AnnotationItem>,
}

#[derive(Debug)]
struct AnnotationItem {
    name: String,
    value: Option<AnnotationValue>,
}

#[derive(Debug)]
enum AnnotationValue {
    Str(String),
    Int(u64),
    Bool(bool),
    IdentList(Vec<String>),
    IntList(Vec<u64>),
}

impl Parse for AnnotationList {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut items = Vec::new();
        while !input.is_empty() {
            items.push(input.parse()?);
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            } else {
                break;
            }
        }
        Ok(Self { items })
    }
}

impl Parse for AnnotationItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let value = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            Some(parse_annotation_value(input)?)
        } else {
            None
        };
        Ok(Self {
            name: name.to_string(),
            value,
        })
    }
}

fn parse_annotation_value(input: ParseStream) -> syn::Result<AnnotationValue> {
    if input.peek(LitStr) {
        let lit: LitStr = input.parse()?;
        Ok(AnnotationValue::Str(lit.value()))
    } else if input.peek(LitInt) {
        let lit: LitInt = input.parse()?;
        let value = lit.base10_parse::<u64>()?;
        Ok(AnnotationValue::Int(value))
    } else if input.peek(LitBool) {
        let lit: LitBool = input.parse()?;
        Ok(AnnotationValue::Bool(lit.value))
    } else if input.peek(syn::token::Paren) {
        let content;
        parenthesized!(content in input);
        let idents: Punctuated<Ident, Token![,]> =
            content.parse_terminated(Ident::parse, Token![,])?;
        if idents.is_empty() {
            return Err(syn::Error::new(
                content.span(),
                "leak list must contain at least one identifier",
            ));
        }
        Ok(AnnotationValue::IdentList(
            idents.into_iter().map(|ident| ident.to_string()).collect(),
        ))
    } else if input.peek(syn::token::Bracket) {
        let content;
        bracketed!(content in input);
        let values: Punctuated<LitInt, Token![,]> =
            content.parse_terminated(LitInt::parse, Token![,])?;
        if values.is_empty() {
            return Err(syn::Error::new(
                content.span(),
                "leakage_budget must contain at least one integer",
            ));
        }
        let parsed = values
            .into_iter()
            .map(|lit| lit.base10_parse::<u64>())
            .collect::<Result<Vec<_>, _>>()?;
        Ok(AnnotationValue::IntList(parsed))
    } else {
        Err(syn::Error::new(
            input.span(),
            "Expected string, integer, boolean, or identifier list",
        ))
    }
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
    code.push_str(&format!("pub mod {namespace} {{\n"));

    // Generate role enum
    code.push_str("    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]\n");
    code.push_str("    pub enum Role {\n");
    for role in roles {
        code.push_str(&format!("        {role},\n"));
    }
    code.push_str("    }\n\n");

    // Generate effect implementations
    for effect in aura_effects {
        match effect {
            AuraEffect::GuardCapability { capability, role } => {
                code.push_str(&format!(
                    "    // Guard capability '{capability}' for role {role}\n"
                ));
            }
            AuraEffect::FlowCost { cost, role } => {
                code.push_str(&format!("    // Flow cost {cost} for role {role}\n"));
            }
            AuraEffect::JournalFacts { facts, role } => {
                code.push_str(&format!("    // Journal facts '{facts}' for role {role}\n"));
            }
            AuraEffect::JournalMerge { role } => {
                code.push_str(&format!("    // Journal merge for role {role}\n"));
            }
            AuraEffect::AuditLog { action, role } => {
                code.push_str(&format!("    // Audit log '{action}' for role {role}\n"));
            }
            AuraEffect::Leakage { observers, role } => {
                code.push_str(&format!(
                    "    // Leakage to observers {observers:?} for role {role}\n"
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
    for raw_segment in choreography_str.split(';') {
        let line = raw_segment.trim();
        if line.is_empty() {
            continue;
        }
        let annotation_content = match extract_annotation_content(line) {
            Some(content) => content,
            None => continue,
        };

        let role = extract_role_from_line(line).unwrap_or_else(|| RoleId::new("UnknownRole"));
        let has_role_annotation = line.contains("->");
        let items = parse_annotation_list(&annotation_content)?;
        let mut has_flow_cost = false;

        for item in items {
            match item.name.as_str() {
                "guard_capability" => {
                    let capability = match item.value {
                        Some(AnnotationValue::Str(value)) => value,
                        Some(_) => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "guard_capability expects a string literal".to_string(),
                            ))
                        }
                        None => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "guard_capability requires a string literal".to_string(),
                            ))
                        }
                    };
                    effects.push(AuraEffect::GuardCapability {
                        capability,
                        role: role.clone(),
                    });
                }
                "flow_cost" => {
                    if has_flow_cost {
                        return Err(AuraExtractionError::InvalidAnnotationValue(
                            "flow_cost specified multiple times".to_string(),
                        ));
                    }
                    let cost = match item.value {
                        Some(AnnotationValue::Int(value)) => value,
                        Some(_) => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "flow_cost expects an integer literal".to_string(),
                            ))
                        }
                        None => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "flow_cost requires an integer literal".to_string(),
                            ))
                        }
                    };
                    effects.push(AuraEffect::FlowCost {
                        cost,
                        role: role.clone(),
                    });
                    has_flow_cost = true;
                }
                "journal_facts" => {
                    let facts = match item.value {
                        Some(AnnotationValue::Str(value)) => value,
                        Some(_) => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "journal_facts expects a string literal".to_string(),
                            ))
                        }
                        None => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "journal_facts requires a string literal".to_string(),
                            ))
                        }
                    };
                    effects.push(AuraEffect::JournalFacts {
                        facts,
                        role: role.clone(),
                    });
                }
                "journal_merge" => match item.value {
                    None => {
                        effects.push(AuraEffect::JournalMerge { role: role.clone() });
                    }
                    Some(AnnotationValue::Bool(true)) => {
                        effects.push(AuraEffect::JournalMerge { role: role.clone() });
                    }
                    Some(AnnotationValue::Bool(false)) => {}
                    Some(_) => {
                        return Err(AuraExtractionError::InvalidAnnotationValue(
                            "journal_merge expects a boolean literal".to_string(),
                        ))
                    }
                },
                "audit_log" => {
                    let action = match item.value {
                        Some(AnnotationValue::Str(value)) => value,
                        Some(_) => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "audit_log expects a string literal".to_string(),
                            ))
                        }
                        None => {
                            return Err(AuraExtractionError::InvalidAnnotationValue(
                                "audit_log requires a string literal".to_string(),
                            ))
                        }
                    };
                    effects.push(AuraEffect::AuditLog {
                        action,
                        role: role.clone(),
                    });
                }
                "leak" => {
                    let observers =
                        match item.value {
                            Some(AnnotationValue::IdentList(values)) => values,
                            Some(AnnotationValue::Str(value)) => vec![value],
                            Some(_) => return Err(AuraExtractionError::InvalidAnnotationValue(
                                "leak expects a parenthesized identifier list or string literal"
                                    .to_string(),
                            )),
                            None => {
                                return Err(AuraExtractionError::InvalidAnnotationValue(
                                    "leak requires observers".to_string(),
                                ))
                            }
                        };
                    effects.push(AuraEffect::Leakage {
                        observers,
                        role: role.clone(),
                    });
                }
                "leakage_budget" => match item.value {
                    Some(AnnotationValue::IntList(_values)) => {}
                    Some(_) => {
                        return Err(AuraExtractionError::InvalidAnnotationValue(
                            "leakage_budget expects a list of integers".to_string(),
                        ))
                    }
                    None => {
                        return Err(AuraExtractionError::InvalidAnnotationValue(
                            "leakage_budget requires a list of integers".to_string(),
                        ))
                    }
                },
                other => {
                    return Err(AuraExtractionError::UnsupportedFeature(format!(
                        "Unknown annotation: {other}"
                    )))
                }
            }
        }

        if has_role_annotation && !has_flow_cost {
            effects.push(AuraEffect::FlowCost { cost: 100, role });
        }
    }

    Ok(())
}

fn parse_annotation_list(content: &str) -> Result<Vec<AnnotationItem>, AuraExtractionError> {
    let normalized = content.replace("leak:", "leak =");
    let list: AnnotationList = syn::parse_str(&normalized)
        .map_err(|err| AuraExtractionError::AnnotationParseError(err.to_string()))?;
    Ok(list.items)
}

fn extract_annotation_content(line: &str) -> Option<String> {
    if line.trim_start().starts_with("#[") {
        return None;
    }

    let bytes = line.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }

        let start = index + 1;
        let mut depth = 1usize;
        index += 1;
        while index < bytes.len() && depth > 0 {
            match bytes[index] {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                _ => {}
            }
            index += 1;
        }

        if depth != 0 {
            break;
        }

        let end = index - 1;
        let content = line[start..end].trim();
        if is_annotation_content(content) {
            return Some(content.to_string());
        }
    }
    None
}

fn is_annotation_content(content: &str) -> bool {
    let normalized = content.replace("leak:", "leak =");
    [
        "guard_capability",
        "flow_cost",
        "journal_facts",
        "journal_merge",
        "audit_log",
        "leak",
        "leakage_budget",
    ]
    .iter()
    .any(|key| normalized.contains(key))
}

/// Extract role from a line - simple implementation
fn extract_role_from_line(line: &str) -> Option<RoleId> {
    // Try to find role before brackets like "Alice[guard_capability = ...]"
    if let Some(bracket_pos) = line.find('[') {
        let before_bracket = line[..bracket_pos].trim();
        if !before_bracket.is_empty() {
            return Some(RoleId::new(before_bracket));
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
            role: RoleId::new("TestRole"),
        };

        match guard_effect {
            AuraEffect::GuardCapability { capability, role } => {
                assert_eq!(capability, "test_capability");
                assert_eq!(role.as_str(), "TestRole");
            }
            _ => panic!("Wrong effect type"),
        }
    }

    #[test]
    fn test_flow_cost_effect() {
        let cost_effect = AuraEffect::FlowCost {
            cost: 150,
            role: RoleId::new("Coordinator"),
        };

        match cost_effect {
            AuraEffect::FlowCost { cost, role } => {
                assert_eq!(cost, 150);
                assert_eq!(role.as_str(), "Coordinator");
            }
            _ => panic!("Wrong effect type"),
        }
    }

    #[test]
    fn test_extract_role_from_line() {
        let line = r#"Alice[guard_capability = "send_message"] -> Bob: Message;"#;
        let role = extract_role_from_line(line);
        assert_eq!(
            role.map(|role| role.as_str().to_string()),
            Some("Alice".to_string())
        );
    }

    #[test]
    fn test_guard_capability_annotation() {
        let choreography = r#"
            Alice[guard_capability = "send_message"] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();
        let has_guard = effects.iter().any(|e| {
            matches!(e, AuraEffect::GuardCapability { capability, role }
                if capability == "send_message" && role.as_str() == "Alice")
        });
        assert!(has_guard, "Should extract guard_capability annotation");
    }

    #[test]
    fn test_journal_facts_annotation() {
        let choreography = r#"
            Alice[journal_facts = "message_sent"] -> Bob: Message;
        "#;
        let effects = extract_aura_annotations(choreography).unwrap();
        let has_facts = effects.iter().any(|e| {
            matches!(e, AuraEffect::JournalFacts { facts, role }
                if facts == "message_sent" && role.as_str() == "Alice")
        });
        assert!(has_facts, "Should extract journal_facts annotation");
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
            .any(|e| matches!(e, AuraEffect::JournalMerge { role } if role.as_str() == "Alice"));

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
            .any(|e| matches!(e, AuraEffect::AuditLog { action, role } if action == "message_sent" && role.as_str() == "Alice"));

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
                && role.as_str() == "Alice")
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
                && role.as_str() == "Alice")
        });

        assert!(
            has_leakage,
            "Should extract leak annotation with quoted string"
        );
    }
}
