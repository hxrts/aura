//! Quint AST Parsing
//!
//! Parses Quint CLI output and AST structures to extract meaningful
//! property and specification information for the simulation framework.

use crate::quint::cli_runner::{QuintDefinition, QuintModule, QuintParseOutput};
use crate::quint::types::{
    self, QuintInvariant, QuintSpec, QuintTemporalProperty, QuintSafetyProperty, SafetyPropertyType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// AST parsing errors
#[derive(Error, Debug)]
pub enum AstParseError {
    #[error("Invalid AST structure: {0}")]
    InvalidStructure(String),

    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),

    #[error("Type resolution failed: {0}")]
    TypeResolution(String),
}

/// Result type for AST parsing operations
pub type AstParseResult<T> = Result<T, AstParseError>;

/// Enhanced Quint definition with parsed metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQuintDefinition {
    /// Definition name identifier
    pub name: String,
    /// Definition kind (definition, value, assumption, import)
    pub kind: String,
    /// Expression body or source code
    pub expression: String,
    /// Optional return type annotation
    pub return_type: Option<String>,
    /// Parameter names extracted from expression
    pub parameters: Vec<String>,
    /// Parsed annotations from comments
    pub annotations: HashMap<String, String>,
    /// Whether this definition is a property check
    pub is_property: bool,
    /// Optional property type classification
    pub property_type: Option<PropertyType>,
}

/// Property type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyType {
    /// State invariant that must hold in all reachable states
    Invariant,
    /// Safety property ensuring nothing bad happens
    Safety,
    /// Liveness property ensuring something good eventually happens
    Liveness,
    /// Temporal property expressing eventual occurrence
    Eventually,
    /// Temporal property expressing continuous truth
    Always,
    /// Temporal property expressing conditional eventual occurrence
    Until,
}

/// AST parser for Quint specifications
pub struct QuintAstParser {
    /// Enable strict parsing mode (fail on unrecognized constructs)
    _strict_mode: bool,
    /// Custom annotation prefixes recognized in comments
    annotation_prefixes: Vec<String>,
}

impl QuintAstParser {
    /// Create a new AST parser
    pub fn new(strict_mode: bool) -> Self {
        Self {
            _strict_mode: strict_mode,
            annotation_prefixes: vec![
                "property".to_string(),
                "invariant".to_string(),
                "safety".to_string(),
                "liveness".to_string(),
                "temporal".to_string(),
            ],
        }
    }

    /// Parse a complete Quint specification from CLI output
    pub fn parse_specification(
        &self,
        parse_output: QuintParseOutput,
        spec_name: String,
    ) -> AstParseResult<QuintSpec> {
        let mut invariants = Vec::new();
        let mut temporal_properties = Vec::new();
        let mut modules = Vec::new();

        for module in parse_output.modules {
            let parsed_module = self.parse_module(&module)?;

            // Extract properties from the module
            for def in &parsed_module.definitions {
                if def.is_property {
                    match def.property_type {
                        Some(PropertyType::Invariant) => {
                            invariants.push(self.definition_to_invariant(def)?);
                        }
                        Some(PropertyType::Always)
                        | Some(PropertyType::Eventually)
                        | Some(PropertyType::Until) => {
                            temporal_properties.push(self.definition_to_temporal_property(def)?);
                        }
                        _ => {
                            // Handle other property types
                        }
                    }
                }
            }

            modules.push(parsed_module);
        }

        Ok(QuintSpec {
            name: spec_name,
            file_path: std::path::PathBuf::from("ast_parser"),
            module_name: "main".to_string(),
            version: "1.0".to_string(),
            description: "Parsed from Quint CLI output".to_string(),
            modules: modules
                .into_iter()
                .map(|p| {
                    let definitions = p
                        .definitions
                        .into_iter()
                        .filter_map(|def| self.parsed_definition_to_types(def).transpose())
                        .collect::<AstParseResult<Vec<_>>>()?;

                    Ok(types::QuintModule {
                        name: p.name,
                        definitions,
                    })
                })
                .collect::<AstParseResult<Vec<_>>>()?,
            metadata: HashMap::new(),
            invariants,
            temporal_properties,
            safety_properties: vec![], // Could be extracted similarly
            state_variables: Vec::new(),
            actions: Vec::new(),
        })
    }

    /// Parse a single Quint module
    fn parse_module(&self, module: &QuintModule) -> AstParseResult<ParsedQuintModule> {
        let mut definitions = Vec::new();

        for def in &module.definitions {
            let parsed_def = self.parse_definition(def)?;
            definitions.push(parsed_def);
        }

        Ok(ParsedQuintModule {
            name: module.name.clone(),
            definitions,
        })
    }

    /// Parse a single definition and classify it
    fn parse_definition(
        &self,
        definition: &QuintDefinition,
    ) -> AstParseResult<ParsedQuintDefinition> {
        match definition {
            QuintDefinition::Definition {
                name,
                def_type,
                body,
            } => {
                let expression = body.clone().unwrap_or_default();
                let (is_property, property_type) = self.classify_property(name, &expression);
                let annotations = self.extract_annotations(&expression);

                Ok(ParsedQuintDefinition {
                    name: name.clone(),
                    kind: "definition".to_string(),
                    expression: expression.clone(),
                    return_type: Some(def_type.clone()),
                    parameters: self.extract_parameters(&expression),
                    annotations,
                    is_property,
                    property_type,
                })
            }
            QuintDefinition::Value {
                name,
                val_type,
                expr,
            } => {
                let (is_property, property_type) = self.classify_property(name, expr);
                let annotations = self.extract_annotations(expr);

                Ok(ParsedQuintDefinition {
                    name: name.clone(),
                    kind: "value".to_string(),
                    expression: expr.clone(),
                    return_type: Some(val_type.clone()),
                    parameters: vec![],
                    annotations,
                    is_property,
                    property_type,
                })
            }
            QuintDefinition::Assumption { name, expr } => {
                let assumption_name = name
                    .clone()
                    .unwrap_or_else(|| "unnamed_assumption".to_string());
                let annotations = self.extract_annotations(expr);

                Ok(ParsedQuintDefinition {
                    name: assumption_name,
                    kind: "assumption".to_string(),
                    expression: expr.clone(),
                    return_type: Some("Bool".to_string()),
                    parameters: vec![],
                    annotations,
                    is_property: true,
                    property_type: Some(PropertyType::Invariant),
                })
            }
            QuintDefinition::Import { name, from } => Ok(ParsedQuintDefinition {
                name: name.clone(),
                kind: "import".to_string(),
                expression: format!("from {}", from),
                return_type: None,
                parameters: vec![],
                annotations: HashMap::new(),
                is_property: false,
                property_type: None,
            }),
        }
    }

    /// Classify whether a definition is a property and what type
    fn classify_property(&self, name: &str, expression: &str) -> (bool, Option<PropertyType>) {
        // Check name patterns
        if name.starts_with("inv_") || name.contains("invariant") {
            return (true, Some(PropertyType::Invariant));
        }

        if name.starts_with("safety_") || name.contains("safety") {
            return (true, Some(PropertyType::Safety));
        }

        if name.starts_with("liveness_") || name.contains("liveness") {
            return (true, Some(PropertyType::Liveness));
        }

        // Check expression patterns
        if expression.contains("always") || expression.contains("□") {
            return (true, Some(PropertyType::Always));
        }

        if expression.contains("eventually") || expression.contains("◇") {
            return (true, Some(PropertyType::Eventually));
        }

        if expression.contains("until") || expression.contains("U") {
            return (true, Some(PropertyType::Until));
        }

        // Check for boolean-valued expressions that could be invariants
        if expression.contains("==")
            || expression.contains("!=")
            || expression.contains("and")
            || expression.contains("or")
            || expression.contains("implies")
        {
            return (true, Some(PropertyType::Invariant));
        }

        (false, None)
    }

    /// Extract annotations from expression comments
    fn extract_annotations(&self, expression: &str) -> HashMap<String, String> {
        let mut annotations = HashMap::new();

        // Look for comments with annotation patterns
        for line in expression.lines() {
            if let Some(comment_start) = line.find("//") {
                let comment = &line[comment_start + 2..].trim();

                for prefix in &self.annotation_prefixes {
                    if comment.starts_with(prefix) {
                        let annotation_content = comment[prefix.len()..].trim();
                        if let Some(colon_pos) = annotation_content.find(':') {
                            let key = annotation_content[..colon_pos].trim().to_string();
                            let value = annotation_content[colon_pos + 1..].trim().to_string();
                            annotations.insert(key, value);
                        } else {
                            annotations.insert(prefix.clone(), annotation_content.to_string());
                        }
                    }
                }
            }
        }

        annotations
    }

    /// Extract parameter names from expression
    fn extract_parameters(&self, expression: &str) -> Vec<String> {
        // Simple parameter extraction (would be more sophisticated in practice)
        let mut parameters = Vec::new();

        // Look for lambda expressions or function definitions
        if let Some(arrow_pos) = expression.find("=>") {
            let param_part = &expression[..arrow_pos];
            if let Some(pipe_start) = param_part.find('|') {
                if let Some(pipe_end) = param_part[pipe_start + 1..].find('|') {
                    let params_str = &param_part[pipe_start + 1..pipe_start + 1 + pipe_end];
                    for param in params_str.split(',') {
                        parameters.push(param.trim().to_string());
                    }
                }
            }
        }

        parameters
    }

    /// Convert parsed definition to QuintInvariant
    fn definition_to_invariant(
        &self,
        def: &ParsedQuintDefinition,
    ) -> AstParseResult<QuintInvariant> {
        let description = def
            .annotations
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("Invariant: {}", def.name));

        let tags = def
            .annotations
            .get("tags")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["auto-extracted".to_string()]);

        Ok(QuintInvariant {
            name: def.name.clone(),
            description,
            expression: def.expression.clone(),
            source_location: "ast_parser".to_string(),
            enabled: true,
            tags,
        })
    }

    /// Convert parsed definition to QuintTemporalProperty
    fn definition_to_temporal_property(
        &self,
        def: &ParsedQuintDefinition,
    ) -> AstParseResult<QuintTemporalProperty> {
        let property_type = match def.property_type {
            Some(PropertyType::Always) => "Always".to_string(),
            Some(PropertyType::Eventually) => "Eventually".to_string(),
            Some(PropertyType::Until) => "Until".to_string(),
            _ => "Always".to_string(), // Default
        };

        let description = def
            .annotations
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("Temporal property: {}", def.name));

        let tags = def
            .annotations
            .get("tags")
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["auto-extracted".to_string()]);

        Ok(QuintTemporalProperty {
            name: def.name.clone(),
            description,
            property_type,
            expression: def.expression.clone(),
            source_location: "ast_parser".to_string(),
            enabled: true,
            tags,
        })
    }

    /// Convert a parsed definition into the simulator-friendly QuintDefinition enum
    fn parsed_definition_to_types(
        &self,
        def: ParsedQuintDefinition,
    ) -> AstParseResult<Option<types::QuintDefinition>> {
        if !def.is_property {
            return Ok(None);
        }

        match def.property_type {
            Some(PropertyType::Invariant) => Ok(Some(types::QuintDefinition::Invariant(
                self.definition_to_invariant(&def)?,
            ))),
            Some(PropertyType::Always) | Some(PropertyType::Eventually) | Some(PropertyType::Until) => {
                Ok(Some(types::QuintDefinition::Temporal(
                    self.definition_to_temporal_property(&def)?,
                )))
            }
            Some(PropertyType::Safety) => {
                let description = def
                    .annotations
                    .get("description")
                    .cloned()
                    .unwrap_or_else(|| format!("Safety property: {}", def.name));

                Ok(Some(types::QuintDefinition::Safety(QuintSafetyProperty {
                    name: def.name,
                    expression: def.expression,
                    description,
                    source_location: "ast_parser".to_string(),
                    safety_type: SafetyPropertyType::General,
                    monitored_variables: vec![],
                })))
            }
            Some(PropertyType::Liveness) => {
                let description = def
                    .annotations
                    .get("description")
                    .cloned()
                    .unwrap_or_else(|| format!("Temporal property: {}", def.name.clone()));

                let tags = def
                    .annotations
                    .get("tags")
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect())
                    .unwrap_or_else(|| vec!["auto-extracted".to_string()]);

                Ok(Some(types::QuintDefinition::Temporal(
                    QuintTemporalProperty {
                        name: def.name,
                        description,
                        property_type: "Liveness".to_string(),
                        expression: def.expression,
                        source_location: "ast_parser".to_string(),
                        enabled: true,
                        tags,
                    },
                )))
            }
            None => Ok(None),
        }
    }
}

/// Parsed Quint module with enhanced metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQuintModule {
    /// Module name identifier
    pub name: String,
    /// Parsed definitions with extracted metadata
    pub definitions: Vec<ParsedQuintDefinition>,
}

impl ParsedQuintModule {
    /// Get all property definitions from this module
    pub fn get_properties(&self) -> Vec<&ParsedQuintDefinition> {
        self.definitions
            .iter()
            .filter(|def| def.is_property)
            .collect()
    }

    /// Get properties by type
    pub fn get_properties_by_type(
        &self,
        property_type: PropertyType,
    ) -> Vec<&ParsedQuintDefinition> {
        self.definitions
            .iter()
            .filter(|def| def.property_type == Some(property_type))
            .collect()
    }
}
