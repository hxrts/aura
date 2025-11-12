//! Parser for Aura choreography syntax with annotations

use proc_macro2::TokenStream;
use std::collections::HashMap;
use syn::{
    braced, parenthesized,
    parse::{Parse, ParseStream},
    parse2, parse_quote, Error, Expr, Ident, LitStr, Result, Token, Type,
};

use crate::annotations::{parse_annotations, AuraAnnotation};

/// Parsed choreography protocol with Aura annotations
#[derive(Debug, Clone)]
pub struct AuraProtocol {
    pub name: Ident,
    pub namespace: Option<String>,
    pub roles: Vec<RoleDefinition>,
    pub interactions: Vec<Interaction>,
}

/// Role definition in the protocol
#[derive(Debug, Clone)]
pub struct RoleDefinition {
    pub name: Ident,
    pub parameter: Option<RoleParameter>,
}

/// Parameter for role definitions
#[derive(Debug, Clone)]
pub enum RoleParameter {
    /// Fixed size: Worker[3]
    Size(usize),
    /// Variable size: Worker[N]
    Variable(Ident),
}

/// Interaction between roles
#[derive(Debug, Clone)]
pub struct Interaction {
    pub from_role: RoleRef,
    pub to_role: RoleRef,
    pub message_name: Ident,
    pub message_type: Type,
    pub annotations: Vec<AuraAnnotation>,
}

/// Reference to a role
#[derive(Debug, Clone)]
pub enum RoleRef {
    /// Static role: Alice
    Static(Ident),
    /// Indexed role: Worker[0], Signer[i]
    Indexed { role: Ident, index: RoleIndex },
    /// Broadcast to all instances: Worker[*]
    Broadcast(Ident),
}

/// Index for parameterized roles
#[derive(Debug, Clone)]
pub enum RoleIndex {
    /// Concrete index: Worker[0]
    Concrete(usize),
    /// Variable index: Worker[i]
    Variable(Ident),
    /// All instances: Worker[*] (reserved for future use)
    #[allow(dead_code)]
    All,
}

/// Internal protocol definition for parsing
struct ProtocolDefinition {
    pub namespace: Option<String>,
    pub name: Ident,
    pub roles: Vec<RoleDefinition>,
    pub interactions: Vec<Interaction>,
}

impl Parse for ProtocolDefinition {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut namespace = None;

        // Parse optional namespace attribute: #[namespace = "name"]
        while input.peek(Token![#]) {
            let _: Token![#] = input.parse()?;
            let content;
            syn::bracketed!(content in input);

            let attr_name: Ident = content.parse()?;
            if attr_name == "namespace" {
                let _: Token![=] = content.parse()?;
                let namespace_str: LitStr = content.parse()?;
                namespace = Some(namespace_str.value());
            }
        }

        // Parse: protocol Name { ... }
        let protocol_token: Ident = input.parse()?;
        if protocol_token != "protocol" {
            return Err(Error::new(protocol_token.span(), "expected 'protocol'"));
        }

        let name: Ident = input.parse()?;

        let content;
        braced!(content in input);

        // Parse roles: roles: Alice, Bob;
        let roles_token: Ident = content.parse()?;
        if roles_token != "roles" {
            return Err(Error::new(roles_token.span(), "expected 'roles'"));
        }
        let _: Token![:] = content.parse()?;

        let mut roles = Vec::new();
        loop {
            let role_name: Ident = content.parse()?;

            // Check for parameterized role: Role[N] or Role[3]
            let parameter = if content.peek(syn::token::Bracket) {
                let bracket_content;
                syn::bracketed!(bracket_content in content);

                // Parse parameter - could be number or identifier
                if let Ok(size) = bracket_content.parse::<syn::LitInt>() {
                    // Fixed size: Worker[3]
                    let size_value = size
                        .base10_parse::<usize>()
                        .map_err(|_| Error::new(size.span(), "Invalid size parameter"))?;
                    Some(RoleParameter::Size(size_value))
                } else if let Ok(param) = bracket_content.parse::<Ident>() {
                    // Variable size: Worker[N]
                    Some(RoleParameter::Variable(param))
                } else {
                    return Err(Error::new(
                        bracket_content.span(),
                        "Expected size or parameter",
                    ));
                }
            } else {
                None
            };

            roles.push(RoleDefinition {
                name: role_name,
                parameter,
            });

            if content.peek(Token![;]) {
                let _: Token![;] = content.parse()?;
                break;
            }
            let _: Token![,] = content.parse()?;
        }

        // Parse interactions
        let mut interactions = Vec::new();
        while !content.is_empty() {
            interactions.push(Self::parse_interaction(&content)?);
        }

        Ok(ProtocolDefinition {
            namespace,
            name,
            roles,
            interactions,
        })
    }
}

impl ProtocolDefinition {
    fn parse_interaction(input: ParseStream) -> Result<Interaction> {
        // Parse from_role (potentially with annotations)
        let (from_role, annotations) = Self::parse_role_with_annotations(input)?;

        let _: Token![->] = input.parse()?;

        // Parse to_role
        let (to_role, _to_annotations) = Self::parse_role_with_annotations(input)?;

        let _: Token![:] = input.parse()?;

        // Parse message name
        let message_name: Ident = input.parse()?;

        // Parse optional message type: Message(String)
        let message_type = if input.peek(syn::token::Paren) {
            let paren_content;
            parenthesized!(paren_content in input);
            paren_content.parse()?
        } else {
            parse_quote!(()) // Unit type as default
        };

        let _: Token![;] = input.parse()?;

        Ok(Interaction {
            from_role,
            to_role,
            message_name,
            message_type,
            annotations,
        })
    }

    fn parse_role_with_annotations(input: ParseStream) -> Result<(RoleRef, Vec<AuraAnnotation>)> {
        let role_name: Ident = input.parse()?;

        // Parse optional annotations FIRST: [guard_capability = "value", flow_cost = 100, ...]
        let mut annotations = Vec::new();
        if input.peek(syn::token::Bracket) {
            let bracket_content;
            syn::bracketed!(bracket_content in input);

            // Try to parse as annotations (simple key = value pairs)
            let mut annotation_map = HashMap::new();
            while !bracket_content.is_empty() {
                let key: Ident = bracket_content.parse()?;
                let _: Token![=] = bracket_content.parse()?;
                let value: Expr = bracket_content.parse()?;

                annotation_map.insert(key.to_string(), value);

                if bracket_content.is_empty() {
                    break;
                }

                // Handle comma separator
                if bracket_content.peek(Token![,]) {
                    let _: Token![,] = bracket_content.parse()?;
                } else {
                    break;
                }
            }

            // Only treat as annotations if we found at least one annotation-like key
            if annotation_map.keys().any(|k| {
                k.starts_with("guard_")
                    || k.starts_with("flow_")
                    || k.starts_with("journal_")
                    || k.starts_with("leakage_")
            }) {
                annotations = parse_annotations(annotation_map)?;
            } else {
                // Not annotations, might be dynamic role syntax
                // Put back what we parsed for role parsing
                // For now, just skip this and assume no annotations
                // TODO: Better lookahead handling
            }
        }

        // Check for indexed role reference after parsing annotations
        let role_ref = if input.peek(syn::token::Bracket) {
            let bracket_content;
            syn::bracketed!(bracket_content in input);

            // Parse index - could be number, identifier, or *
            if bracket_content.peek(Token![*]) {
                let _: Token![*] = bracket_content.parse()?;
                RoleRef::Broadcast(role_name)
            } else if let Ok(index) = bracket_content.parse::<syn::LitInt>() {
                // Concrete index: Worker[0]
                let index_value = index
                    .base10_parse::<usize>()
                    .map_err(|_| Error::new(index.span(), "Invalid index"))?;
                RoleRef::Indexed {
                    role: role_name,
                    index: RoleIndex::Concrete(index_value),
                }
            } else if let Ok(var) = bracket_content.parse::<Ident>() {
                // Variable index: Worker[i]
                RoleRef::Indexed {
                    role: role_name,
                    index: RoleIndex::Variable(var),
                }
            } else {
                return Err(Error::new(
                    bracket_content.span(),
                    "Expected index, variable, or *",
                ));
            }
        } else {
            // Static role
            RoleRef::Static(role_name)
        };

        Ok((role_ref, annotations))
    }
}

/// Parser for Aura choreography protocols
pub struct AuraProtocolParser;

impl AuraProtocolParser {
    /// Parse an Aura choreography protocol from token stream
    pub fn parse(input: TokenStream) -> Result<AuraProtocol> {
        let protocol_def: ProtocolDefinition = parse2(input)?;

        Ok(AuraProtocol {
            name: protocol_def.name,
            namespace: protocol_def.namespace,
            roles: protocol_def.roles,
            interactions: protocol_def.interactions,
        })
    }
}

impl Interaction {
    /// Check if this interaction has Aura annotations
    pub fn has_annotations(&self) -> bool {
        !self.annotations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn test_basic_protocol_parsing() {
        let input = quote! {
            #[namespace = "test"]
            protocol TestProtocol {
                roles: Alice, Bob;

                Alice[guard_capability = "send_message"]
                -> Bob: Message(String);
            }
        };

        let result = AuraProtocolParser::parse(input);
        match result {
            Ok(protocol) => {
                assert_eq!(protocol.name, "TestProtocol");
                assert_eq!(protocol.namespace, Some("test".to_string()));
            }
            Err(e) => {
                eprintln!("Parsing error: {}", e);
                panic!("Basic protocol parsing should succeed");
            }
        }
    }

    #[test]
    #[ignore] // TODO: Implement dynamic role parsing when integrating with rumpsteak-aura
    fn test_dynamic_role_parsing() {
        let input = quote! {
            protocol DynamicProtocol {
                roles: Coordinator, Signers[N];

                Coordinator -> Signers[*]: Request(RequestData);
                Signers[threshold_subset] -> Coordinator: Response(ResponseData);
            }
        };

        let result = AuraProtocolParser::parse(input);
        match result {
            Ok(_) => {
                // Test passed
            }
            Err(e) => {
                eprintln!("Dynamic role parsing error: {}", e);
                panic!("Dynamic role protocol parsing should succeed");
            }
        }
    }
}
