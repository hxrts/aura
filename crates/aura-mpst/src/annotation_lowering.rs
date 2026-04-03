//! Aura annotation lowering.
//!
//! This module lowers compiled Telltale annotation metadata into Aura-owned
//! effects. Parsing and projection stay upstream; Aura only owns the semantic
//! interpretation layer.

use crate::ids::RoleId;
use crate::upstream::language::{AnnotationScope, CompiledChoreography, ProtocolAnnotationRecord};
use aura_core::{CapabilityName, CapabilityNameError};

const DEFAULT_FLOW_COST: u64 = 100;

/// Aura-specific effect that can be generated from annotations
#[derive(Debug, Clone, PartialEq)]
pub enum AuraEffect {
    /// Capability-based authorization requirement
    GuardCapability {
        /// The required canonical capability name
        capability: CapabilityName,
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
    /// Static link-composition metadata for choreography bundles.
    Link {
        /// Parsed link directive.
        directive: LinkDirective,
        /// The role that declared this annotation.
        role: RoleId,
    },
}

/// Parsed `link` annotation directive.
///
/// Format:
/// `link : "bundle=<id>|exports=a,b|imports=c,d"`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkDirective {
    /// Composed bundle identifier.
    pub bundle_id: String,
    /// Interfaces exported by this bundle.
    pub exports: Vec<String>,
    /// Interfaces imported by this bundle.
    pub imports: Vec<String>,
}

/// Errors that can occur during annotation lowering.
#[derive(Debug, thiserror::Error)]
pub enum AuraExtractionError {
    /// Encountered malformed annotation metadata.
    #[error("annotation lowering error: {0}")]
    AnnotationParseError(String),

    /// Invalid annotation value
    #[error("Invalid annotation value: {0}")]
    InvalidAnnotationValue(String),

    /// Unsupported feature
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
}

/// Errors that can occur while parsing a choreography guard capability.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChoreographyCapabilityError {
    /// Capability grammar validation failed.
    #[error(transparent)]
    InvalidName(#[from] CapabilityNameError),

    /// Choreography capability strings must use canonical namespaced names.
    #[error("choreography guard capabilities must be canonical namespaced values, got `{value}`")]
    MissingNamespace {
        /// The rejected raw capability string.
        value: String,
    },

    /// The legacy `cap:` namespace is not admitted.
    #[error("legacy `cap:` choreography capability namespace is not admitted: `{value}`")]
    LegacyNamespace {
        /// The rejected raw capability string.
        value: String,
    },
}

/// Parse a choreography guard capability at the DSL boundary.
pub fn parse_choreography_capability(
    value: &str,
) -> Result<CapabilityName, ChoreographyCapabilityError> {
    let parsed = CapabilityName::parse(value)?;
    if !parsed.as_str().contains(':') {
        return Err(ChoreographyCapabilityError::MissingNamespace {
            value: value.to_string(),
        });
    }
    if parsed.as_str().starts_with("cap:") {
        return Err(ChoreographyCapabilityError::LegacyNamespace {
            value: value.to_string(),
        });
    }
    Ok(parsed)
}

/// Lower Aura effects from a compiled Telltale choreography.
pub fn lower_aura_effects(
    compiled: &CompiledChoreography,
) -> Result<Vec<AuraEffect>, AuraExtractionError> {
    lower_aura_effects_from_records(&compiled.annotation_records())
}

/// Lower Aura effects from ordered Telltale annotation records.
pub fn lower_aura_effects_from_records(
    records: &[ProtocolAnnotationRecord],
) -> Result<Vec<AuraEffect>, AuraExtractionError> {
    let mut effects = Vec::new();
    let mut current_block: Option<SenderAnnotationBlock> = None;

    for record in records
        .iter()
        .filter(|record| is_sender_annotation_record(record))
    {
        let role = record.role.as_deref().map(RoleId::new).ok_or_else(|| {
            AuraExtractionError::AnnotationParseError(format!(
                "sender annotation record at {} is missing a role",
                record.path
            ))
        })?;

        let block_changed = current_block.as_ref().is_some_and(|block| {
            block.path != record.path || block.node_kind != record.node_kind || block.role != role
        });
        if block_changed {
            flush_sender_annotation_block(&mut current_block, &mut effects);
        }

        if current_block.is_none() {
            current_block = Some(SenderAnnotationBlock::new(
                record.path.clone(),
                record.node_kind.clone(),
                role.clone(),
            ));
        }

        if let Some(block) = current_block.as_mut() {
            parse_sender_annotation_record(block, record)?;
        }
    }

    flush_sender_annotation_block(&mut current_block, &mut effects);
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
            AuraEffect::Link { directive, role } => {
                code.push_str(&format!(
                    "    // Link directive bundle={} exports={:?} imports={:?} role={role}\n",
                    directive.bundle_id, directive.exports, directive.imports
                ));
            }
        }
    }

    code.push_str("}\n");
    code
}

#[derive(Debug)]
struct SenderAnnotationBlock {
    path: String,
    node_kind: String,
    role: RoleId,
    has_explicit_flow_cost: bool,
    effects: Vec<AuraEffect>,
}

impl SenderAnnotationBlock {
    fn new(path: String, node_kind: String, role: RoleId) -> Self {
        Self {
            path,
            node_kind,
            role,
            has_explicit_flow_cost: false,
            effects: Vec::new(),
        }
    }
}

fn is_sender_annotation_record(record: &ProtocolAnnotationRecord) -> bool {
    matches!(record.scope, AnnotationScope::Sender)
        && matches!(record.node_kind.as_str(), "send" | "broadcast")
}

fn flush_sender_annotation_block(
    block: &mut Option<SenderAnnotationBlock>,
    effects: &mut Vec<AuraEffect>,
) {
    if let Some(mut block) = block.take() {
        if !block.has_explicit_flow_cost {
            block.effects.push(AuraEffect::FlowCost {
                cost: DEFAULT_FLOW_COST,
                role: block.role.clone(),
            });
        }
        effects.extend(block.effects);
    }
}

fn parse_sender_annotation_record(
    block: &mut SenderAnnotationBlock,
    record: &ProtocolAnnotationRecord,
) -> Result<(), AuraExtractionError> {
    match record.key.as_str() {
        "guard_capability" => {
            let capability = parse_choreography_capability(&record.value).map_err(|error| {
                AuraExtractionError::InvalidAnnotationValue(format!(
                    "invalid guard_capability `{}`: {error}",
                    record.value
                ))
            })?;
            block.effects.push(AuraEffect::GuardCapability {
                capability,
                role: block.role.clone(),
            });
        }
        "flow_cost" => {
            if block.has_explicit_flow_cost {
                return Err(AuraExtractionError::InvalidAnnotationValue(
                    "flow_cost specified multiple times".to_string(),
                ));
            }
            let cost = parse_u64_annotation_value("flow_cost", &record.value)?;
            block.effects.push(AuraEffect::FlowCost {
                cost,
                role: block.role.clone(),
            });
            block.has_explicit_flow_cost = true;
        }
        "journal_facts" => {
            block.effects.push(AuraEffect::JournalFacts {
                facts: record.value.clone(),
                role: block.role.clone(),
            });
        }
        "journal_merge" => {
            if parse_bool_annotation_value("journal_merge", &record.value)? {
                block.effects.push(AuraEffect::JournalMerge {
                    role: block.role.clone(),
                });
            }
        }
        "audit_log" => {
            block.effects.push(AuraEffect::AuditLog {
                action: record.value.clone(),
                role: block.role.clone(),
            });
        }
        "leak" => {
            block.effects.push(AuraEffect::Leakage {
                observers: parse_leak_observers(&record.value)?,
                role: block.role.clone(),
            });
        }
        "leakage_budget" => {
            parse_leakage_budget_string(&record.value)?;
        }
        "link" => {
            block.effects.push(AuraEffect::Link {
                directive: parse_link_directive(&record.value)?,
                role: block.role.clone(),
            });
        }
        key if is_ignored_upstream_annotation_key(key) => {}
        other => {
            return Err(AuraExtractionError::UnsupportedFeature(format!(
                "Unknown annotation: {other}"
            )))
        }
    }

    Ok(())
}

fn parse_u64_annotation_value(key: &str, value: &str) -> Result<u64, AuraExtractionError> {
    value.parse::<u64>().map_err(|_| {
        AuraExtractionError::InvalidAnnotationValue(format!("{key} expects an integer literal"))
    })
}

fn parse_bool_annotation_value(key: &str, value: &str) -> Result<bool, AuraExtractionError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AuraExtractionError::InvalidAnnotationValue(format!(
            "{key} expects a boolean literal"
        ))),
    }
}

fn parse_leak_observers(value: &str) -> Result<Vec<String>, AuraExtractionError> {
    let trimmed = value.trim();
    if let Some(inner) = trimmed
        .strip_prefix('(')
        .and_then(|value| value.strip_suffix(')'))
    {
        let observers = inner
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if observers.is_empty() {
            return Err(AuraExtractionError::InvalidAnnotationValue(
                "leak requires observers".to_string(),
            ));
        }
        return Ok(observers);
    }

    if trimmed.is_empty() {
        return Err(AuraExtractionError::InvalidAnnotationValue(
            "leak requires observers".to_string(),
        ));
    }

    Ok(vec![trimmed.to_string()])
}

fn is_ignored_upstream_annotation_key(key: &str) -> bool {
    matches!(
        key,
        "timed_choice"
            | "timeout_ms"
            | "priority"
            | "retry"
            | "idempotent"
            | "trace"
            | "runtime_timeout"
            | "heartbeat"
            | "parallel"
            | "ordered"
            | "min_responses"
            | "required_capability"
    )
}

fn parse_link_directive(raw: &str) -> Result<LinkDirective, AuraExtractionError> {
    let mut bundle_id: Option<String> = None;
    let mut exports = Vec::new();
    let mut imports = Vec::new();

    for segment in raw.split(['|', ';']) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let (key, value) = segment.split_once('=').ok_or_else(|| {
            AuraExtractionError::InvalidAnnotationValue(
                "link directive segments must be key=value pairs".to_string(),
            )
        })?;
        let key = key.trim();
        let value = value.trim();
        match key {
            "bundle" => {
                if value.is_empty() {
                    return Err(AuraExtractionError::InvalidAnnotationValue(
                        "link bundle id cannot be empty".to_string(),
                    ));
                }
                bundle_id = Some(value.to_string());
            }
            "exports" => {
                exports = value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(ToString::to_string)
                    .collect();
            }
            "imports" => {
                imports = value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(ToString::to_string)
                    .collect();
            }
            other => {
                return Err(AuraExtractionError::InvalidAnnotationValue(format!(
                    "unsupported link directive key: {other}"
                )))
            }
        }
    }

    let bundle_id = bundle_id.ok_or_else(|| {
        AuraExtractionError::InvalidAnnotationValue(
            "link directive requires bundle=<id>".to_string(),
        )
    })?;

    Ok(LinkDirective {
        bundle_id,
        exports,
        imports,
    })
}

fn parse_leakage_budget_string(raw: &str) -> Result<Vec<u64>, AuraExtractionError> {
    let trimmed = raw.trim();
    let inner = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed)
        .trim();

    if inner.is_empty() {
        return Err(AuraExtractionError::InvalidAnnotationValue(
            "leakage_budget string must contain at least one integer".to_string(),
        ));
    }

    inner
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                AuraExtractionError::InvalidAnnotationValue(format!(
                    "leakage_budget string contains a non-integer entry: {value}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(|values| {
            if values.is_empty() {
                Err(AuraExtractionError::InvalidAnnotationValue(
                    "leakage_budget string must contain at least one integer".to_string(),
                ))
            } else {
                Ok(values)
            }
        })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn test_compiled_choreography(body: &str) -> CompiledChoreography {
        let declared_roles = ["Alice", "Bob", "Coordinator", "Worker"]
            .into_iter()
            .filter(|role| body.contains(role))
            .collect::<Vec<_>>()
            .join(", ");
        crate::upstream::language::compile_choreography(&format!(
            "protocol Extracted =\n  roles {}\n  {}\n",
            declared_roles,
            body.trim(),
        ))
        .expect("test choreography should compile")
    }

    #[test]
    fn test_aura_effect_types() {
        let guard_effect = AuraEffect::GuardCapability {
            capability: parse_choreography_capability("chat:message:send")
                .expect("canonical choreography capability"),
            role: RoleId::new("TestRole"),
        };

        match guard_effect {
            AuraEffect::GuardCapability { capability, role } => {
                assert_eq!(capability.as_str(), "chat:message:send");
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
    fn test_guard_capability_annotation() {
        let compiled = test_compiled_choreography(
            r#"Alice { guard_capability : "chat:message:send" } -> Bob: Message"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();
        let has_guard = effects.iter().any(|e| {
            matches!(e, AuraEffect::GuardCapability { capability, role }
                if capability.as_str() == "chat:message:send" && role.as_str() == "Alice")
        });
        assert!(has_guard, "Should extract guard_capability annotation");
    }

    #[test]
    fn test_journal_facts_annotation() {
        let compiled = test_compiled_choreography(
            r#"Alice { journal_facts : "message_sent" } -> Bob: Message"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();
        let has_facts = effects.iter().any(|e| {
            matches!(e, AuraEffect::JournalFacts { facts, role }
                if facts == "message_sent" && role.as_str() == "Alice")
        });
        assert!(has_facts, "Should extract journal_facts annotation");
    }

    #[test]
    fn test_default_flow_cost() {
        // Line with annotation but no flow_cost should get default of 100
        let compiled = test_compiled_choreography(
            r#"Alice { guard_capability : "chat:message:send" } -> Bob: Message"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();

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
        let compiled = test_compiled_choreography(
            r#"Alice { guard_capability : "chat:message:send", flow_cost : 250 } -> Bob: Message"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();

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
        let compiled = test_compiled_choreography(r#"Alice -> Bob: SimpleMessage"#);
        let effects = lower_aura_effects(&compiled).unwrap();

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
        let compiled =
            test_compiled_choreography(r#"Alice { journal_merge : true } -> Bob: MergeRequest"#);
        let effects = lower_aura_effects(&compiled).unwrap();

        let has_journal_merge = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::JournalMerge { role } if role.as_str() == "Alice"));

        assert!(has_journal_merge, "Should extract journal_merge annotation");
    }

    #[test]
    fn test_audit_log_annotation() {
        let compiled =
            test_compiled_choreography(r#"Alice { audit_log : "message_sent" } -> Bob: Message"#);
        let effects = lower_aura_effects(&compiled).unwrap();

        let has_audit_log = effects
            .iter()
            .any(|e| matches!(e, AuraEffect::AuditLog { action, role } if action == "message_sent" && role.as_str() == "Alice"));

        assert!(has_audit_log, "Should extract audit_log annotation");
    }

    #[test]
    fn test_leak_annotation_parentheses() {
        let compiled =
            test_compiled_choreography(r#"Alice { leak: (External, Neighbor) } -> Bob: Message"#);
        let effects = lower_aura_effects(&compiled).unwrap();

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
        let compiled = test_compiled_choreography(r#"Alice { leak : "External" } -> Bob: Message"#);
        let effects = lower_aura_effects(&compiled).unwrap();

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

    #[test]
    fn test_link_annotation_parsing() {
        let compiled = test_compiled_choreography(
            r#"Coordinator { link : "bundle=sync_chat|exports=chat.send,sync.push|imports=journal.commit" } -> Worker: Step"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();
        let has_link = effects.iter().any(|effect| {
            matches!(effect, AuraEffect::Link { directive, role }
                if directive.bundle_id == "sync_chat"
                && directive.exports.contains(&"chat.send".to_string())
                && directive.imports.contains(&"journal.commit".to_string())
                && role.as_str() == "Coordinator")
        });
        assert!(has_link, "Should parse link annotation directive");
    }

    #[test]
    fn test_link_annotation_requires_bundle_key() {
        let compiled = test_compiled_choreography(
            r#"Coordinator { link : "exports=chat.send|imports=sync.push" } -> Worker: Step"#,
        );
        let err = lower_aura_effects(&compiled).expect_err("missing bundle must fail");
        assert!(
            err.to_string().contains("bundle=<id>"),
            "error should mention required bundle key"
        );
    }

    #[test]
    fn test_link_annotation_record_syntax() {
        let compiled = test_compiled_choreography(
            r#"Coordinator { link : "bundle=sync_chat|exports=chat.send,sync.push|imports=journal.commit" } -> Worker : Step of crate.demo.Payload"#,
        );
        let effects = lower_aura_effects(&compiled).unwrap();
        let has_link = effects.iter().any(|effect| {
            matches!(effect, AuraEffect::Link { directive, role }
                if directive.bundle_id == "sync_chat"
                && directive.exports.contains(&"chat.send".to_string())
                && directive.imports.contains(&"journal.commit".to_string())
                && role.as_str() == "Coordinator")
        });
        assert!(
            has_link,
            "Should parse record-style link annotation directive"
        );
    }

    #[test]
    fn test_leakage_budget_annotation_quoted() {
        let compiled =
            test_compiled_choreography(r#"Alice { leakage_budget : "1, 0, 0" } -> Bob: Message"#);
        let effects = lower_aura_effects(&compiled).unwrap();
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, AuraEffect::FlowCost { cost: 100, .. })),
            "quoted leakage_budget should parse without breaking annotation lowering"
        );
    }

    #[test]
    fn test_parse_choreography_capability_rejects_unnamespaced_legacy_value() {
        let err = parse_choreography_capability("send_message").expect_err("legacy name must fail");
        assert!(matches!(
            err,
            ChoreographyCapabilityError::MissingNamespace { .. }
        ));
    }

    #[test]
    fn test_parse_choreography_capability_rejects_legacy_cap_namespace() {
        let err = parse_choreography_capability("cap:amp_send")
            .expect_err("legacy cap namespace must fail");
        assert!(matches!(
            err,
            ChoreographyCapabilityError::LegacyNamespace { .. }
        ));
    }

    #[test]
    fn test_lower_effects_from_compiled_choreography() {
        let compiled = crate::upstream::language::compile_choreography(
            r#"
protocol Guarded =
  roles Alice, Bob
  Alice { guard_capability : "chat:message:send", flow_cost : 42 } -> Bob : Message
"#,
        )
        .expect("choreography should compile");

        let effects = lower_aura_effects(&compiled).expect("compiled annotations should parse");

        assert!(effects.iter().any(|effect| {
            matches!(
                effect,
                AuraEffect::GuardCapability { capability, role }
                    if capability.as_str() == "chat:message:send" && role.as_str() == "Alice"
            )
        }));
        assert!(effects.iter().any(
            |effect| matches!(effect, AuraEffect::FlowCost { cost: 42, role } if role.as_str() == "Alice")
        ));
    }
}
