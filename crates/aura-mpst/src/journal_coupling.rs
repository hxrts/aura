//! Journal Coupling Annotations
//!
//! This module provides annotations for choreographic operations that affect the Journal CRDT.
//! Journal coupling ensures that protocol operations are properly integrated with the
//! distributed state management system.
//!
//! # Syntax
//!
//! Journal coupling uses the syntax: `[▷ Δfacts]` where:
//! - `▷` indicates a journal operation
//! - `Δfacts` describes the change to the facts semilattice
//!
//! Additional variants:
//! - `[▷ facts: δ]` - Specific fact delta
//! - `[▷ caps: γ]` - Capability refinement
//! - `[▷ merge]` - General merge operation
//!
//! # Examples
//!
//! ```ignore
//! // Protocol with journal coupling
//! choreography! {
//!     Alice[▷ facts: new_device_fact] -> Bob: DeviceAdded;
//!     Bob[▷ caps: revoke_access] -> Alice: AccessRevoked;
//! }
//! ```

use async_trait::async_trait;
use aura_core::{AuraError, AuraResult, Journal, JournalEffects};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type of journal operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum JournalOpType {
    /// Add facts (join semilattice operation)
    AddFacts,
    /// Refine capabilities (meet semilattice operation)
    RefineCaps,
    /// General merge operation
    Merge,
    /// Custom operation with description
    Custom(String),
}

/// Journal annotation for protocol operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalAnnotation {
    /// Type of journal operation
    pub op_type: JournalOpType,
    /// Description of the operation
    pub description: Option<String>,
    /// Delta to apply (if applicable)
    pub delta: Option<Journal>,
}

impl JournalAnnotation {
    /// Create a new fact addition annotation
    pub fn add_facts(description: impl Into<String>) -> Self {
        Self {
            op_type: JournalOpType::AddFacts,
            description: Some(description.into()),
            delta: None,
        }
    }

    /// Create a capability refinement annotation
    pub fn refine_caps(description: impl Into<String>) -> Self {
        Self {
            op_type: JournalOpType::RefineCaps,
            description: Some(description.into()),
            delta: None,
        }
    }

    /// Create a merge annotation
    pub fn merge(description: impl Into<String>) -> Self {
        Self {
            op_type: JournalOpType::Merge,
            description: Some(description.into()),
            delta: None,
        }
    }

    /// Create annotation with specific delta
    pub fn with_delta(op_type: JournalOpType, delta: Journal) -> Self {
        Self {
            op_type,
            description: None,
            delta: Some(delta),
        }
    }

    /// Apply this annotation to a journal using effects
    pub async fn apply(
        &self,
        effects: &impl JournalEffects,
        target: &Journal,
    ) -> AuraResult<Journal> {
        match &self.op_type {
            JournalOpType::AddFacts => {
                if let Some(delta) = &self.delta {
                    effects.merge_facts(target, delta).await
                } else {
                    // Without specific delta, return unchanged journal
                    Ok(target.clone())
                }
            }
            JournalOpType::RefineCaps => {
                if let Some(refinement) = &self.delta {
                    effects.refine_caps(target, refinement).await
                } else {
                    Ok(target.clone())
                }
            }
            JournalOpType::Merge => {
                if let Some(delta) = &self.delta {
                    // General merge - apply both facts and caps
                    let with_facts = effects.merge_facts(target, delta).await?;
                    effects.refine_caps(&with_facts, delta).await
                } else {
                    Ok(target.clone())
                }
            }
            JournalOpType::Custom(_) => {
                // Custom operations are application-specific
                Ok(target.clone())
            }
        }
    }
}

/// Delta annotation for specific changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaAnnotation {
    /// The journal delta to apply
    pub delta: Journal,
    /// Operation description
    pub description: String,
}

impl DeltaAnnotation {
    /// Create a new delta annotation
    pub fn new(delta: Journal, description: impl Into<String>) -> Self {
        Self {
            delta,
            description: description.into(),
        }
    }

    /// Apply this delta to a journal
    pub async fn apply(
        &self,
        effects: &impl JournalEffects,
        target: &Journal,
    ) -> AuraResult<Journal> {
        // Apply as a general merge operation
        let with_facts = effects.merge_facts(target, &self.delta).await?;
        effects.refine_caps(&with_facts, &self.delta).await
    }
}

/// Protocol with journal coupling
#[async_trait]
pub trait JournalCoupling {
    /// Get all journal annotations for this protocol
    fn journal_annotations(&self) -> &HashMap<String, JournalAnnotation>;

    /// Apply all journal annotations
    async fn apply_all_annotations(
        &self,
        effects: &(impl JournalEffects + Sync),
        journal: &Journal,
    ) -> AuraResult<Journal> {
        let mut current = journal.clone();

        for (name, annotation) in self.journal_annotations() {
            tracing::debug!("Applying journal annotation: {}", name);
            current = annotation.apply(effects, &current).await.map_err(|e| {
                AuraError::internal(format!("Journal annotation '{}' failed: {}", name, e))
            })?;
        }

        Ok(current)
    }

    /// Validate that all annotations are consistent
    fn validate_annotations(&self) -> AuraResult<()> {
        // Check for conflicting annotations or invalid deltas
        // This is a placeholder for annotation validation logic
        Ok(())
    }
}

/// Journal coupling syntax parser
pub struct JournalCouplingParser;

impl JournalCouplingParser {
    /// Parse journal coupling syntax
    /// Formats:
    /// - "[▷ facts: description]"
    /// - "[▷ caps: description]"
    /// - "[▷ merge]"
    /// - "[▷ Δfacts]"
    pub fn parse(expr: &str) -> AuraResult<JournalAnnotation> {
        let expr = expr.trim();

        if !expr.starts_with("[▷") || !expr.ends_with("]") {
            return Err(AuraError::invalid(
                "Journal annotation must be in format [▷ ...]",
            ));
        }

        let inner = &expr[2..expr.len() - 1].trim();

        if inner.starts_with("facts:") {
            let desc = inner
                .strip_prefix("facts:")
                .expect("already checked with starts_with")
                .trim();
            Ok(JournalAnnotation::add_facts(desc))
        } else if inner.starts_with("caps:") {
            let desc = inner
                .strip_prefix("caps:")
                .expect("already checked with starts_with")
                .trim();
            Ok(JournalAnnotation::refine_caps(desc))
        } else if *inner == "merge" {
            Ok(JournalAnnotation::merge("General merge"))
        } else if inner.starts_with("Δ") {
            let desc = inner
                .strip_prefix("Δ")
                .expect("already checked with starts_with")
                .trim();
            Ok(JournalAnnotation::add_facts(format!("Delta: {}", desc)))
        } else {
            Ok(JournalAnnotation::merge(inner.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_annotation_creation() {
        let annotation = JournalAnnotation::add_facts("Add device fact");
        assert_eq!(annotation.op_type, JournalOpType::AddFacts);
        assert!(annotation.description.is_some());
    }

    #[test]
    fn test_journal_coupling_parser() {
        let annotation = JournalCouplingParser::parse("[▷ facts: new device]").unwrap();
        assert_eq!(annotation.op_type, JournalOpType::AddFacts);

        let annotation = JournalCouplingParser::parse("[▷ caps: revoke access]").unwrap();
        assert_eq!(annotation.op_type, JournalOpType::RefineCaps);

        let annotation = JournalCouplingParser::parse("[▷ merge]").unwrap();
        assert_eq!(annotation.op_type, JournalOpType::Merge);
    }

    #[test]
    fn test_invalid_syntax() {
        assert!(JournalCouplingParser::parse("invalid").is_err());
        assert!(JournalCouplingParser::parse("[invalid]").is_err());
    }

    #[test]
    fn test_delta_annotation() {
        let journal = Journal::new();
        let delta = DeltaAnnotation::new(journal, "Test delta");
        assert_eq!(delta.description, "Test delta");
    }
}
