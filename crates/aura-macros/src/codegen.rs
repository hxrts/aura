//! Aura-specific code generation for choreographies

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{Ident, Result};

use crate::{
    annotations::{AuraAnnotation, FlowCost, GuardCapability, JournalFacts, LeakageBudget},
    parsing::{AuraProtocol, Interaction},
};

/// Generates Aura-specific integration code for choreographic protocols
pub struct AuraChoreographyGenerator {
    protocol: AuraProtocol,
}

impl AuraChoreographyGenerator {
    /// Create a new generator for the given protocol
    pub fn new(protocol: AuraProtocol) -> Self {
        Self { protocol }
    }

    /// Generate the complete Aura integration code
    pub fn generate(&self) -> Result<TokenStream> {
        let namespace_mod = self.get_namespace_module();
        let guard_profiles = self.generate_guard_profiles()?;
        let journal_coupling = self.generate_journal_coupling()?;
        let execution_functions = self.generate_execution_functions()?;
        let adapter_helpers = self.generate_adapter_helpers()?;

        Ok(quote! {
            pub mod #namespace_mod {
                //! Generated Aura choreography integration for the protocol
                //!
                //! This module contains generated guard profiles, configuration types,
                //! journal coupling functions, and helper functions for the choreographic protocol.

                use std::collections::HashMap;
                use serde_json::Value as JsonValue;

                /// Send guard profile for choreographic messages
                #[derive(Debug, Clone)]
                pub struct SendGuardProfile {
                    pub capabilities: Vec<String>,
                    pub flow_cost: u32,
                    pub delta_facts: Vec<serde_json::Value>,
                    pub leakage_external: u32,
                    pub leakage_neighbor: u32,
                    pub leakage_ingroup: u32,
                }

                impl Default for SendGuardProfile {
                    fn default() -> Self {
                        Self {
                            capabilities: Vec::new(),
                            flow_cost: 1,
                            delta_facts: Vec::new(),
                            leakage_external: 0,
                            leakage_neighbor: 0,
                            leakage_ingroup: 0,
                        }
                    }
                }

                #guard_profiles
                #journal_coupling
                #adapter_helpers
                #execution_functions
            }
        })
    }

    fn get_namespace_module(&self) -> Ident {
        let namespace = self.protocol.namespace.as_deref().unwrap_or("protocol");
        format_ident!("{}", namespace)
    }

    /// Generate guard profiles for annotated interactions
    fn generate_guard_profiles(&self) -> Result<TokenStream> {
        let mut profiles = Vec::new();
        let mut seen_messages = std::collections::HashSet::new();

        for interaction in &self.protocol.interactions {
            if interaction.has_annotations() {
                let message_name = &interaction.message_name;
                // Only generate guard profile once per message type
                if !seen_messages.contains(&message_name.to_string()) {
                    seen_messages.insert(message_name.to_string());
                    if let Some(profile) =
                        self.generate_guard_profile_for_interaction(interaction)?
                    {
                        profiles.push(profile);
                    }
                }
            }
        }

        Ok(quote! {
            #(#profiles)*
        })
    }

    fn generate_guard_profile_for_interaction(
        &self,
        interaction: &Interaction,
    ) -> Result<Option<TokenStream>> {
        let message_name = &interaction.message_name;
        let profile_fn_name =
            format_ident!("{}_guard_profile", message_name.to_string().to_lowercase());

        let mut capabilities = Vec::new();
        let mut flow_cost = 1u32;
        let mut leakage_budget = None;
        let mut journal_operations = Vec::new();

        for annotation in &interaction.annotations {
            match annotation {
                AuraAnnotation::GuardCapability(GuardCapability { capability }) => {
                    capabilities.push(capability.clone());
                }
                AuraAnnotation::FlowCost(FlowCost { cost }) => {
                    flow_cost = *cost;
                }
                AuraAnnotation::LeakageBudget(LeakageBudget {
                    external,
                    neighbor,
                    ingroup,
                }) => {
                    leakage_budget = Some((*external, *neighbor, *ingroup));
                }
                AuraAnnotation::JournalFacts(JournalFacts { description }) => {
                    journal_operations.push(format!("add_facts: {}", description));
                }
                AuraAnnotation::JournalMerge(_) => {
                    journal_operations.push("merge".to_string());
                }
            }
        }

        let capability_setup = capabilities.iter().map(|cap| {
            quote! { #cap.to_string() }
        });

        let (external, neighbor, ingroup) = leakage_budget.unwrap_or((0, 0, 0));

        let delta_facts = journal_operations.iter().map(|op| {
            quote! {
                serde_json::json!({
                    "type": "choreography_operation",
                    "operation": #op,
                    "message_type": stringify!(#message_name),
                    "timestamp": 0u64
                })
            }
        });

        Ok(Some(quote! {
            /// Guard profile for #message_name interactions
            pub fn #profile_fn_name() -> SendGuardProfile {
                SendGuardProfile {
                    capabilities: vec![#(#capability_setup),*],
                    flow_cost: #flow_cost,
                    delta_facts: vec![#(#delta_facts),*],
                    leakage_external: #external,
                    leakage_neighbor: #neighbor,
                    leakage_ingroup: #ingroup,
                }
            }
        }))
    }

    /// Generate journal coupling functions based on annotations
    fn generate_journal_coupling(&self) -> Result<TokenStream> {
        let mut coupling_functions = Vec::new();
        let mut seen_messages = std::collections::HashSet::new();

        // Generate a journal coupling function for each annotated interaction (deduplicated)
        for interaction in &self.protocol.interactions {
            if interaction.has_annotations() {
                let message_name = &interaction.message_name;
                if !seen_messages.contains(&message_name.to_string()) {
                    seen_messages.insert(message_name.to_string());
                    if let Some(function) =
                        self.generate_journal_coupling_for_interaction(interaction)?
                    {
                        coupling_functions.push(function);
                    }
                }
            }
        }

        // Generate a master journal operations collector
        let annotated_interactions: Vec<_> = self
            .protocol
            .interactions
            .iter()
            .filter(|i| {
                i.has_annotations()
                    && i.annotations.iter().any(|a| {
                        matches!(
                            a,
                            AuraAnnotation::JournalFacts(_) | AuraAnnotation::JournalMerge(_)
                        )
                    })
            })
            .collect();

        let interaction_names: Vec<_> = annotated_interactions
            .iter()
            .map(|i| &i.message_name)
            .collect();

        let journal_fn_names: Vec<_> = interaction_names
            .iter()
            .map(|name| format_ident!("{}_journal_operations", name.to_string().to_lowercase()))
            .collect();

        let master_journal = if !interaction_names.is_empty() {
            quote! {
                /// Get all journal operation descriptions for the protocol
                pub fn get_protocol_journal_operations() -> Vec<(String, String, String)> {
                    let mut operations = Vec::new();
                    #(operations.extend(#journal_fn_names());)*
                    operations
                }

                /// Get journal operations for a specific message type
                pub fn get_journal_operations_for_message(message_type: &str) -> Vec<(String, String, String)> {
                    match message_type {
                        #(stringify!(#interaction_names) => #journal_fn_names(),)*
                        _ => Vec::new(),
                    }
                }
            }
        } else {
            quote! {
                /// Get all journal operations for the protocol (no operations found)
                pub fn get_protocol_journal_operations() -> Vec<(String, String, String)> {
                    Vec::new()
                }

                /// Get journal operations for a specific message type (no operations defined)
                pub fn get_journal_operations_for_message(_message_type: &str) -> Vec<(String, String, String)> {
                    Vec::new()
                }
            }
        };

        Ok(quote! {
            // Generated journal coupling functions
            #(#coupling_functions)*

            #master_journal

            /// Helper to format journal operation data
            /// Returns (operation_type, description, json_data)
            pub fn format_journal_operation(
                op_type: &str,
                description: &str,
                message_type: &str
            ) -> (String, String, String) {
                let json_data = match op_type {
                    "add_facts" => serde_json::json!({
                        "fact_type": "choreography_fact",
                        "description": description,
                        "message_type": message_type,
                        "timestamp": 0u64
                    }),
                    "refine_caps" => serde_json::json!({
                        "capability_change": "refine",
                        "description": description,
                        "message_type": message_type,
                        "timestamp": 0u64
                    }),
                    "merge" => serde_json::json!({
                        "operation": "merge",
                        "description": description,
                        "message_type": message_type,
                        "timestamp": 0u64
                    }),
                    _ => serde_json::json!({
                        "operation": op_type,
                        "description": description,
                        "message_type": message_type,
                        "timestamp": 0u64
                    })
                };

                (
                    op_type.to_string(),
                    description.to_string(),
                    json_data.to_string()
                )
            }
        })
    }

    fn generate_journal_coupling_for_interaction(
        &self,
        interaction: &Interaction,
    ) -> Result<Option<TokenStream>> {
        let message_name = &interaction.message_name;
        let function_name = format_ident!(
            "{}_journal_operations",
            message_name.to_string().to_lowercase()
        );

        let mut journal_annotations = Vec::new();

        for annotation in &interaction.annotations {
            match annotation {
                AuraAnnotation::JournalFacts(JournalFacts { description }) => {
                    journal_annotations.push(quote! {
                        format_journal_operation("add_facts", &#description, stringify!(#message_name))
                    });
                }
                AuraAnnotation::JournalMerge(_) => {
                    journal_annotations.push(quote! {
                        format_journal_operation("merge", "merge operation", stringify!(#message_name))
                    });
                }
                // Other annotation types don't generate journal operations
                _ => {}
            }
        }

        if journal_annotations.is_empty() {
            return Ok(None);
        }

        Ok(Some(quote! {
            /// Journal operations for #message_name
            /// Returns Vec<(operation_type, description, json_data)>
            pub fn #function_name() -> Vec<(String, String, String)> {
                vec![
                    #(#journal_annotations),*
                ]
            }
        }))
    }

    /// Generate execution functions for each role
    fn generate_execution_functions(&self) -> Result<TokenStream> {
        // For now, generate placeholder execution functions
        // In a complete implementation, this would generate the full protocol logic
        let protocol_name = &self.protocol.name;

        Ok(quote! {
            /// Execute the protocol (placeholder implementation)
            pub fn execute_protocol() -> Result<String, String> {
                Ok(format!("Protocol {} executed successfully", stringify!(#protocol_name)))
            }
        })
    }

    /// Generate helper functions for setting up AuraHandlerAdapter
    fn generate_adapter_helpers(&self) -> Result<TokenStream> {
        let protocol_name = &self.protocol.name;
        let config_type = format_ident!("{}Config", protocol_name);
        let result_type = format_ident!("{}Result", protocol_name);

        Ok(quote! {
            /// Configuration for #protocol_name choreography
            #[derive(Debug, Clone)]
            pub struct #config_type {
                pub timeout_ms: u64,
            }

            impl Default for #config_type {
                fn default() -> Self {
                    Self {
                        timeout_ms: 30000,
                    }
                }
            }

            /// Result of #protocol_name choreography execution
            #[derive(Debug, Clone)]
            pub struct #result_type {
                pub success: bool,
                pub role: String,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::AuraProtocolParser;
    use quote::quote;

    #[test]
    fn test_guard_profile_generation() {
        let input = quote! {
            #[namespace = "test"]
            protocol TestProtocol {
                roles: Alice, Bob;

                Alice[guard_capability = "send_message",
                      flow_cost = 150]
                -> Bob: Message(String);
            }
        };

        let protocol = AuraProtocolParser::parse(input).unwrap();
        let generator = AuraChoreographyGenerator::new(protocol);
        let result = generator.generate();

        assert!(result.is_ok(), "Code generation should succeed");
        let code = result.unwrap();
        assert!(!code.is_empty(), "Generated code should not be empty");
    }
}
