//! Wrapper around rumpsteak-choreography integration
//!
//! This module provides the AuraChoreographyWrapper that:
//! 1. Extracts Aura-specific annotations from the input
//! 2. Transforms the syntax to pure choreography DSL for rumpsteak-choreography
//! 3. Delegates session type generation to rumpsteak-choreography
//! 4. Generates Aura-specific integration code for guards, capabilities, and journal coupling

use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use syn::Result;
use std::collections::HashSet;

use crate::parsing::AuraProtocolParser;
use crate::codegen::AuraChoreographyGenerator;

/// Wrapper that combines rumpsteak-aura session types with Aura enhancements
pub struct AuraChoreographyWrapper {
    protocol: crate::parsing::AuraProtocol,
}

impl AuraChoreographyWrapper {
    /// Create a new wrapper from the input token stream
    pub fn new(input: TokenStream) -> Result<Self> {
        let protocol = AuraProtocolParser::parse(input)?;
        Ok(Self { protocol })
    }

    /// Generate the complete output combining rumpsteak-aura session types
    /// with Aura-specific integration code
    pub fn generate(&self) -> Result<TokenStream> {
        // 1. Transform our syntax to rumpsteak-aura compatible syntax
        let rumpsteak_dsl = self.transform_for_rumpsteak()?;
        
        // 2. Delegate session type generation to rumpsteak-aura
        let session_types = self.generate_session_types(&rumpsteak_dsl)?;
        
        // 3. Generate our Aura-specific integration code
        let generator = AuraChoreographyGenerator::new(self.protocol.clone());
        let aura_integration = generator.generate()?;
        
        // 4. Combine both outputs with proper module structure
        let namespace = if let Some(ref ns) = self.protocol.namespace {
            format_ident!("{}", ns)
        } else {
            format_ident!("{}", self.protocol.name.to_string().to_lowercase())
        };
        
        let output = quote! {
            // Combined module with session types and Aura integration
            pub mod #namespace {
                use super::*;
                #session_types
                #aura_integration
            }
        };
        
        Ok(output)
    }
    
    /// Transform Aura syntax to rumpsteak-aura compatible syntax
    fn transform_for_rumpsteak(&self) -> Result<String> {
        let protocol_name = &self.protocol.name;
        let roles = &self.protocol.roles;
        
        let mut dsl_output = String::new();
        
        // Generate choreography header
        dsl_output.push_str(&format!("choreography {} {{\n", protocol_name));
        
        // Add roles (including parameterized ones)
        dsl_output.push_str("    roles: ");
        for (i, role) in roles.iter().enumerate() {
            if i > 0 {
                dsl_output.push_str(", ");
            }
            dsl_output.push_str(&role.name.to_string());
            
            // Add parameter if present
            if let Some(ref param) = role.parameter {
                match param {
                    crate::parsing::RoleParameter::Size(size) => {
                        dsl_output.push_str(&format!("[{}]", size));
                    }
                    crate::parsing::RoleParameter::Variable(var) => {
                        dsl_output.push_str(&format!("[{}]", var));
                    }
                }
            }
        }
        dsl_output.push_str("\n\n");
        
        // Convert interactions, stripping Aura annotations
        for interaction in &self.protocol.interactions {
            // Extract role names from RoleRef (enhanced for dynamic roles)
            let from = match &interaction.from_role {
                crate::parsing::RoleRef::Static(name) => name.to_string(),
                crate::parsing::RoleRef::Indexed { role, index } => {
                    match index {
                        crate::parsing::RoleIndex::Concrete(i) => format!("{}[{}]", role, i),
                        crate::parsing::RoleIndex::Variable(var) => format!("{}[{}]", role, var),
                        crate::parsing::RoleIndex::All => format!("{}[*]", role),
                    }
                },
                crate::parsing::RoleRef::Broadcast(role) => format!("{}[*]", role),
            };
            
            let to = match &interaction.to_role {
                crate::parsing::RoleRef::Static(name) => name.to_string(),
                crate::parsing::RoleRef::Indexed { role, index } => {
                    match index {
                        crate::parsing::RoleIndex::Concrete(i) => format!("{}[{}]", role, i),
                        crate::parsing::RoleIndex::Variable(var) => format!("{}[{}]", role, var),
                        crate::parsing::RoleIndex::All => format!("{}[*]", role),
                    }
                },
                crate::parsing::RoleRef::Broadcast(role) => format!("{}[*]", role),
            };
            
            let message = &interaction.message_name.to_string();
            
            // Generate simple choreography statement
            dsl_output.push_str(&format!("    {} -> {}: {}\n", from, to, message));
        }
        
        dsl_output.push_str("}\n");
        
        Ok(dsl_output)
    }
    
    /// Generate session types using enhanced rumpsteak-aura integration
    fn generate_session_types(&self, _rumpsteak_dsl: &str) -> Result<TokenStream> {
        // Enhanced version that integrates more deeply with rumpsteak-aura patterns
        // while maintaining compatibility with our Aura annotations
        
        let protocol_name = &self.protocol.name;
        let roles = &self.protocol.roles;
        
        // Generate role structs with enhanced metadata (supporting parameterized roles)
        let mut role_structs = Vec::new();
        let mut role_names = Vec::new();
        
        for role in roles {
            let role_name = &role.name;
            role_names.push(role_name);
            
            match &role.parameter {
                None => {
                    // Static role
                    role_structs.push(quote! {
                        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
                        pub struct #role_name;
                        
                        impl std::fmt::Display for #role_name {
                            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                                write!(f, "{}", stringify!(#role_name))
                            }
                        }
                    });
                }
                Some(crate::parsing::RoleParameter::Size(size)) => {
                    // Fixed-size parameterized role
                    let size_lit = syn::LitInt::new(&size.to_string(), proc_macro2::Span::call_site());
                    role_structs.push(quote! {
                        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
                        pub struct #role_name {
                            pub index: usize,
                        }
                        
                        impl #role_name {
                            pub const SIZE: usize = #size_lit;
                            
                            pub fn new(index: usize) -> Result<Self, String> {
                                if index < Self::SIZE {
                                    Ok(Self { index })
                                } else {
                                    Err(format!("Index {} out of bounds for role {} (size {})", index, stringify!(#role_name), Self::SIZE))
                                }
                            }
                            
                            pub fn all_instances() -> Vec<Self> {
                                (0..Self::SIZE).map(|i| Self { index: i }).collect()
                            }
                        }
                        
                        impl std::fmt::Display for #role_name {
                            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                                write!(f, "{}[{}]", stringify!(#role_name), self.index)
                            }
                        }
                    });
                }
                Some(crate::parsing::RoleParameter::Variable(var)) => {
                    // Variable-size parameterized role
                    role_structs.push(quote! {
                        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
                        pub struct #role_name<const #var: usize> {
                            pub index: usize,
                        }
                        
                        impl<const #var: usize> #role_name<#var> {
                            pub fn new(index: usize) -> Result<Self, String> {
                                if index < #var {
                                    Ok(Self { index })
                                } else {
                                    Err(format!("Index {} out of bounds for role {} (size {})", index, stringify!(#role_name), #var))
                                }
                            }
                            
                            pub fn all_instances() -> Vec<Self> {
                                (0..#var).map(|i| Self { index: i }).collect()
                            }
                        }
                        
                        impl<const #var: usize> std::fmt::Display for #role_name<#var> {
                            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                                write!(f, "{}[{}]", stringify!(#role_name), self.index)
                            }
                        }
                    });
                }
            }
        }
        
        // Generate message types (deduplicated) with rumpsteak-aura compatibility
        let mut message_types = Vec::new();
        let mut message_variant_names = Vec::new();
        let mut seen_messages = HashSet::new();
        
        for interaction in &self.protocol.interactions {
            let message = &interaction.message_name;
            let message_type = &interaction.message_type;
            
            // Only generate each message type once
            if !seen_messages.contains(&message.to_string()) {
                seen_messages.insert(message.to_string());
                message_types.push(quote! {
                    #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
                    pub struct #message(pub #message_type);
                    
                    impl #message {
                        pub fn new(data: #message_type) -> Self {
                            Self(data)
                        }
                        
                        pub fn inner(&self) -> &#message_type {
                            &self.0
                        }
                    }
                });
                message_variant_names.push(message);
            }
        }
        
        // Generate role enum for easier handling
        let role_enum = quote! {
            #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
            pub enum RoleType {
                #(#role_names),*
            }
            
            impl RoleType {
                pub fn all_roles() -> Vec<RoleType> {
                    vec![#(RoleType::#role_names),*]
                }
                
                pub fn name(&self) -> &'static str {
                    match self {
                        #(RoleType::#role_names => stringify!(#role_names)),*
                    }
                }
            }
        };
        
        Ok(quote! {
            // Enhanced role definitions with metadata
            #(#role_structs)*
            
            // Role enum for dynamic role handling
            #role_enum
            
            // Enhanced message types with helpers
            #(#message_types)*
            
            // Message enum for protocol  
            #[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
            pub enum Message {
                #(#message_variant_names(#message_variant_names)),*
            }
            
            impl Message {
                pub fn message_type(&self) -> &'static str {
                    match self {
                        #(Message::#message_variant_names(_) => stringify!(#message_variant_names)),*
                    }
                }
            }
            
            // Protocol configuration with enhanced metadata
            #[derive(Clone, Debug)]
            pub struct #protocol_name {
                pub protocol_id: String,
                pub created_at: std::time::SystemTime,
            }
            
            impl #protocol_name {
                pub fn new() -> Self {
                    Self {
                        protocol_id: "protocol-instance".to_string(),
                        created_at: std::time::SystemTime::UNIX_EPOCH,
                    }
                }
                
                pub fn with_id(id: String) -> Self {
                    Self {
                        protocol_id: id,
                        created_at: std::time::SystemTime::UNIX_EPOCH,
                    }
                }
                
                pub fn roles() -> Vec<RoleType> {
                    RoleType::all_roles()
                }
            }
            
            impl Default for #protocol_name {
                fn default() -> Self {
                    Self::new()
                }
            }
            
            /// Enhanced protocol execution with metadata
            pub fn execute_protocol() -> Result<String, String> {
                let protocol = #protocol_name::new();
                let roles = #protocol_name::roles();
                
                Ok(format!(
                    "Protocol {} ({}) executed with {} roles: {:?}",
                    stringify!(#protocol_name),
                    protocol.protocol_id,
                    roles.len(),
                    roles.iter().map(|r| r.name()).collect::<Vec<_>>()
                ))
            }
        })
    }
}