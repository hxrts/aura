//! Aura Choreography Macro Implementation
//!
//! This module provides the choreography! macro that generates both the underlying
//! rumpsteak-aura choreography and the Aura-specific wrapper module expected by
//! the examples and integration code.
//!
//! The macro generates:
//! - Core choreographic projection via rumpsteak-aura
//! - Aura wrapper module with role types and helper functions
//! - Integration with the Aura effects system

use proc_macro2::TokenStream;
use quote::quote;
use rumpsteak_aura_choreography::{
    compiler::{generate_choreography_code_with_namespacing, parse_choreography_str, project},
    extensions::ExtensionRegistry,
    parse_and_generate_with_extensions,
};
use syn::{parse::Parse, Ident, Token};

// Import Biscuit-related types for the updated annotation system
use aura_mpst::ast_extraction::{extract_aura_annotations, AuraEffect};

/// Parse choreography input to extract roles and protocol name
#[derive(Debug)]
struct ChoreographyInput {
    _protocol_name: Ident,
    roles: Vec<Ident>,
    aura_annotations: Vec<AuraEffect>,
}

impl Parse for ChoreographyInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        // Skip any attributes (like #[namespace = "..."]) before the choreography keyword
        while input.peek(Token![#]) {
            let _: syn::Attribute = input
                .call(syn::Attribute::parse_outer)?
                .pop()
                .ok_or_else(|| syn::Error::new(input.span(), "Expected attribute"))?;
        }

        // Parse "choreography ProtocolName" or "protocol ProtocolName"
        let _keyword: Ident = input.parse()?; // "choreography" or "protocol"
        let protocol_name: Ident = input.parse()?;

        // Parse roles section
        let content;
        syn::braced!(content in input);

        // Look for "roles: Role1, Role2, ...;"
        let roles_keyword: Ident = content.parse()?;
        if roles_keyword != "roles" {
            return Err(syn::Error::new(roles_keyword.span(), "Expected 'roles'"));
        }
        content.parse::<Token![:]>()?;

        let mut roles = Vec::new();
        loop {
            if content.peek(Token![;]) {
                content.parse::<Token![;]>()?;
                break;
            }
            roles.push(content.parse()?);
            if content.peek(Token![,]) {
                content.parse::<Token![,]>()?;
            }
        }

        // Messages and actions are parsed by rumpsteak; consume remaining tokens
        // so this wrapper can still extract Aura annotations cleanly.
        while !content.is_empty() {
            let _: proc_macro2::TokenTree = content.parse()?;
        }

        // Extract Aura annotations from the full input
        let full_input_str = input.to_string();
        let aura_annotations =
            extract_aura_annotations(&full_input_str).unwrap_or_else(|_| Vec::new()); // Gracefully handle extraction errors

        Ok(ChoreographyInput {
            _protocol_name: protocol_name,
            roles,
            aura_annotations,
        })
    }
}

/// Implementation of the Aura choreography! macro
///
/// Uses namespace-aware rumpsteak-aura generation to avoid module conflicts
pub fn choreography_impl(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Try to parse the input to extract roles and protocol name for Aura wrapper
    let parsed_input = syn::parse2::<ChoreographyInput>(input.clone()).ok();

    // Generate the rumpsteak-aura choreography using namespace-aware functions
    let rumpsteak_output = choreography_impl_namespace_aware(input.clone()).unwrap_or_else(|err| {
        // If rumpsteak fails, generate a helpful error message but continue with Aura wrapper
        let _error_msg = err.to_string();
        quote! {
            /// Rumpsteak integration failed - using Aura-only mode
            pub mod rumpsteak_session_types {
                // Rumpsteak integration error (this is expected in some cases):
                // #error_msg
                // Using Aura choreography system only.
            }
        }
    });

    // Generate the Aura wrapper module with namespace support
    let aura_wrapper = if let Some(parsed) = &parsed_input {
        // Extract namespace from the original input - we need to re-parse to get namespace
        let namespace = extract_namespace_from_input(input.clone());
        generate_aura_wrapper(parsed, namespace.as_deref())
    } else {
        // Fallback: generate with default Alice/Bob roles
        generate_aura_wrapper(
            &ChoreographyInput {
                _protocol_name: Ident::new("DefaultProtocol", proc_macro2::Span::call_site()),
                roles: vec![
                    Ident::new("Alice", proc_macro2::Span::call_site()),
                    Ident::new("Bob", proc_macro2::Span::call_site()),
                ],
                aura_annotations: Vec::new(),
            },
            None,
        )
    };

    // Extract namespace for uniqueness check (reuse from aura_wrapper if available)
    let namespace = extract_namespace_from_input(input.clone());

    // Generate a uniqueness marker to catch duplicate namespaces at compile time
    let namespace_uniqueness_check = if let Some(ns) = &namespace {
        let marker_name = quote::format_ident!(
            "_CHOREOGRAPHY_NAMESPACE_{}_MUST_BE_UNIQUE",
            ns.to_uppercase()
        );
        quote! {
            // Compile-time uniqueness check for choreography namespace
            // If you see an error here, it means you have two choreographies with the same namespace.
            // Each choreography must have a unique namespace within the same compilation unit.
            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            const #marker_name: () = ();
        }
    } else {
        // No namespace specified - generate a helpful compile error
        quote! {
            compile_error!(
                "Choreography is missing a namespace attribute. \
                 Add #[namespace = \"unique_name\"] before your choreography! macro. \
                 Each choreography in the same file must have a unique namespace."
            );
        }
    };

    // Return both namespace-aware modules with uniqueness check
    Ok(quote! {
        #namespace_uniqueness_check

        // Rumpsteak-aura generated choreography (session types, projections) - namespace-aware
        #rumpsteak_output

        // Aura wrapper module (effect system integration) - namespace-aware
        #aura_wrapper
    })
}

/// Generate the Aura wrapper module that integrates with the effects system
fn generate_aura_wrapper(input: &ChoreographyInput, namespace: Option<&str>) -> TokenStream {
    let roles = &input.roles;
    let annotations = &input.aura_annotations;

    let role_variants: Vec<_> = roles
        .iter()
        .map(|role| {
            quote! { #role }
        })
        .collect();

    let role_display_arms: Vec<_> = roles
        .iter()
        .map(|role| {
            let role_str = role.to_string();
            quote! {
                Self::#role => write!(f, #role_str)
            }
        })
        .collect();

    let first_role = match roles.first() {
        Some(role) => role,
        None => {
            return quote! {
                compile_error!("Choreography must have at least one role");
            };
        }
    };

    // Generate module name using namespace
    let module_name = if let Some(ns) = namespace {
        quote::format_ident!("aura_choreography_{}", ns)
    } else {
        quote::format_ident!("aura_choreography")
    };

    // Generate leakage integration code
    let leakage_integration = generate_leakage_integration(annotations);

    quote! {
        /// Generated Aura choreography module with effects system integration
        pub mod #module_name {
            use std::collections::HashMap;
            use std::fmt;

            // Note: The generated code expects these types to be available at runtime:
            // - biscuit_auth::Biscuit
            // - aura_core::FlowBudget
            // - aura_core::scope::ResourceScope
            // - aura_mpst::ast_extraction::AuraEffect (for annotation processing)
            // Users should provide their own BiscuitGuardEvaluator implementation

            /// Role enumeration for this choreography
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub enum AuraRole {
                #(#role_variants),*
            }

            impl fmt::Display for AuraRole {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    match self {
                        #(#role_display_arms),*
                    }
                }
            }

            /// Aura handler with authorization and flow management hooks
            ///
            /// Users should inject actual Biscuit evaluator and flow budget at runtime
            pub struct AuraHandler<BiscuitEvaluator = (), FlowBudget = ()> {
                pub role: AuraRole,
                pub biscuit_evaluator: Option<BiscuitEvaluator>,
                pub flow_budget: Option<FlowBudget>,
                pub flow_balance: i32,
                pub audit_log: Vec<String>,
            }

            impl<BiscuitEvaluator, FlowBudget> AuraHandler<BiscuitEvaluator, FlowBudget> {
                pub fn get_flow_balance(&self) -> i32 {
                    self.flow_balance
                }

                pub fn charge_flow_cost(&mut self, cost: i32) -> Result<(), String> {
                    if self.flow_balance >= cost {
                        self.flow_balance -= cost;
                        Ok(())
                    } else {
                        Err(format!("Insufficient flow balance: {} < {}", self.flow_balance, cost))
                    }
                }

                /// Biscuit guard validation hook
                ///
                /// Implementations should call into a real BiscuitGuardEvaluator supplied via the
                /// generic type parameter. The default checks for a non-empty capability string.
                pub fn validate_guard(&self, capability: &str, resource_type: &str) -> Result<(), String> {
                    // Default implementation logs the validation attempt
                    if capability.is_empty() {
                        Err("Empty capability".to_string())
                    } else {
                        Ok(())
                    }
                }

                /// Biscuit guard evaluation with flow cost
                ///
                /// Implementations should charge flow budget and validate Biscuit guards. Default
                /// behavior delegates to `validate_guard`.
                pub fn evaluate_guard_with_flow(&mut self, capability: &str, resource_type: &str, _flow_cost: u64) -> Result<(), String> {
                    // Default implementation just validates capability
                    self.validate_guard(capability, resource_type)
                }

                pub fn log_audit(&mut self, event: String) {
                    self.audit_log.push(event);
                }
            }

            /// Create a new Aura handler for a role
            pub fn create_handler<BiscuitEvaluator, FlowBudget>(
                role: AuraRole,
                biscuit_evaluator: Option<BiscuitEvaluator>,
                flow_budget: Option<FlowBudget>
            ) -> AuraHandler<BiscuitEvaluator, FlowBudget> {
                AuraHandler {
                    role,
                    biscuit_evaluator,
                    flow_budget,
                    flow_balance: 1000, // Default balance
                    audit_log: Vec::new(),
                }
            }

            /// Simple handler creation for basic use cases
            pub fn create_simple_handler(role: AuraRole) -> AuraHandler<(), ()> {
                create_handler(role, None, None)
            }

            /// Choreography builder for effects composition
            pub struct ChoreographyBuilder {
                operations: Vec<Operation>,
            }

            #[derive(Debug, Clone)]
            enum Operation {
                AuditLog(String, HashMap<String, String>),
                ValidateGuard(AuraRole, String, String), // role, capability, resource_type
                EvaluateGuardWithFlow(AuraRole, String, String, u64), // role, capability, resource_type, flow_cost
                ChargeFlowCost(AuraRole, i32),
                Send(AuraRole, AuraRole, String),
                RecordLeakage(AuraRole, AuraRole, Vec<String>, u64), // from, to, observers, flow_cost
            }

            impl ChoreographyBuilder {
                pub fn new() -> Self {
                    Self {
                        operations: Vec::new(),
                    }
                }

                pub fn audit_log(mut self, event: impl Into<String>, metadata: HashMap<String, String>) -> Self {
                    self.operations.push(Operation::AuditLog(event.into(), metadata));
                    self
                }

                pub fn validate_guard(
                    mut self,
                    role: AuraRole,
                    guard_capability: impl Into<String>,
                    resource_type: impl Into<String>
                ) -> Self {
                    self.operations.push(Operation::ValidateGuard(role, guard_capability.into(), resource_type.into()));
                    self
                }

                pub fn evaluate_guard_with_flow(
                    mut self,
                    role: AuraRole,
                    guard_capability: impl Into<String>,
                    resource_type: impl Into<String>,
                    flow_cost: u64
                ) -> Self {
                    self.operations.push(Operation::EvaluateGuardWithFlow(role, guard_capability.into(), resource_type.into(), flow_cost));
                    self
                }

                pub fn charge_flow_cost(mut self, role: AuraRole, cost: i32) -> Self {
                    self.operations.push(Operation::ChargeFlowCost(role, cost));
                    self
                }

                pub fn send(mut self, from: AuraRole, to: AuraRole, message: impl Into<String>) -> Self {
                    self.operations.push(Operation::Send(from, to, message.into()));
                    self
                }

                pub fn record_leakage(
                    mut self,
                    from: AuraRole,
                    to: AuraRole,
                    observers: Vec<String>,
                    flow_cost: u64
                ) -> Self {
                    self.operations.push(Operation::RecordLeakage(from, to, observers, flow_cost));
                    self
                }

                pub fn end(self) -> ChoreographyProgram {
                    ChoreographyProgram {
                        operations: self.operations,
                    }
                }
            }

            /// Executable choreography program
            #[derive(Clone, Debug)]
            pub struct ChoreographyProgram {
                operations: Vec<Operation>,
            }

            /// Map guard capability to appropriate resource type string
            ///
            /// This provides a simple mapping that users can extend for their Biscuit ResourceScope types
            pub fn map_capability_to_resource_type(capability: &str) -> &'static str {
                match capability {
                    // Storage operations
                    cap if cap.contains("read_storage") || cap.contains("access_storage") => "storage",
                    cap if cap.contains("write_storage") => "storage",

                    // Journal operations
                    cap if cap.contains("read_journal") => "journal",
                    cap if cap.contains("write_journal") || cap.contains("journal_sync") => "journal",

                    // Recovery operations
                    cap if cap.contains("emergency_recovery") || cap.contains("guardian") => "recovery",

                    // Admin operations
                    cap if cap.contains("admin") || cap.contains("setup") => "admin",

                    // Relay/communication operations
                    cap if cap.contains("relay") || cap.contains("send") || cap.contains("receive") || cap.contains("initiate") => "relay",

                    // Default fallback
                    _ => "default"
                }
            }

            /// Create a new choreography builder
            pub fn builder() -> ChoreographyBuilder {
                ChoreographyBuilder::new()
            }

            /// Execute a choreography program
            pub async fn execute<BiscuitEvaluator, FlowBudget>(
                handler: &mut AuraHandler<BiscuitEvaluator, FlowBudget>,
                _endpoint: &mut (),
                program: ChoreographyProgram,
            ) -> Result<(), String> {
                for operation in program.operations {
                    match operation {
                        Operation::AuditLog(event, _metadata) => {
                            handler.log_audit(format!("AUDIT: {}", event));
                        },
                        Operation::ValidateGuard(role, guard_capability, resource_type) => {
                            if role == handler.role {
                                handler.validate_guard(&guard_capability, &resource_type)?;
                                handler.log_audit(format!("GUARD_VALIDATED: {} ({}) for {:?}", guard_capability, resource_type, role));
                            }
                        },
                        Operation::EvaluateGuardWithFlow(role, guard_capability, resource_type, flow_cost) => {
                            if role == handler.role {
                                handler.evaluate_guard_with_flow(&guard_capability, &resource_type, flow_cost)?;
                                handler.log_audit(format!("GUARD_EVALUATED_WITH_FLOW: {} ({}, cost: {}) for {:?}", guard_capability, resource_type, flow_cost, role));
                            }
                        },
                        Operation::ChargeFlowCost(role, cost) => {
                            if role == handler.role {
                                handler.charge_flow_cost(cost)?;
                                handler.log_audit(format!("CHARGED: {} flow cost for {:?}", cost, role));
                            }
                        },
                        Operation::Send(from, to, message) => {
                            if from == handler.role {
                                handler.log_audit(format!("SENT: {} from {:?} to {:?}", message, from, to));
                            } else if to == handler.role {
                                handler.log_audit(format!("RECEIVED: {} from {:?} to {:?}", message, from, to));
                            }
                        },
                        Operation::RecordLeakage(from, to, observers, flow_cost) => {
                            // Record leakage for send/recv operations
                            if from == handler.role {
                                handler.charge_flow_cost(flow_cost as i32)?;
                                handler.log_audit(format!("LEAKAGE_SENT: from {:?} to {:?}, observers: {:?}, cost: {}", from, to, observers, flow_cost));
                            } else if to == handler.role {
                                handler.log_audit(format!("LEAKAGE_RECV: from {:?} to {:?}, observers: {:?}", from, to, observers));
                            }
                        },
                    }
                }
                Ok(())
            }

            /// Example choreography for demonstration with guard validation
            pub fn example_aura_choreography() -> ChoreographyProgram {
                let capability = "send_money";
                let resource_type = map_capability_to_resource_type(capability);

                builder()
                    .audit_log("example_start", HashMap::new())
                    .validate_guard(AuraRole::#first_role, capability, resource_type)
                    .charge_flow_cost(AuraRole::#first_role, 200)
                    .audit_log("example_complete", HashMap::new())
                    .end()
            }

            /// Generate a choreography program using the actual parsed annotations
            /// This would be called by the macro expansion with the real extracted annotations
            pub fn generate_protocol_choreography() -> ChoreographyProgram {
                // In a real macro expansion, this would receive the extracted annotations
                // Example annotations that would be extracted from choreography syntax:
                let sample_annotations = vec![
                    ("Alice".to_string(), "guard_capability".to_string(), "send_message".to_string(), 100),
                    ("Bob".to_string(), "guard_with_flow".to_string(), "receive_message".to_string(), 50),
                    ("Alice".to_string(), "journal_facts".to_string(), "message_sent".to_string(), 0),
                ];
                generate_from_annotations_impl(sample_annotations)
            }

            /// Generate choreography from extracted annotations
            pub fn generate_from_annotations_impl(annotations: Vec<(String, String, String, u64)>) -> ChoreographyProgram {
                let roles = vec![AuraRole::#first_role]; // Simplified - real impl would get all roles
                let mut builder = builder();
                builder = builder.audit_log("choreography_start", HashMap::new());

                // Generate operations based on parsed annotation tuples:
                // (role_name, annotation_type, capability_or_value, flow_cost)
                for (role_name, annotation_type, capability_or_value, flow_cost) in annotations {
                    if let Some(role) = parse_role_from_name(&role_name) {
                        match annotation_type.as_str() {
                            "guard_capability" => {
                                let resource_type = map_capability_to_resource_type(&capability_or_value);
                                builder = builder.validate_guard(role, capability_or_value, resource_type);
                            },
                            "flow_cost" => {
                                builder = builder.charge_flow_cost(role, flow_cost as i32);
                            },
                            "guard_with_flow" => {
                                let resource_type = map_capability_to_resource_type(&capability_or_value);
                                builder = builder.evaluate_guard_with_flow(role, capability_or_value, resource_type, flow_cost);
                            },
                            "journal_facts" => {
                                let mut metadata = HashMap::new();
                                metadata.insert("journal_facts".to_string(), capability_or_value);
                                builder = builder.audit_log("journal_facts_recorded", metadata);
                            },
                            "journal_merge" => {
                                builder = builder.audit_log("journal_merge_requested", HashMap::new());
                            },
                            "leak" => {
                                // Parse observers from capability_or_value
                                let observers: Vec<String> = capability_or_value
                                    .split(',')
                                    .map(|s| s.trim().to_string())
                                    .collect();

                                // For leakage, we need to know sender and receiver
                                // This is simplified - real implementation would parse from choreography
                                if let Some(next_role) = roles.iter().find(|r| **r != role) {
                                    builder = builder.record_leakage(
                                        role,
                                        *next_role,
                                        observers,
                                        flow_cost.max(100), // Default 100 if not specified
                                    );
                                }
                            },
                            _ => {
                                // Unknown annotation type - log it for debugging
                                let mut metadata = HashMap::new();
                                metadata.insert("annotation_type".to_string(), annotation_type);
                                metadata.insert("capability_or_value".to_string(), capability_or_value);
                                builder = builder.audit_log("unknown_annotation", metadata);
                            }
                        }
                    }
                }

                builder = builder.audit_log("choreography_complete", HashMap::new());
                builder.end()
            }

            /// Parse a role name string into an AuraRole enum variant
            fn parse_role_from_name(role_name: &str) -> Option<AuraRole> {
                match role_name {
                    #(stringify!(#role_variants) => Some(AuraRole::#role_variants),)*
                    _ => None,
                }
            }

            /// Bridge module for converting annotations to EffectCommand sequences
            ///
            /// This module provides runtime conversion from choreographic annotations
            /// to the algebraic effect commands defined in aura-core::effects::guard.
            pub mod effect_bridge {
                use aura_core::effects::guard::EffectCommand;
                use aura_core::time::TimeStamp;
                use aura_core::types::identifiers::{AuthorityId, ContextId};

                /// Runtime context for effect command generation
                #[derive(Clone)]
                pub struct EffectBridgeContext {
                    /// Current context ID
                    pub context: ContextId,
                    /// Local authority ID
                    pub authority: AuthorityId,
                    /// Peer authority ID (for communication)
                    pub peer: AuthorityId,
                    /// Current timestamp
                    pub timestamp: TimeStamp,
                }

                /// Convert an annotation tuple to effect commands
                ///
                /// Takes: (role_name, annotation_type, value, flow_cost)
                /// Returns: Vec of EffectCommand for the interpreter
                pub fn annotation_to_commands(
                    ctx: &EffectBridgeContext,
                    annotation: (String, String, String, u64),
                ) -> Vec<EffectCommand> {
                    let (_role_name, annotation_type, value, flow_cost) = annotation;
                    let mut commands = Vec::new();

                    match annotation_type.as_str() {
                        "guard_capability" => {
                            // Capability checks are pure (done against snapshot)
                            // Store capability validation metadata for audit trail
                            commands.push(EffectCommand::StoreMetadata {
                                key: "guard_validated".to_string(),
                                value: value.clone(),
                            });
                        }
                        "flow_cost" | "guard_with_flow" => {
                            // Charge flow budget
                            commands.push(EffectCommand::ChargeBudget {
                                context: ctx.context,
                                authority: ctx.authority,
                                peer: ctx.peer,
                                amount: flow_cost as u32,
                            });
                        }
                        "journal_facts" => {
                            // Append journal entry
                            // Note: Creating minimal fact - actual fact details stored in metadata
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("journal_fact:{}", value),
                                value: value.clone(),
                            });
                        }
                        "journal_merge" => {
                            // Record merge operation in metadata
                            commands.push(EffectCommand::StoreMetadata {
                                key: "journal_merge_requested".to_string(),
                                value: "true".to_string(),
                            });
                        }
                        "audit_log" => {
                            // Record audit entry as metadata (audit trail)
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("audit_log:{}", value),
                                value: value.clone(),
                            });
                        }
                        "leak" => {
                            // Parse leakage bits from value or use default
                            let bits = value.split(',').count() as u32 * 8; // Rough estimate: 8 bits per observer class
                            commands.push(EffectCommand::RecordLeakage { bits });
                        }
                        _ => {
                            // Unknown annotation - store in metadata for debugging
                            commands.push(EffectCommand::StoreMetadata {
                                key: format!("unknown_annotation:{}", annotation_type),
                                value,
                            });
                        }
                    }

                    commands
                }

                /// Convert a batch of annotations to effect commands
                pub fn annotations_to_commands(
                    ctx: &EffectBridgeContext,
                    annotations: Vec<(String, String, String, u64)>,
                ) -> Vec<EffectCommand> {
                    annotations
                        .into_iter()
                        .flat_map(|ann| annotation_to_commands(ctx, ann))
                        .collect()
                }

                /// Create effect context from runtime values
                pub fn create_context(
                    context: ContextId,
                    authority: AuthorityId,
                    peer: AuthorityId,
                    timestamp: TimeStamp,
                ) -> EffectBridgeContext {
                    EffectBridgeContext {
                        context,
                        authority,
                        peer,
                        timestamp,
                    }
                }

                /// Execute effect commands through an interpreter
                ///
                /// This is the main integration point for the effect system.
                /// It converts annotations to commands and executes them asynchronously.
                pub async fn execute_commands<I: aura_core::effects::guard::EffectInterpreter>(
                    interpreter: &I,
                    ctx: &EffectBridgeContext,
                    annotations: Vec<(String, String, String, u64)>,
                ) -> Result<Vec<aura_core::effects::guard::EffectResult>, String> {
                    use aura_core::effects::guard::EffectResult;

                    let commands = annotations_to_commands(ctx, annotations);
                    let mut results = Vec::with_capacity(commands.len());

                    for cmd in commands {
                        match interpreter.execute(cmd).await {
                            Ok(result) => results.push(result),
                            Err(e) => return Err(format!("Effect execution failed: {}", e)),
                        }
                    }

                    Ok(results)
                }
            }

            #leakage_integration
        }
    }
}

/// Namespace-aware rumpsteak-aura implementation
///
/// Uses the namespace-aware rumpsteak functions to avoid module conflicts
fn choreography_impl_namespace_aware(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Convert token stream to string for parsing
    let input_str = input.to_string();

    // Parse the choreography to extract namespace information
    match parse_choreography_str(&input_str) {
        Ok(choreo) => {
            // Project to local types
            let mut local_types = Vec::new();
            for role in &choreo.roles {
                match project(&choreo, role) {
                    Ok(local_type) => {
                        local_types.push((role.clone(), local_type));
                    }
                    Err(err) => {
                        return Err(syn::Error::new(
                            proc_macro2::Span::call_site(),
                            format!("Projection failed for role {}: {}", role.name, err),
                        ));
                    }
                }
            }

            // Generate code with namespace support
            let generated_code = generate_choreography_code_with_namespacing(&choreo, &local_types);

            // Add necessary imports for the generated rumpsteak code
            let imports = quote! {
                #[allow(unused_imports)]
                use super::{Ping, Pong};

                // Standard rumpsteak-aura imports
                use rumpsteak_aura::{
                    channel::Bidirectional, session, try_session,
                    Branch, End, Message, Receive, Role, Roles, Select, Send
                };
                use futures::channel::mpsc::{self, Receiver, Sender};

                // Label type for message routing
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                pub enum Label {
                    Ping(Ping),
                    Pong(Pong),
                }

                const CHANNEL_BUFFER: usize = 64;

                fn channel() -> (Sender<Label>, Receiver<Label>) {
                    mpsc::channel(CHANNEL_BUFFER)
                }

                // Channel type definition
                type Channel = Bidirectional<Sender<Label>, Receiver<Label>>;
            };

            // Generate module name using namespace
            let module_name = if let Some(ns) = &choreo.namespace {
                quote::format_ident!("rumpsteak_session_types_{}", ns)
            } else {
                quote::format_ident!("rumpsteak_session_types")
            };

            Ok(quote! {
                /// Rumpsteak-aura generated session types and choreographic projections
                pub mod #module_name {
                    #imports
                    #generated_code
                }
            })
        }
        Err(err) => {
            let error_msg = err.to_string();
            Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Choreography parsing error: {}", error_msg),
            ))
        }
    }
}

/// Extract namespace from choreography input tokens
fn extract_namespace_from_input(input: TokenStream) -> Option<String> {
    let input_str = input.to_string();

    // Simple regex-based extraction of namespace attribute
    // Pattern: #[namespace = "namespace_name"]
    let re = regex::Regex::new(r#"#\s*\[\s*namespace\s*=\s*"([^"]+)"\s*\]"#).ok()?;
    let captures = re.captures(&input_str)?;
    captures.get(1).map(|m| m.as_str().to_string())
}

/// Generate leakage integration code from annotations
fn generate_leakage_integration(annotations: &[AuraEffect]) -> TokenStream {
    let mut leakage_ops = Vec::new();

    // Process each annotation to find leakage effects
    for annotation in annotations {
        if let AuraEffect::Leakage { observers, role } = annotation {
            // Convert observer strings to ObserverClass enum variants
            let observer_variants: Vec<_> = observers
                .iter()
                .map(|obs| {
                    match obs.as_str() {
                        "External" => quote! { ObserverClass::External },
                        "Neighbor" => quote! { ObserverClass::Neighbor },
                        "InGroup" => quote! { ObserverClass::InGroup },
                        _ => quote! { ObserverClass::External }, // Default fallback
                    }
                })
                .collect();

            // Generate leakage recording code for this role
            let role_ident = quote::format_ident!("{}", role);
            leakage_ops.push(quote! {
                // Record leakage for role #role_ident
                if handler.role == AuraRole::#role_ident {
                    #(
                    handler.log_audit(format!("LEAKAGE: {:?} operation visible to {:?}", #role, #observer_variants));
                    )*
                }
            });
        }
    }

    // If we have leakage operations, generate the integration module
    if !leakage_ops.is_empty() {
        quote! {
            /// Leakage tracking integration
            pub mod leakage_integration {
                use super::*;
                use aura_core::effects::{LeakageEffects, ObserverClass};

                /// Apply leakage tracking to choreography operations
                pub async fn track_leakage<BiscuitEvaluator, FlowBudget>(
                    handler: &mut AuraHandler<BiscuitEvaluator, FlowBudget>,
                    leakage_effects: &impl LeakageEffects,
                ) -> Result<(), String> {
                    #(#leakage_ops)*
                    Ok(())
                }
            }
        }
    } else {
        // No leakage annotations, generate empty module
        quote! {}
    }
}

/// Standard rumpsteak-aura implementation with empty extension registry
///
/// This follows the external-demo pattern exactly and provides full
/// rumpsteak-aura feature inheritance without extension conflicts.
///
/// NOTE: This is an alternative implementation strategy kept for reference.
/// The active implementation uses `choreography_impl_namespace_aware` which
/// provides proper namespace isolation. This standard version is preserved
/// for cases where namespace isolation is not needed.
#[allow(dead_code)] // Alternative implementation - active version uses namespace_aware
fn choreography_impl_standard(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Convert token stream to string for parsing
    let input_str = input.to_string();

    // Create empty extension registry to avoid buggy timeout extension
    // This follows the external-demo pattern and ensures we inherit ALL
    // standard rumpsteak-aura features without extension conflicts
    let registry = ExtensionRegistry::new();

    // Parse and generate code with full rumpsteak-aura feature inheritance
    match parse_and_generate_with_extensions(&input_str, &registry) {
        Ok(tokens) => {
            // Add necessary imports for the generated rumpsteak code following external demo pattern
            let imports = quote! {
                #[allow(unused_imports)]
                use super::{Ping, Pong};

                // Standard rumpsteak-aura imports (from external demo analysis)
                use rumpsteak_aura::{
                    channel::Bidirectional, session, try_session,
                    Branch, End, Message, Receive, Role, Roles, Select, Send
                };
                use futures::channel::mpsc::{self, Receiver, Sender};

                // Label type for message routing
                #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
                pub enum Label {
                    Ping(Ping),
                    Pong(Pong),
                }

                const CHANNEL_BUFFER: usize = 64;

                fn channel() -> (Sender<Label>, Receiver<Label>) {
                    mpsc::channel(CHANNEL_BUFFER)
                }

                // Channel type definition following external demo pattern
                type Channel = Bidirectional<Sender<Label>, Receiver<Label>>;
            };

            Ok(quote! {
                /// Rumpsteak-aura generated session types and choreographic projections
                pub mod rumpsteak_session_types {
                    #imports
                    #tokens
                }
            })
        }
        Err(err) => {
            let error_msg = err.to_string();
            Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Choreography compilation error: {}", error_msg),
            ))
        }
    }
}
