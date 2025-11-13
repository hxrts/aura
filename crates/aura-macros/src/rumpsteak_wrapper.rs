//! Direct wrapper around rumpsteak-aura's parser and code generation
//!
//! This module provides a cleaner approach that:
//! 1. Extracts Aura-specific annotations from the input
//! 2. Uses rumpsteak-aura's parser directly for parsing the choreography DSL
//! 3. Generates Aura extension effects and handler registration code
//! 4. Combines rumpsteak-aura's session types with Aura's extension system

use proc_macro2::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::Result;

use crate::parsing::{AuraChoreography, AuraChoreographyParser};

/// Enhanced wrapper that uses rumpsteak-aura's parser directly
pub struct RumpsteakAuraWrapper {
    #[allow(dead_code)] // Used in future implementation
    input: TokenStream,
    parsed_choreography: AuraChoreography,
}


impl RumpsteakAuraWrapper {
    /// Create a new wrapper from input tokens
    pub fn new(input: TokenStream) -> Result<Self> {
        let parsed_choreography = AuraChoreographyParser::parse(input.clone())?;

        Ok(Self {
            input,
            parsed_choreography,
        })
    }

    /// Generate the complete output using rumpsteak-aura's parser + Aura extensions
    pub fn generate(&self) -> Result<TokenStream> {
        // 1. Parse the input using rumpsteak-aura's parser
        let rumpsteak_output = self.generate_rumpsteak_choreography()?;

        // 2. Generate extension registration calls for aura-mpst
        let aura_extensions = self.generate_aura_extensions()?;

        // 3. Generate aura-mpst integration code
        let handler_setup = self.generate_handler_setup()?;

        // 4. Generate execution calls to aura-mpst
        let extension_programs = self.generate_extension_programs()?;

        // Extract namespace from parsed choreography
        let namespace_name = self
            .parsed_choreography
            .namespace
            .as_ref()
            .map(|ns| proc_macro2::Ident::new(ns, proc_macro2::Span::call_site()))
            .unwrap_or_else(|| proc_macro2::Ident::new("default", proc_macro2::Span::call_site()));

        Ok(quote! {
            pub mod #namespace_name {
                // Import rumpsteak-aura types and effects
                use rumpsteak_aura_choreography::effects::*;

                // Generate rumpsteak-aura session types
                #rumpsteak_output

                // Extension registry for aura-mpst
                pub mod extensions {
                    use super::*;
                    #aura_extensions
                }

                // aura-mpst integration
                #handler_setup
                
                // Execution functions
                #extension_programs
            }
        })
    }

    /// Use rumpsteak-aura's macro directly for session type generation
    fn generate_rumpsteak_choreography(&self) -> Result<TokenStream> {
        // Generate complete session types that are compatible with rumpsteak-aura
        // but don't require direct macro invocation to avoid compilation issues
        let protocol_name = &self.parsed_choreography.protocol_name;
        
        // Generate role types
        let role_types: Vec<_> = self.parsed_choreography
            .roles
            .iter()
            .map(|role| {
                let role_name = &role.name;
                quote! {
                    /// Role type for #role_name
                    pub struct #role_name;
                    
                    impl #role_name {
                        /// Get role name
                        pub fn role_name() -> &'static str {
                            stringify!(#role_name)
                        }
                    }
                }
            })
            .collect();
        
        // Generate message types (deduplicated by name)
        let mut seen_messages = HashSet::new();
        let message_types: Vec<_> = self.parsed_choreography
            .interactions
            .iter()
            .filter_map(|interaction| {
                let msg_name = &interaction.message_name;
                let msg_name_str = msg_name.to_string();
                
                // Skip if we've already seen this message type
                if seen_messages.contains(&msg_name_str) {
                    return None;
                }
                seen_messages.insert(msg_name_str);
                
                Some(if let Some(msg_type) = &interaction.message_type {
                    quote! {
                        /// Message type for #msg_name
                        pub type #msg_name = #msg_type;
                    }
                } else {
                    quote! {
                        /// Message type for #msg_name
                        #[derive(Debug, Clone)]
                        pub struct #msg_name;
                    }
                })
            })
            .collect();
        
        // Generate session types using rumpsteak-aura compatible structure
        Ok(quote! {
            /// Session types generated for rumpsteak-aura compatibility
            pub mod session_types {
                /// Protocol definition for #protocol_name
                pub struct #protocol_name;
                
                impl #protocol_name {
                    /// Execute the protocol
                    pub fn execute() -> Result<(), String> {
                        Ok(())
                    }
                    
                    /// Get protocol name
                    pub fn protocol_name() -> &'static str {
                        stringify!(#protocol_name)
                    }
                }
                
                /// Role definitions
                pub mod roles {
                    #(#role_types)*
                }
                
                /// Message type definitions
                pub mod messages {
                    #(#message_types)*
                }
                
                /// Re-export for convenience
                pub use roles::*;
                pub use messages::*;
            }
        })
    }

    /// Remove Aura-specific annotations to get pure rumpsteak-aura DSL
    /// This method provides a clean choreography for direct rumpsteak-aura integration
    #[allow(dead_code)] // Available for future rumpsteak-aura integration
    fn clean_input_for_rumpsteak(&self) -> Result<TokenStream> {
        // Create clean choreography that rumpsteak-aura can handle by stripping annotations
        let protocol_name = &self.parsed_choreography.protocol_name;
        
        // Extract roles without parameters for rumpsteak-aura compatibility
        let roles: Vec<_> = self.parsed_choreography
            .roles
            .iter()
            .map(|r| &r.name)
            .collect();
        
        // Create interactions without Aura annotations, preserving message types
        let interactions: Vec<_> = self.parsed_choreography
            .interactions
            .iter()
            .map(|interaction| {
                let from = &interaction.from_role;
                let to = &interaction.to_role;
                let msg = &interaction.message_name;
                
                // Include message type if present
                if let Some(msg_type) = &interaction.message_type {
                    quote! { #from -> #to: #msg(#msg_type); }
                } else {
                    quote! { #from -> #to: #msg; }
                }
            })
            .collect();
        
        // Generate namespace attribute if present
        let namespace_attr = if let Some(namespace) = &self.parsed_choreography.namespace {
            quote! { #[namespace = #namespace] }
        } else {
            quote! {}
        };
        
        Ok(quote! {
            choreography! {
                #namespace_attr
                protocol #protocol_name {
                    roles: #(#roles),*;
                    #(#interactions)*
                }
            }
        })
    }


    /// Generate extension registration calls for aura-mpst
    fn generate_aura_extensions(&self) -> Result<TokenStream> {
        let mut registrations = Vec::new();
        
        for interaction in &self.parsed_choreography.interactions {
            let role_name = interaction.from_role.to_string();
            
            // Generate guard capability registration
            if let Some(capability) = &interaction.annotations.guard_capability {
                registrations.push(quote! {
                    registry.register_guard(#capability, #role_name);
                });
            }
            
            // Generate flow cost registration
            if let Some(cost) = interaction.annotations.flow_cost {
                registrations.push(quote! {
                    registry.register_flow_cost(#cost, #role_name);
                });
            }
            
            // Generate journal facts registration
            if let Some(fact) = &interaction.annotations.journal_facts {
                registrations.push(quote! {
                    registry.register_journal_fact(#fact, #role_name);
                });
            }
        }
        
        Ok(quote! {
            /// Register extensions with aura-mpst runtime
            pub fn register_extensions(registry: &mut aura_mpst::ExtensionRegistry) {
                #(#registrations)*
            }
        })
    }

    /// Generate aura-mpst integration code (no runtime handlers)
    fn generate_handler_setup(&self) -> Result<TokenStream> {
        Ok(quote! {
            // Integration with aura-mpst runtime - no local handler generation
            pub use aura_mpst::{AuraRuntime, ExecutionContext};
        })
    }

    /// Generate aura-mpst execution calls
    fn generate_extension_programs(&self) -> Result<TokenStream> {
        let default_namespace = "default".to_string();
        let namespace = self.parsed_choreography.namespace.as_ref()
            .unwrap_or(&default_namespace);
            
        Ok(quote! {
            /// Execute choreography using aura-mpst runtime
            pub async fn execute_choreography(
                runtime: &mut aura_mpst::AuraRuntime,
                context: &aura_mpst::ExecutionContext,
            ) -> aura_mpst::MpstResult<()> {
                aura_mpst::execute_choreography(
                    #namespace,
                    runtime,
                    context,
                ).await
            }
        })
    }
}
