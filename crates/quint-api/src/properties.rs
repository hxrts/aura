//! Property specification and management for Quint

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for a property specification
pub type PropertyId = Uuid;

/// Type information for Quint expressions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuintType {
    /// Boolean type
    Bool,
    /// Integer type
    Int,
    /// String type
    Str,
    /// Set type
    Set(Box<QuintType>),
    /// Record type
    Record(HashMap<String, QuintType>),
    /// Function type
    Function {
        /// Parameter types
        params: Vec<QuintType>,
        /// Return type
        result: Box<QuintType>,
    },
    /// Union type
    Union(Vec<QuintType>),
    /// Custom type
    Custom {
        /// Type name
        name: String,
        /// Type parameters
        params: Vec<QuintType>,
    },
}

/// A property specification for formal verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySpec {
    /// Unique identifier for this property
    pub id: PropertyId,
    /// Human-readable name for the property
    pub name: String,
    /// Property description
    pub description: Option<String>,
    /// Property kind (invariant, temporal, etc.)
    pub kind: PropertyKind,
    /// The actual property expression in Quint syntax
    pub expression: String,
    /// Context variables and their types
    pub context: HashMap<String, QuintType>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Property metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Path to the spec file (for file-based verification)
    pub spec_file: String,
    /// List of property names to verify
    pub properties: Vec<String>,
}

impl PropertySpec {
    /// Create a new property specification
    #[allow(clippy::disallowed_methods)]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            kind: PropertyKind::Invariant,
            expression: String::new(),
            context: HashMap::new(),
            tags: Vec::new(),
            metadata: HashMap::new(),
            spec_file: String::new(),
            properties: Vec::new(),
        }
    }

    /// Set the property description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the property kind
    pub fn with_kind(mut self, kind: PropertyKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set the property expression (invariant condition)
    pub fn with_invariant(mut self, expression: impl Into<String>) -> Self {
        self.kind = PropertyKind::Invariant;
        self.expression = expression.into();
        self
    }

    /// Set a temporal property expression
    pub fn with_temporal(mut self, expression: impl Into<String>) -> Self {
        self.kind = PropertyKind::Temporal;
        self.expression = expression.into();
        self
    }

    /// Add a context variable with its type
    pub fn with_context(mut self, name: impl Into<String>, var_type: impl Into<QuintType>) -> Self {
        self.context.insert(name.into(), var_type.into());
        self
    }

    /// Add multiple context variables
    pub fn with_contexts(mut self, contexts: HashMap<String, QuintType>) -> Self {
        self.context.extend(contexts);
        self
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Add multiple tags
    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// Add metadata
    pub fn with_metadata(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Set the spec file path
    pub fn with_spec_file(mut self, spec_file: impl Into<String>) -> Self {
        self.spec_file = spec_file.into();
        self
    }

    /// Add a property name to verify
    pub fn with_property(mut self, property: impl Into<String>) -> Self {
        self.properties.push(property.into());
        self
    }

    /// Generate the complete Quint specification for this property
    pub fn to_quint_spec(&self) -> String {
        let mut spec = String::new();

        // Add module declaration
        spec.push_str(&format!("module {} {{\n", self.name.replace(" ", "_")));

        // Add context variables as constants or variables
        for (name, var_type) in &self.context {
            let type_str = quint_type_to_string(var_type);
            spec.push_str(&format!("  const {}: {}\n", name, type_str));
        }

        // Add the property definition
        match self.kind {
            PropertyKind::Invariant => {
                spec.push_str(&format!(
                    "  inv {}: {}\n",
                    self.name.replace(" ", "_"),
                    self.expression
                ));
            }
            PropertyKind::Temporal => {
                spec.push_str(&format!(
                    "  temporal {}: {}\n",
                    self.name.replace(" ", "_"),
                    self.expression
                ));
            }
            PropertyKind::Safety => {
                spec.push_str(&format!(
                    "  inv {}: {}\n",
                    self.name.replace(" ", "_"),
                    self.expression
                ));
            }
            PropertyKind::Liveness => {
                spec.push_str(&format!(
                    "  temporal {}: {}\n",
                    self.name.replace(" ", "_"),
                    self.expression
                ));
            }
        }

        spec.push_str("}\n");
        spec
    }
}

/// Type of property for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyKind {
    /// Invariant property (always holds)
    Invariant,
    /// Temporal property (eventually/always with time)
    Temporal,
    /// Safety property (nothing bad happens)
    Safety,
    /// Liveness property (something good eventually happens)
    Liveness,
}

/// Collection of related properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertySuite {
    /// Suite identifier
    pub id: Uuid,
    /// Suite name
    pub name: String,
    /// Suite description
    pub description: Option<String>,
    /// Properties in this suite
    pub properties: Vec<PropertySpec>,
    /// Shared context across all properties
    pub shared_context: HashMap<String, QuintType>,
    /// Suite metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl PropertySuite {
    /// Create a new property suite
    #[allow(clippy::disallowed_methods)]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: None,
            properties: Vec::new(),
            shared_context: HashMap::new(),
            metadata: HashMap::new(),
        }
    }

    /// Add a property to the suite
    pub fn add_property(mut self, property: PropertySpec) -> Self {
        self.properties.push(property);
        self
    }

    /// Add shared context variable
    pub fn with_shared_context(
        mut self,
        name: impl Into<String>,
        var_type: impl Into<QuintType>,
    ) -> Self {
        self.shared_context.insert(name.into(), var_type.into());
        self
    }

    /// Generate complete Quint module for the entire suite
    pub fn to_quint_module(&self) -> String {
        let mut module = String::new();

        // Module header
        module.push_str(&format!("module {} {{\n", self.name.replace(" ", "_")));

        // Shared context
        for (name, var_type) in &self.shared_context {
            let type_str = quint_type_to_string(var_type);
            module.push_str(&format!("  const {}: {}\n", name, type_str));
        }

        // Individual properties
        for property in &self.properties {
            module.push('\n');

            // Property-specific context
            for (name, var_type) in &property.context {
                if !self.shared_context.contains_key(name) {
                    let type_str = quint_type_to_string(var_type);
                    module.push_str(&format!("  const {}: {}\n", name, type_str));
                }
            }

            // Property definition
            match property.kind {
                PropertyKind::Invariant | PropertyKind::Safety => {
                    module.push_str(&format!(
                        "  inv {}: {}\n",
                        property.name.replace(" ", "_"),
                        property.expression
                    ));
                }
                PropertyKind::Temporal | PropertyKind::Liveness => {
                    module.push_str(&format!(
                        "  temporal {}: {}\n",
                        property.name.replace(" ", "_"),
                        property.expression
                    ));
                }
            }
        }

        module.push_str("}\n");
        module
    }
}

/// Convert QuintType to Quint syntax string
fn quint_type_to_string(quint_type: &QuintType) -> String {
    match quint_type {
        QuintType::Bool => "bool".to_string(),
        QuintType::Int => "int".to_string(),
        QuintType::Str => "str".to_string(),
        QuintType::Set(inner) => format!("Set[{}]", quint_type_to_string(inner)),
        QuintType::Record(fields) => {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(name, typ)| format!("{}: {}", name, quint_type_to_string(typ)))
                .collect();
            format!("{{ {} }}", field_strs.join(", "))
        }
        QuintType::Function { params, result } => {
            let param_strs: Vec<String> = params.iter().map(quint_type_to_string).collect();
            format!(
                "({}) => {}",
                param_strs.join(", "),
                quint_type_to_string(result)
            )
        }
        QuintType::Union(types) => {
            let type_strs: Vec<String> = types.iter().map(quint_type_to_string).collect();
            type_strs.join(" | ")
        }
        QuintType::Custom { name, params } => {
            if params.is_empty() {
                name.clone()
            } else {
                let param_strs: Vec<String> =
                    params.iter().map(quint_type_to_string).collect();
                format!("{}[{}]", name, param_strs.join(", "))
            }
        }
    }
}

impl From<&str> for QuintType {
    fn from(s: &str) -> Self {
        match s {
            "bool" => QuintType::Bool,
            "int" => QuintType::Int,
            "str" => QuintType::Str,
            _ => QuintType::Custom {
                name: s.to_string(),
                params: Vec::new(),
            },
        }
    }
}

impl From<String> for QuintType {
    fn from(s: String) -> Self {
        QuintType::from(s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_property_spec_creation() {
        let property = PropertySpec::new("counter_non_negative")
            .with_description("Counter should never be negative")
            .with_invariant("counter >= 0")
            .with_context("counter", QuintType::Int)
            .with_tag("safety");

        assert_eq!(property.name, "counter_non_negative");
        assert!(property.description.is_some());
        assert_eq!(property.expression, "counter >= 0");
        assert!(matches!(property.kind, PropertyKind::Invariant));
        assert!(property.context.contains_key("counter"));
        assert!(property.tags.contains(&"safety".to_string()));
    }

    #[test]
    fn test_quint_spec_generation() {
        let property = PropertySpec::new("test_property")
            .with_invariant("x > 0")
            .with_context("x", QuintType::Int);

        let spec = property.to_quint_spec();
        assert!(spec.contains("module test_property"));
        assert!(spec.contains("const x: int"));
        assert!(spec.contains("inv test_property: x > 0"));
    }

    #[test]
    fn test_property_suite() {
        let property1 = PropertySpec::new("prop1")
            .with_invariant("x >= 0")
            .with_context("x", QuintType::Int);

        let property2 = PropertySpec::new("prop2")
            .with_invariant("y < 100")
            .with_context("y", QuintType::Int);

        let suite = PropertySuite::new("test_suite")
            .with_shared_context("shared_var", QuintType::Bool)
            .add_property(property1)
            .add_property(property2);

        let module = suite.to_quint_module();
        assert!(module.contains("module test_suite"));
        assert!(module.contains("const shared_var: bool"));
        assert!(module.contains("inv prop1: x >= 0"));
        assert!(module.contains("inv prop2: y < 100"));
    }
}
