//! # Aura Macros - Layer 2: Specification (DSL Compiler)
//!
//! **Purpose**: Compile-time DSL parser for choreographies with Aura-specific annotations.
//!
//! This crate provides choreography and effect handler macros for the Aura project,
//! implementing a compile-time DSL that parses `guard_capability`, `flow_cost`, `journal_facts`
//! and generates type-safe Rust code for distributed protocols.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - YES Choreography DSL parsing and code generation
//! - YES Aura-specific annotation extraction
//! - YES Type-safe macro generation for distributed protocols
//! - YES Integration with Telltale projection
//! - NO effect handler implementations (that's aura-effects)
//! - NO runtime coordination logic (that's aura-protocol)
//! - NO handler composition (that's aura-composition)

use proc_macro::TokenStream;
use quote::{quote, ToTokens};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    parse_quote, Block, Error, Expr, ExprAwait, ExprCall, ExprGroup, ExprLit, ExprMethodCall,
    ExprParen, ExprReference, FnArg, GenericArgument, ImplItemFn, ItemEnum, ItemFn, ItemStruct,
    Lit, LitStr, MetaNameValue, PatType, PathArguments, Result as SynResult, Token, Type, TypePath,
};

mod capability_family;
mod ceremony_facts;
mod choreography;
mod domain_fact;
mod effect_handlers;
mod effect_system;
mod error_types;
mod handler_adapters;
mod test_macros;

/// Full-featured choreography! macro with complete Telltale feature inheritance
///
/// This macro inherits ALL standard Telltale features including:
/// - Module namespaces: `module my_protocol exposing (ProtocolName)`
/// - Parameterized roles: `Worker[N]`, `Signer[*]`
/// - Choice constructs: `choice at Role { ... }`
/// - Loop constructs: `loop { ... }`
/// - Session type safety and choreographic projection
/// - Protocol composition and modular design
///
/// Following the external-demo pattern, we use an empty extension registry
/// to avoid buggy extensions while maintaining full feature inheritance.
///
/// # Example
///
/// ```ignore
/// use aura_macros::choreography;
///
/// choreography!(r#"
/// module threshold_ceremony exposing (ThresholdExample)
///
/// protocol ThresholdExample =
///   roles Coordinator, Signer[N]
///   Coordinator -> Signer[*] : StartRequest
///   Signer[*] -> Coordinator : Commitment
/// "#);
/// ```
#[proc_macro]
pub fn choreography(input: TokenStream) -> TokenStream {
    match choreography::choreography_impl(input.into()) {
        Ok(output) => output.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Derive macro for DomainFact implementations with canonical encoding.
///
/// Usage:
/// ```ignore
/// #[derive(DomainFact)]
/// #[domain_fact(type_id = "chat", schema_version = 1, context = "context_id")]
/// pub enum ChatFact { /* ... */ }
/// ```
#[proc_macro_derive(DomainFact, attributes(domain_fact))]
pub fn derive_domain_fact(input: TokenStream) -> TokenStream {
    domain_fact::derive_domain_fact_impl(input)
}

/// Generate effect handler implementations with mock and real variants
///
/// This macro eliminates boilerplate code for effect handler implementations by
/// generating consistent patterns for mock and real handler variants.
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_effect_handlers;
///
/// aura_effect_handlers! {
///     trait_name: StorageEffects,
///     mock: {
///         struct_name: MockStorageHandler,
///         state: {
///             data: HashMap<String, Vec<u8>>,
///         },
///         methods: {
///             read(key: String) -> Result<Vec<u8>, StorageError> => {
///                 self.data.get(&key)
///                     .cloned()
///                     .ok_or_else(|| StorageError::NotFound(key))
///             },
///             write(key: String, value: Vec<u8>) -> Result<(), StorageError> => {
///                 self.data.insert(key, value);
///                 Ok(())
///             },
///         },
///     },
///     real: {
///         struct_name: RealStorageHandler,
///         methods: {
///             read(key: String) -> Result<Vec<u8>, StorageError> => {
///                 std::fs::read(&key)
///                     .map_err(|e| StorageError::IoError(e.to_string()))
///             },
///             write(key: String, value: Vec<u8>) -> Result<(), StorageError> => {
///                 std::fs::write(&key, value)
///                     .map_err(|e| StorageError::IoError(e.to_string()))
///             },
///         },
///     },
/// }
/// ```
///
/// Note: Effect implementations in aura-effects are the abstraction boundary where
/// direct OS calls are expected. Application code should always use injected effects,
/// never call OS functions directly.
/// Attribute macro that adds canonical ceremony helpers to fact enums.
///
/// This macro expects the enum to define the standard ceremony variants:
/// `CeremonyInitiated`, `CeremonyAcceptanceReceived`, `CeremonyCommitted`,
/// `CeremonyAborted`, and `CeremonySuperseded`.
///
/// It generates `ceremony_id()` and `ceremony_timestamp_ms()` accessors.
#[proc_macro_attribute]
pub fn ceremony_facts(attr: TokenStream, item: TokenStream) -> TokenStream {
    ceremony_facts::ceremony_facts_impl(attr, item)
}

/// Declare a first-party capability family with validated canonical names.
///
/// Usage:
/// ```ignore
/// use aura_macros::capability_family;
///
/// #[capability_family(namespace = "invitation")]
/// pub enum InvitationCapability {
///     #[capability("send")]
///     Send,
///     #[capability("guardian:accept")]
///     GuardianAccept,
/// }
/// ```
#[proc_macro_attribute]
pub fn capability_family(attr: TokenStream, item: TokenStream) -> TokenStream {
    capability_family::capability_family_impl(attr, item)
}

#[proc_macro]
pub fn aura_effect_handlers(input: TokenStream) -> TokenStream {
    match effect_handlers::aura_effect_handlers_impl(input) {
        Ok(output) => output,
        Err(err) => err.to_compile_error().into(),
    }
}

/// Generate handler adapter implementations for the AuraHandler trait
///
/// This macro eliminates boilerplate for creating handler adapters that bridge
/// effect traits to the AuraHandler trait for use in the stateless executor.
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_handler_adapters;
///
/// aura_handler_adapters! {
///     TimeHandlerAdapter: TimeEffects => Time {
///         "current_epoch" => current_epoch() -> u64,
///         "sleep_ms" => sleep_ms(u64),
///         "set_timeout" => set_timeout(u64) -> TimeoutHandle,
///     },
///     NetworkHandlerAdapter: NetworkEffects => Network {
///         "send_to_peer" => send_to_peer((Uuid, Vec<u8>)),
///         "receive" => receive() -> Vec<u8>,
///     }
/// }
/// ```
#[proc_macro]
pub fn aura_handler_adapters(input: TokenStream) -> TokenStream {
    handler_adapters::aura_handler_adapters_impl(input)
}

/// Generate effect trait implementations with automatic execution patterns
///
/// This macro eliminates the repetitive serialize → execute → deserialize pattern
/// that appears hundreds of times in effect system implementations.
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_effect_implementations;
///
/// aura_effect_implementations! {
///     TimeEffects: Time -> TimeError {
///         "current_epoch" => current_epoch() -> u64,
///         "sleep_ms" => sleep_ms(u64),
///         "set_timeout" => set_timeout(u64) -> TimeoutHandle,
///     },
///     NetworkEffects: Network -> NetworkError {
///         "send_to_peer" => send_to_peer((uuid::Uuid, Vec<u8>)),
///         "receive" => receive() -> Vec<u8>,
///     }
/// }
/// ```
#[proc_macro]
pub fn aura_effect_implementations(input: TokenStream) -> TokenStream {
    effect_system::aura_effect_implementations_impl(input)
}

/// Generate error type definitions with automatic implementations
///
/// This macro eliminates boilerplate in error type definitions by auto-generating
/// Display implementations, From conversions, constructor helpers, and other
/// common patterns that appear across 66+ files with 2,000+ lines of repetition.
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_error_types;
///
/// aura_error_types! {
///     #[derive(Debug, Clone, Serialize, Deserialize)]
///     pub enum StorageError {
///         #[category = "not_found"]
///         ContentNotFound { content_id: String } => "Content not found: {content_id}",
///
///         #[category = "storage"]
///         QuotaExceeded { requested: u64, available: u64 } =>
///             "Storage quota exceeded: requested {requested} bytes, available {available} bytes",
///
///         NetworkTimeout => "Network operation timed out",
///     }
/// }
/// ```
#[proc_macro]
pub fn aura_error_types(input: TokenStream) -> TokenStream {
    error_types::aura_error_types_impl(input)
}

/// Attribute macro for Aura async tests with automatic setup
///
/// This macro wraps async tests and provides:
/// - Automatic tracing initialization via `aura_testkit::init_test_tracing()`
/// - 30 second timeout by default
/// - Better error messages on timeout
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_test;
/// use aura_core::AuraResult;
///
/// #[aura_test]
/// async fn my_test() -> AuraResult<()> {
///     // Tracing is automatically initialized
///     // Test has 30s timeout
///     let fixture = aura_testkit::create_test_fixture().await?;
///     assert_ne!(fixture.device_id().to_string(), "");
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn aura_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    test_macros::aura_test_impl(attr, item)
}

/// Marker attribute for parity-critical semantic owner functions.
///
/// The attribute binds the function to Aura's canonical typed semantic-owner
/// protocol in `aura_core::SemanticOwnerProtocol` and remains inspectable by
/// repo-local ownership lints for body-policy enforcement.
#[proc_macro_attribute]
pub fn semantic_owner(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<SemanticOwnerAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_semantic_owner(item, config)
}

/// Marker attribute for best-effort side-effect boundaries.
///
/// The attribute binds the function to Aura's canonical typed best-effort
/// protocol in `aura_core::BestEffortBoundaryProtocol` and remains inspectable
/// by repo-local ownership lints for side-effect-boundary enforcement.
#[proc_macro_attribute]
pub fn best_effort_boundary(_attr: TokenStream, item: TokenStream) -> TokenStream {
    transform_best_effort_boundary(item)
}

/// Marker attribute for helpers that mint or read authoritative semantic truth.
///
/// The marker is validated so Rust-native ownership lints can rely on it as a
/// real declaration surface rather than an unchecked comment replacement.
#[proc_macro_attribute]
pub fn authoritative_source(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<AuthoritativeSourceAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_authoritative_source(item, config)
}

/// Marker attribute for canonical strong-reference types.
///
/// The marker is validated so Rust-native ownership lints can distinguish
/// canonical owned references/handles from weak identifier inputs reliably.
#[proc_macro_attribute]
pub fn strong_reference(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<StrongReferenceAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_strong_reference(item, config)
}

/// Marker attribute for weak identifier carrier types.
///
/// The marker is validated so repo-local ownership lints can model
/// weak-to-strong upgrade boundaries explicitly.
#[proc_macro_attribute]
pub fn weak_identifier(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<WeakIdentifierAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_weak_identifier(item, config)
}

/// Marker attribute for helpers that are projection/display-only.
///
/// This is currently a no-op at expansion time and exists so repo-local
/// ownership lints can distinguish observed-only reads from semantic workflow
/// ownership.
#[proc_macro_attribute]
pub fn observed_only(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}

/// Marker attribute for values that must reach typed terminal settlement.
#[proc_macro_attribute]
pub fn must_settle(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<MustSettleAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_must_settle(item, config)
}

/// Marker attribute for proofs minted only by an owning boundary.
#[proc_macro_attribute]
pub fn owner_issued_proof(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<OwnerIssuedProofAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_owner_issued_proof(item, config)
}

#[proc_macro_attribute]
pub fn actor_owned(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<ActorOwnedAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    let strukt = match syn::parse::<ItemStruct>(item.clone()) {
        Ok(strukt) => strukt,
        Err(_) => {
            return Error::new(
                proc_macro2::Span::call_site(),
                "#[actor_owned] may only be applied to structs",
            )
            .to_compile_error()
            .into();
        }
    };

    if let Err(error) = validate_actor_owned_struct(&strukt, &config) {
        return error.to_compile_error().into();
    }

    let ident = &strukt.ident;
    let owner = config.owner;
    let domain = config.domain;
    let gate = config.gate;
    let command = config.command;
    let capacity = config.capacity;
    let category = config.category;

    quote! {
        #strukt

        impl #ident {
            pub const ACTOR_BOUNDARY_CATEGORY: ::aura_core::BoundaryDeclarationCategory =
                ::aura_core::BoundaryDeclarationCategory::ActorOwned;
            pub const ACTOR_OWNER_NAME: &'static str = #owner;
            pub const ACTOR_DOMAIN_NAME: &'static str = #domain;
            pub const ACTOR_INGRESS_GATE: &'static str = #gate;
            pub const ACTOR_DECLARATION_CATEGORY_LITERAL: &'static str = #category;

            pub fn actor_declaration() -> ::aura_core::actor_owned::ActorDeclaration<Self, #command> {
                let _: ::aura_core::BoundaryDeclarationCategory =
                    ::aura_core::BoundaryDeclarationCategory::ActorOwned;
                ::aura_core::actor_owned::ActorDeclaration::new(#owner, #domain, #gate, #capacity)
            }

            pub fn actor_ingress() -> ::aura_core::actor_owned::BoundedActorIngress<Self, #command> {
                Self::actor_declaration().into_ingress()
            }

            pub fn register_actor_supervision<HandleId>(
                handle_id: HandleId,
                shutdown: ::aura_core::actor_owned::OwnedShutdownToken,
            ) -> ::aura_core::actor_owned::SupervisionRegistration<Self, #command, HandleId> {
                Self::actor_declaration().register_supervision(handle_id, shutdown)
            }
        }
    }
    .into()
}

#[proc_macro_attribute]
pub fn actor_root(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<ActorRootAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    let strukt = match syn::parse::<ItemStruct>(item.clone()) {
        Ok(strukt) => strukt,
        Err(_) => {
            return Error::new(
                proc_macro2::Span::call_site(),
                "#[actor_root] may only be applied to structs",
            )
            .to_compile_error()
            .into();
        }
    };

    if let Err(error) = validate_actor_root_struct(&strukt, &config) {
        return error.to_compile_error().into();
    }

    let ident = &strukt.ident;
    let owner = config.owner;
    let domain = config.domain;
    let supervision = config.supervision;
    let category = config.category;

    quote! {
        #strukt

        impl #ident {
            pub const ACTOR_ROOT_BOUNDARY_CATEGORY: ::aura_core::BoundaryDeclarationCategory =
                ::aura_core::BoundaryDeclarationCategory::ActorOwned;
            pub const ACTOR_ROOT_OWNER_NAME: &'static str = #owner;
            pub const ACTOR_ROOT_DOMAIN_NAME: &'static str = #domain;
            pub const ACTOR_ROOT_SUPERVISION_GATE: &'static str = #supervision;
            pub const ACTOR_ROOT_DECLARATION_CATEGORY_LITERAL: &'static str = #category;

            pub fn actor_root_declaration() -> ::aura_core::ActorRootDeclaration<Self> {
                let _: ::aura_core::BoundaryDeclarationCategory =
                    ::aura_core::BoundaryDeclarationCategory::ActorOwned;
                ::aura_core::ActorRootDeclaration::new(#owner, #domain, #supervision)
            }

            pub fn register_actor_root_supervision<HandleId>(
                handle_id: HandleId,
                shutdown: ::aura_core::OwnedShutdownToken,
            ) -> ::aura_core::ActorRootSupervisionRegistration<Self, HandleId> {
                Self::actor_root_declaration().register_supervision(handle_id, shutdown)
            }
        }
    }
    .into()
}

/// Marker attribute for capability-gated mutation/publication points.
#[proc_macro_attribute]
pub fn capability_boundary(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<CapabilityBoundaryAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_capability_boundary(item, config)
}

/// Generate a legal transition surface for small boundary state machines.
#[proc_macro_attribute]
pub fn ownership_lifecycle(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = match syn::parse::<OwnershipLifecycleAttr>(attr) {
        Ok(config) => config,
        Err(error) => return error.to_compile_error().into(),
    };
    transform_ownership_lifecycle(item, config)
}

struct SemanticOwnerAttr {
    owner: LitStr,
    terminal: LitStr,
    postcondition: LitStr,
    proof: Option<Type>,
    authoritative_inputs: Vec<LitStr>,
    depends_on: Vec<LitStr>,
    child_ops: Vec<LitStr>,
    category: LitStr,
}

struct ActorOwnedAttr {
    owner: LitStr,
    domain: LitStr,
    gate: LitStr,
    command: Type,
    capacity: u32,
    category: LitStr,
}

struct ActorRootAttr {
    owner: LitStr,
    domain: LitStr,
    supervision: LitStr,
    category: LitStr,
}

struct CapabilityBoundaryAttr {
    category: LitStr,
    capability: LitStr,
}

struct AuthoritativeSourceAttr {
    kind: LitStr,
}

struct StrongReferenceAttr {
    domain: LitStr,
}

struct WeakIdentifierAttr {
    domain: LitStr,
}

struct MustSettleAttr {
    kind: LitStr,
}

struct OwnerIssuedProofAttr {
    domain: LitStr,
}

struct OwnershipLifecycleAttr {
    initial: LitStr,
    ordered: Vec<LitStr>,
    terminals: Vec<LitStr>,
}

impl Parse for ActorOwnedAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut owner = None;
        let mut domain = None;
        let mut gate = None;
        let mut command = None;
        let mut capacity = None;
        let mut category = None;

        for meta in metas {
            if meta.path.is_ident("owner") {
                owner = Some(expect_string_literal(&meta, "owner", "actor_owned")?);
            } else if meta.path.is_ident("domain") {
                domain = Some(expect_string_literal(&meta, "domain", "actor_owned")?);
            } else if meta.path.is_ident("gate") {
                gate = Some(expect_string_literal(&meta, "gate", "actor_owned")?);
            } else if meta.path.is_ident("command") {
                command = Some(expect_type_value(&meta, "command", "actor_owned")?);
            } else if meta.path.is_ident("capacity") {
                capacity = Some(expect_u32_literal(&meta, "capacity", "actor_owned")?);
            } else if meta.path.is_ident("category") {
                category = Some(expect_string_literal(&meta, "category", "actor_owned")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported actor_owned attribute key; expected `owner`, `domain`, `gate`, `command`, `capacity`, or `category`",
                ));
            }
        }

        Ok(Self {
            owner: owner.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `owner = \"...\"`",
                )
            })?,
            domain: domain.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `domain = \"...\"`",
                )
            })?,
            gate: gate.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `gate = \"...\"`",
                )
            })?,
            command: command.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `command = Type`",
                )
            })?,
            capacity: capacity.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `capacity = N`",
                )
            })?,
            category: category.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_owned requires `category = \"actor_owned\"`",
                )
            })?,
        })
    }
}

impl Parse for ActorRootAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut owner = None;
        let mut domain = None;
        let mut supervision = None;
        let mut category = None;

        for meta in metas {
            if meta.path.is_ident("owner") {
                owner = Some(expect_string_literal(&meta, "owner", "actor_root")?);
            } else if meta.path.is_ident("domain") {
                domain = Some(expect_string_literal(&meta, "domain", "actor_root")?);
            } else if meta.path.is_ident("supervision") {
                supervision = Some(expect_string_literal(&meta, "supervision", "actor_root")?);
            } else if meta.path.is_ident("category") {
                category = Some(expect_string_literal(&meta, "category", "actor_root")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported actor_root attribute key; expected `owner`, `domain`, `supervision`, or `category`",
                ));
            }
        }

        Ok(Self {
            owner: owner.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_root requires `owner = \"...\"`",
                )
            })?,
            domain: domain.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_root requires `domain = \"...\"`",
                )
            })?,
            supervision: supervision.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_root requires `supervision = \"...\"`",
                )
            })?,
            category: category.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "actor_root requires `category = \"actor_owned\"`",
                )
            })?,
        })
    }
}

impl Parse for SemanticOwnerAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut owner = None;
        let mut terminal = None;
        let mut postcondition = None;
        let mut proof = None;
        let mut authoritative_inputs = None;
        let mut depends_on = None;
        let mut child_ops = None;
        let mut category = None;

        for meta in metas {
            if meta.path.is_ident("owner") {
                owner = Some(expect_string_literal(&meta, "owner", "semantic_owner")?);
            } else if meta.path.is_ident("terminal") {
                terminal = Some(expect_string_literal(&meta, "terminal", "semantic_owner")?);
            } else if meta.path.is_ident("postcondition") {
                postcondition = Some(expect_string_literal(
                    &meta,
                    "postcondition",
                    "semantic_owner",
                )?);
            } else if meta.path.is_ident("proof") {
                proof = Some(expect_type_value(&meta, "proof", "semantic_owner")?);
            } else if meta.path.is_ident("authoritative_inputs") {
                authoritative_inputs = Some(parse_optional_list_literal(&expect_string_literal(
                    &meta,
                    "authoritative_inputs",
                    "semantic_owner",
                )?)?);
            } else if meta.path.is_ident("depends_on") {
                depends_on = Some(parse_optional_list_literal(&expect_string_literal(
                    &meta,
                    "depends_on",
                    "semantic_owner",
                )?)?);
            } else if meta.path.is_ident("child_ops") {
                child_ops = Some(parse_optional_list_literal(&expect_string_literal(
                    &meta,
                    "child_ops",
                    "semantic_owner",
                )?)?);
            } else if meta.path.is_ident("category") {
                category = Some(expect_string_literal(&meta, "category", "semantic_owner")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported semantic_owner attribute key; expected `owner`, `terminal`, `postcondition`, `proof`, `authoritative_inputs`, `depends_on`, `child_ops`, or `category`",
                ));
            }
        }

        Ok(Self {
            owner: owner.ok_or_else(|| {
                Error::new(proc_macro2::Span::call_site(), "semantic_owner requires `owner = \"...\"`")
            })?,
            terminal: terminal.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `terminal = \"...\"`",
                )
            })?,
            postcondition: postcondition.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `postcondition = \"...\"`",
                )
            })?,
            proof: Some(proof.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `proof = TypePath`",
                )
            })?),
            authoritative_inputs: authoritative_inputs.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `authoritative_inputs = \"a,b,...\"` (use empty string for none)",
                )
            })?,
            depends_on: depends_on.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `depends_on = \"a,b,...\"` (use empty string for none)",
                )
            })?,
            child_ops: child_ops.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `child_ops = \"a,b,...\"` (use empty string for none)",
                )
            })?,
            category: category.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "semantic_owner requires `category = \"move_owned\"`",
                )
            })?,
        })
    }
}

impl Parse for CapabilityBoundaryAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut category = None;
        let mut capability = None;

        for meta in metas {
            if meta.path.is_ident("category") {
                category = Some(expect_string_literal(
                    &meta,
                    "category",
                    "capability_boundary",
                )?);
            } else if meta.path.is_ident("capability") {
                capability = Some(expect_string_literal(
                    &meta,
                    "capability",
                    "capability_boundary",
                )?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported capability_boundary attribute key; expected `category` or `capability`",
                ));
            }
        }

        Ok(Self {
            category: category.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "capability_boundary requires `category = \"capability_gated\"`",
                )
            })?,
            capability: capability.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "capability_boundary requires `capability = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for AuthoritativeSourceAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut kind = None;

        for meta in metas {
            if meta.path.is_ident("kind") {
                kind = Some(expect_string_literal(
                    &meta,
                    "kind",
                    "authoritative_source",
                )?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported authoritative_source attribute key; expected `kind`",
                ));
            }
        }

        Ok(Self {
            kind: kind.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "authoritative_source requires `kind = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for StrongReferenceAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut domain = None;

        for meta in metas {
            if meta.path.is_ident("domain") {
                domain = Some(expect_string_literal(&meta, "domain", "strong_reference")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported strong_reference attribute key; expected `domain`",
                ));
            }
        }

        Ok(Self {
            domain: domain.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "strong_reference requires `domain = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for WeakIdentifierAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut domain = None;

        for meta in metas {
            if meta.path.is_ident("domain") {
                domain = Some(expect_string_literal(&meta, "domain", "weak_identifier")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported weak_identifier attribute key; expected `domain`",
                ));
            }
        }

        Ok(Self {
            domain: domain.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "weak_identifier requires `domain = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for MustSettleAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut kind = None;

        for meta in metas {
            if meta.path.is_ident("kind") {
                kind = Some(expect_string_literal(&meta, "kind", "must_settle")?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported must_settle attribute key; expected `kind`",
                ));
            }
        }

        Ok(Self {
            kind: kind.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "must_settle requires `kind = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for OwnerIssuedProofAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut domain = None;

        for meta in metas {
            if meta.path.is_ident("domain") {
                domain = Some(expect_string_literal(
                    &meta,
                    "domain",
                    "owner_issued_proof",
                )?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported owner_issued_proof attribute key; expected `domain`",
                ));
            }
        }

        Ok(Self {
            domain: domain.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "owner_issued_proof requires `domain = \"...\"`",
                )
            })?,
        })
    }
}

impl Parse for OwnershipLifecycleAttr {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let metas = Punctuated::<MetaNameValue, Token![,]>::parse_terminated(input)?;
        let mut initial = None;
        let mut ordered = None;
        let mut terminals = None;

        for meta in metas {
            if meta.path.is_ident("initial") {
                initial = Some(expect_string_literal(
                    &meta,
                    "initial",
                    "ownership_lifecycle",
                )?);
            } else if meta.path.is_ident("ordered") {
                ordered = Some(parse_list_literal(&expect_string_literal(
                    &meta,
                    "ordered",
                    "ownership_lifecycle",
                )?)?);
            } else if meta.path.is_ident("terminals") {
                terminals = Some(parse_list_literal(&expect_string_literal(
                    &meta,
                    "terminals",
                    "ownership_lifecycle",
                )?)?);
            } else {
                return Err(Error::new_spanned(
                    meta,
                    "unsupported ownership_lifecycle attribute key; expected `initial`, `ordered`, or `terminals`",
                ));
            }
        }

        Ok(Self {
            initial: initial.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "ownership_lifecycle requires `initial = \"Variant\"`",
                )
            })?,
            ordered: ordered.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "ownership_lifecycle requires `ordered = \"A,B,...\"`",
                )
            })?,
            terminals: terminals.ok_or_else(|| {
                Error::new(
                    proc_macro2::Span::call_site(),
                    "ownership_lifecycle requires `terminals = \"T1,T2,...\"`",
                )
            })?,
        })
    }
}

fn expect_string_literal(meta: &MetaNameValue, name: &str, attr: &str) -> SynResult<LitStr> {
    match &meta.value {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Ok(value.clone()),
        other => Err(Error::new_spanned(
            other,
            format!("{attr} `{name}` value must be a string literal"),
        )),
    }
}

fn expect_type_value(meta: &MetaNameValue, name: &str, attr: &str) -> SynResult<Type> {
    match &meta.value {
        Expr::Path(expr_path) => Ok(Type::Path(TypePath {
            qself: None,
            path: expr_path.path.clone(),
        })),
        other => Err(Error::new_spanned(
            other,
            format!("{attr} `{name}` value must be a type"),
        )),
    }
}

fn expect_u32_literal(meta: &MetaNameValue, name: &str, attr: &str) -> SynResult<u32> {
    match &meta.value {
        Expr::Lit(ExprLit {
            lit: Lit::Int(value),
            ..
        }) => value
            .base10_parse::<u32>()
            .map_err(|_| Error::new_spanned(value, format!("{attr} `{name}` must fit in u32"))),
        other => Err(Error::new_spanned(
            other,
            format!("{attr} `{name}` value must be an integer literal"),
        )),
    }
}

fn parse_list_literal(list: &LitStr) -> SynResult<Vec<LitStr>> {
    let parsed = list
        .value()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| LitStr::new(value, list.span()))
        .collect::<Vec<_>>();
    if parsed.is_empty() {
        return Err(Error::new_spanned(
            list,
            "expected a non-empty comma-separated variant list",
        ));
    }
    Ok(parsed)
}

fn parse_optional_list_literal(list: &LitStr) -> SynResult<Vec<LitStr>> {
    if list.value().trim().is_empty() {
        return Ok(Vec::new());
    }
    parse_list_literal(list)
}

fn transform_authoritative_source(
    item: TokenStream,
    config: AuthoritativeSourceAttr,
) -> TokenStream {
    if let Ok(function) = syn::parse::<ItemFn>(item.clone()) {
        if let Err(error) = validate_authoritative_source_kind(&config.kind) {
            return error.to_compile_error().into();
        }
        return quote! { #function }.into();
    }
    if let Ok(function) = syn::parse::<ImplItemFn>(item.clone()) {
        if let Err(error) = validate_authoritative_source_kind(&config.kind) {
            return error.to_compile_error().into();
        }
        return quote! { #function }.into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[authoritative_source] may only be applied to free or impl functions",
    )
    .to_compile_error()
    .into()
}

fn transform_strong_reference(item: TokenStream, config: StrongReferenceAttr) -> TokenStream {
    if let Err(error) = validate_reference_domain(&config.domain, "strong_reference") {
        return error.to_compile_error().into();
    }
    if let Ok(strukt) = syn::parse::<ItemStruct>(item.clone()) {
        return quote! { #strukt }.into();
    }
    if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        return quote! { #item_enum }.into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[strong_reference] may only be applied to structs or enums",
    )
    .to_compile_error()
    .into()
}

fn transform_weak_identifier(item: TokenStream, config: WeakIdentifierAttr) -> TokenStream {
    if let Err(error) = validate_reference_domain(&config.domain, "weak_identifier") {
        return error.to_compile_error().into();
    }
    if let Ok(strukt) = syn::parse::<ItemStruct>(item.clone()) {
        return quote! { #strukt }.into();
    }
    if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        return quote! { #item_enum }.into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[weak_identifier] may only be applied to structs or enums",
    )
    .to_compile_error()
    .into()
}

fn transform_must_settle(item: TokenStream, config: MustSettleAttr) -> TokenStream {
    let kind = config.kind;
    if let Ok(strukt) = syn::parse::<ItemStruct>(item.clone()) {
        return quote! {
            #strukt
            const _: &'static str = #kind;
        }
        .into();
    }
    if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        return quote! {
            #item_enum
            const _: &'static str = #kind;
        }
        .into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[must_settle] may only be applied to structs or enums",
    )
    .to_compile_error()
    .into()
}

fn transform_owner_issued_proof(item: TokenStream, config: OwnerIssuedProofAttr) -> TokenStream {
    if let Err(error) = validate_reference_domain(&config.domain, "owner_issued_proof") {
        return error.to_compile_error().into();
    }
    let domain = config.domain;
    if let Ok(strukt) = syn::parse::<ItemStruct>(item.clone()) {
        return quote! {
            #strukt
            const _: &'static str = #domain;
        }
        .into();
    }
    if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        return quote! {
            #item_enum
            const _: &'static str = #domain;
        }
        .into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[owner_issued_proof] may only be applied to structs or enums",
    )
    .to_compile_error()
    .into()
}

fn validate_authoritative_source_kind(kind: &LitStr) -> SynResult<()> {
    if matches!(
        kind.value().as_str(),
        "runtime" | "signal" | "app_core" | "proof_issuer"
    ) {
        return Ok(());
    }
    Err(Error::new_spanned(
        kind,
        "authoritative_source kind must be one of `runtime`, `signal`, `app_core`, or `proof_issuer`",
    ))
}

fn validate_reference_domain(domain: &LitStr, attr: &str) -> SynResult<()> {
    if matches!(
        domain.value().as_str(),
        "channel" | "invitation" | "ceremony" | "home" | "home_scope"
    ) {
        return Ok(());
    }
    Err(Error::new_spanned(
        domain,
        format!(
            "{attr} domain must be one of `channel`, `invitation`, `ceremony`, `home`, or `home_scope`"
        ),
    ))
}

fn transform_semantic_owner(item: TokenStream, config: SemanticOwnerAttr) -> TokenStream {
    if let Ok(function) = syn::parse::<ItemFn>(item.clone()) {
        return transform_semantic_owner_fn(function, config).into();
    }
    if let Ok(function) = syn::parse::<ImplItemFn>(item.clone()) {
        return transform_semantic_owner_impl_fn(function, config).into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[semantic_owner] may only be applied to free or impl async functions",
    )
    .to_compile_error()
    .into()
}

fn transform_semantic_owner_fn(
    mut function: ItemFn,
    config: SemanticOwnerAttr,
) -> proc_macro2::TokenStream {
    let owner = config.owner.clone();
    let terminal = config.terminal.clone();
    let postcondition = config.postcondition.clone();
    let proof = config.proof.clone();
    let authoritative_inputs = config.authoritative_inputs.clone();
    let depends_on = config.depends_on.clone();
    let child_ops = config.child_ops.clone();
    if let Err(error) = validate_semantic_owner_signature(
        &function.sig.inputs,
        function.sig.asyncness.is_some(),
        &function.block,
        &config,
    ) {
        return error.to_compile_error();
    }
    function.block.stmts.insert(
        0,
        parse_quote! {
            let _: ::aura_core::SemanticOwnerProtocol =
                ::aura_core::SemanticOwnerProtocol::CANONICAL;
        },
    );
    function.block.stmts.insert(
        1,
        parse_quote! {
            let _: &'static str = #owner;
        },
    );
    function.block.stmts.insert(
        2,
        parse_quote! {
            let _: ::aura_core::BoundaryDeclarationCategory =
                ::aura_core::BoundaryDeclarationCategory::MoveOwned;
        },
    );
    function.block.stmts.insert(
        3,
        parse_quote! {
            let _: ::aura_core::SemanticOwnerPostcondition =
                ::aura_core::SemanticOwnerPostcondition::new(#postcondition);
        },
    );
    if let Some(proof) = proof.clone() {
        function.block.stmts.insert(
            4,
            parse_quote! {
                let _: fn(#proof) = |proof| {
                    let _: ::aura_core::SemanticOwnerPostcondition =
                        ::aura_core::SemanticSuccessProof::declared_postcondition(&proof);
                };
            },
        );
    }
    let proof_stmt_count = usize::from(proof.is_some());
    for (index, input) in authoritative_inputs.iter().enumerate() {
        let stmt_index = 4 + proof_stmt_count + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerAuthoritativeInput =
                    ::aura_core::SemanticOwnerAuthoritativeInput::new(#input);
            },
        );
    }
    for (index, dependency) in depends_on.iter().enumerate() {
        let stmt_index = 4 + proof_stmt_count + authoritative_inputs.len() + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerDependency =
                    ::aura_core::SemanticOwnerDependency::new(#dependency);
            },
        );
    }
    for (index, child_op) in child_ops.iter().enumerate() {
        let stmt_index =
            4 + proof_stmt_count + authoritative_inputs.len() + depends_on.len() + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerChildOperation =
                    ::aura_core::SemanticOwnerChildOperation::new(#child_op);
            },
        );
    }
    function.block.stmts.insert(
        4 + proof_stmt_count + authoritative_inputs.len() + depends_on.len() + child_ops.len(),
        parse_quote! {
            let _: &'static str = #terminal;
        },
    );
    quote!(#function)
}

fn transform_semantic_owner_impl_fn(
    mut function: ImplItemFn,
    config: SemanticOwnerAttr,
) -> proc_macro2::TokenStream {
    let owner = config.owner.clone();
    let terminal = config.terminal.clone();
    let postcondition = config.postcondition.clone();
    let proof = config.proof.clone();
    let authoritative_inputs = config.authoritative_inputs.clone();
    let depends_on = config.depends_on.clone();
    let child_ops = config.child_ops.clone();
    if let Err(error) = validate_semantic_owner_signature(
        &function.sig.inputs,
        function.sig.asyncness.is_some(),
        &function.block,
        &config,
    ) {
        return error.to_compile_error();
    }
    function.block.stmts.insert(
        0,
        parse_quote! {
            let _: ::aura_core::SemanticOwnerProtocol =
                ::aura_core::SemanticOwnerProtocol::CANONICAL;
        },
    );
    function.block.stmts.insert(
        1,
        parse_quote! {
            let _: &'static str = #owner;
        },
    );
    function.block.stmts.insert(
        2,
        parse_quote! {
            let _: ::aura_core::BoundaryDeclarationCategory =
                ::aura_core::BoundaryDeclarationCategory::MoveOwned;
        },
    );
    function.block.stmts.insert(
        3,
        parse_quote! {
            let _: ::aura_core::SemanticOwnerPostcondition =
                ::aura_core::SemanticOwnerPostcondition::new(#postcondition);
        },
    );
    if let Some(proof) = proof.clone() {
        function.block.stmts.insert(
            4,
            parse_quote! {
                let _: fn(#proof) = |proof| {
                    let _: ::aura_core::SemanticOwnerPostcondition =
                        ::aura_core::SemanticSuccessProof::declared_postcondition(&proof);
                };
            },
        );
    }
    let proof_stmt_count = usize::from(proof.is_some());
    for (index, input) in authoritative_inputs.iter().enumerate() {
        let stmt_index = 4 + proof_stmt_count + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerAuthoritativeInput =
                    ::aura_core::SemanticOwnerAuthoritativeInput::new(#input);
            },
        );
    }
    for (index, dependency) in depends_on.iter().enumerate() {
        let stmt_index = 4 + proof_stmt_count + authoritative_inputs.len() + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerDependency =
                    ::aura_core::SemanticOwnerDependency::new(#dependency);
            },
        );
    }
    for (index, child_op) in child_ops.iter().enumerate() {
        let stmt_index =
            4 + proof_stmt_count + authoritative_inputs.len() + depends_on.len() + index;
        function.block.stmts.insert(
            stmt_index,
            parse_quote! {
                let _: ::aura_core::SemanticOwnerChildOperation =
                    ::aura_core::SemanticOwnerChildOperation::new(#child_op);
            },
        );
    }
    function.block.stmts.insert(
        4 + proof_stmt_count + authoritative_inputs.len() + depends_on.len() + child_ops.len(),
        parse_quote! {
            let _: &'static str = #terminal;
        },
    );
    quote!(#function)
}

fn validate_semantic_owner_signature(
    inputs: &syn::punctuated::Punctuated<FnArg, Token![,]>,
    is_async: bool,
    block: &syn::Block,
    config: &SemanticOwnerAttr,
) -> SynResult<()> {
    if config.category.value() != "move_owned" {
        return Err(Error::new_spanned(
            &config.category,
            "semantic_owner category must be `move_owned`",
        ));
    }

    if !is_async {
        return Err(Error::new_spanned(
            block,
            "semantic_owner requires an async function",
        ));
    }

    let has_operation_context = inputs.iter().any(fn_arg_contains_operation_context);
    if !has_operation_context {
        return Err(Error::new_spanned(
            inputs,
            "semantic_owner requires a parameter typed as OperationContext or containing OperationContext",
        ));
    }

    let block_tokens = quote!(#block).to_string();
    let terminal_name = config.terminal.value();
    if !block_tokens.contains(&terminal_name) {
        return Err(Error::new_spanned(
            block,
            format!(
                "semantic_owner requires sanctioned terminal path `{}` to appear in the function body",
                terminal_name
            ),
        ));
    }

    validate_semantic_owner_body(block)?;

    Ok(())
}

fn validate_actor_owned_struct(strukt: &ItemStruct, config: &ActorOwnedAttr) -> SynResult<()> {
    if config.category.value() != "actor_owned" {
        return Err(Error::new_spanned(
            &config.category,
            "actor_owned category must be `actor_owned`",
        ));
    }

    let ident = strukt.ident.to_string();
    if !matches_actor_owned_name(&ident) {
        return Err(Error::new_spanned(
            &strukt.ident,
            "actor_owned is reserved for long-lived mutable async domains (expected a name ending with `Service`, `Manager`, `Coordinator`, `Subsystem`, or `Actor`)",
        ));
    }

    for field in &strukt.fields {
        if type_contains_forbidden_actor_substitute(&field.ty) {
            return Err(Error::new_spanned(
                &field.ty,
                "actor_owned structs may not embed move-owned handoff or terminal-publication primitives directly",
            ));
        }
    }

    Ok(())
}

fn validate_actor_root_struct(strukt: &ItemStruct, config: &ActorRootAttr) -> SynResult<()> {
    if config.category.value() != "actor_owned" {
        return Err(Error::new_spanned(
            &config.category,
            "actor_root category must be `actor_owned`",
        ));
    }

    let ident = strukt.ident.to_string();
    if !matches_actor_owned_name(&ident) {
        return Err(Error::new_spanned(
            &strukt.ident,
            "actor_root is reserved for long-lived mutable async service roots (expected a name ending with `Service`, `Manager`, `Coordinator`, `Subsystem`, or `Actor`)",
        ));
    }

    Ok(())
}

fn matches_actor_owned_name(name: &str) -> bool {
    ["Service", "Manager", "Coordinator", "Subsystem", "Actor"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
}

fn type_contains_forbidden_actor_substitute(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => {
            type_path
                .path
                .segments
                .last()
                .is_some_and(|segment| {
                    matches!(
                        segment.ident.to_string().as_str(),
                        "OperationContext"
                            | "TerminalPublisher"
                            | "AuthorizedTerminalPublication"
                            | "OwnershipTransfer"
                    )
                })
                || type_path.path.segments.iter().any(|segment| match &segment.arguments {
                    PathArguments::AngleBracketed(arguments) => arguments.args.iter().any(|arg| {
                        matches!(arg, GenericArgument::Type(inner) if type_contains_forbidden_actor_substitute(inner))
                    }),
                    _ => false,
                })
        }
        Type::Reference(reference) => type_contains_forbidden_actor_substitute(&reference.elem),
        Type::Paren(paren) => type_contains_forbidden_actor_substitute(&paren.elem),
        Type::Group(group) => type_contains_forbidden_actor_substitute(&group.elem),
        Type::Tuple(tuple) => tuple.elems.iter().any(type_contains_forbidden_actor_substitute),
        _ => false,
    }
}

fn validate_capability_boundary_signature(
    function: &ItemFn,
    config: &CapabilityBoundaryAttr,
) -> SynResult<()> {
    validate_capability_boundary_parts(
        &function.sig.inputs,
        function.sig.output.to_token_stream().to_string(),
        &function.block,
        config,
    )
}

fn validate_capability_boundary_impl_signature(
    function: &ImplItemFn,
    config: &CapabilityBoundaryAttr,
) -> SynResult<()> {
    validate_capability_boundary_parts(
        &function.sig.inputs,
        function.sig.output.to_token_stream().to_string(),
        &function.block,
        config,
    )
}

fn validate_capability_boundary_parts(
    inputs: &syn::punctuated::Punctuated<FnArg, Token![,]>,
    output_tokens: String,
    block: &Block,
    config: &CapabilityBoundaryAttr,
) -> SynResult<()> {
    let capability_name = config.capability.value();
    let inputs_tokens = inputs.to_token_stream().to_string();
    let block_tokens = block.to_token_stream().to_string();
    if !inputs_tokens.contains("Capability")
        && !output_tokens.contains("Capability")
        && !output_tokens.contains("Authorized")
        && !block_tokens.contains(&capability_name)
        && !block_tokens.contains("issue_operation_context")
        && !block_tokens.contains("_CAPABILITY")
    {
        return Err(Error::new_spanned(
            block,
            "capability_boundary requires a capability-bearing signature or body",
        ));
    }
    Ok(())
}

fn build_ownership_lifecycle(
    item_enum: ItemEnum,
    config: OwnershipLifecycleAttr,
) -> SynResult<proc_macro2::TokenStream> {
    let ident = item_enum.ident.clone();
    let variants = item_enum
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect::<Vec<_>>();
    let initial = config.initial.value();
    let ordered = config.ordered.iter().map(LitStr::value).collect::<Vec<_>>();
    let terminals = config
        .terminals
        .iter()
        .map(LitStr::value)
        .collect::<Vec<_>>();

    if !variants.contains(&initial) {
        return Err(Error::new_spanned(
            &item_enum,
            format!("ownership_lifecycle initial variant `{initial}` is missing from the enum"),
        ));
    }
    for variant in ordered.iter().chain(terminals.iter()) {
        if !variants.contains(variant) {
            return Err(Error::new_spanned(
                &item_enum,
                format!("ownership_lifecycle variant `{variant}` is missing from the enum"),
            ));
        }
    }

    let initial_ident = syn::Ident::new(&initial, config.initial.span());
    let terminal_idents = config
        .terminals
        .iter()
        .map(|lit| syn::Ident::new(&lit.value(), lit.span()))
        .collect::<Vec<_>>();

    let allowed_arms = ordered
        .iter()
        .enumerate()
        .map(|(index, from)| {
            let from_ident = syn::Ident::new(from, config.ordered[index].span());
            let mut allowed = vec![quote!(Self::#from_ident)];
            for later in ordered.iter().skip(index + 1) {
                let later_ident = syn::Ident::new(later, proc_macro2::Span::call_site());
                allowed.push(quote!(Self::#later_ident));
            }
            for terminal in &terminal_idents {
                allowed.push(quote!(Self::#terminal));
            }
            quote! {
                Self::#from_ident => matches!(next, #(#allowed)|*)
            }
        })
        .collect::<Vec<_>>();

    let terminal_arms = terminal_idents
        .iter()
        .map(|terminal| {
            quote! {
                Self::#terminal => matches!(next, Self::#terminal)
            }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #item_enum

        impl #ident {
            pub const INITIAL: Self = Self::#initial_ident;

            #[must_use]
            pub fn is_terminal(self) -> bool {
                matches!(self, #(Self::#terminal_idents)|*)
            }

            #[must_use]
            pub fn can_transition_to(self, next: Self) -> bool {
                match self {
                    #(#allowed_arms,)*
                    #(#terminal_arms,)*
                }
            }

            pub fn assert_transition_to(self, next: Self) -> ::aura_core::OwnershipResult<()> {
                if self.can_transition_to(next) {
                    return Ok(());
                }
                Err(::aura_core::OwnershipError::TerminalRegression {
                    detail: format!(
                        "illegal lifecycle transition for {}: {:?} -> {:?}",
                        stringify!(#ident),
                        self,
                        next
                    ),
                })
            }
        }
    })
}

fn validate_semantic_owner_body(block: &Block) -> SynResult<()> {
    let has_handoff = function_contains_call(block, "handoff_to_app_workflow");
    let mut visitor = OwnerBodyValidator::default();
    visitor.visit_block(block);

    if has_handoff {
        if let (Some(await_span), Some(handoff_span)) =
            (visitor.first_await_span, visitor.first_handoff_span)
        {
            if await_span.start().line < handoff_span.start().line {
                return Err(Error::new(
                    await_span,
                    "semantic_owner awaits before canonical handoff_to_app_workflow",
                ));
            }
        }
    }

    if let Some((span, call_name)) = visitor.raw_runtime_or_effect_await {
        return Err(Error::new(
            span,
            format!("semantic_owner contains raw runtime/effects await: {call_name}"),
        ));
    }

    if let Some((span, call_name)) = visitor.best_effort_before_terminal {
        return Err(Error::new(
            span,
            format!(
                "semantic_owner awaits best-effort helper before terminal publication: {call_name}"
            ),
        ));
    }

    Ok(())
}

fn validate_best_effort_boundary_body(block: &Block) -> SynResult<()> {
    let mut visitor = BestEffortBodyValidator::default();
    visitor.visit_block(block);

    if let Some((span, call_name)) = visitor.primary_publication_call {
        return Err(Error::new(
            span,
            format!("best_effort_boundary publishes primary lifecycle directly: {call_name}"),
        ));
    }

    if let Some((span, call_name)) = visitor.raw_awaited_side_effect {
        return Err(Error::new(
            span,
            format!("best_effort_boundary awaits raw side effect directly: {call_name}"),
        ));
    }

    Ok(())
}

fn transform_best_effort_boundary(item: TokenStream) -> TokenStream {
    if let Ok(mut function) = syn::parse::<ItemFn>(item.clone()) {
        if let Err(error) = validate_best_effort_boundary_body(&function.block) {
            return error.to_compile_error().into();
        }
        function.block.stmts.insert(
            0,
            parse_quote! {
                let _: ::aura_core::BestEffortBoundaryProtocol =
                    ::aura_core::BestEffortBoundaryProtocol::POST_TERMINAL_ONLY;
            },
        );
        return quote!(#function).into();
    }
    if let Ok(mut function) = syn::parse::<ImplItemFn>(item.clone()) {
        if let Err(error) = validate_best_effort_boundary_body(&function.block) {
            return error.to_compile_error().into();
        }
        function.block.stmts.insert(
            0,
            parse_quote! {
                let _: ::aura_core::BestEffortBoundaryProtocol =
                    ::aura_core::BestEffortBoundaryProtocol::POST_TERMINAL_ONLY;
            },
        );
        return quote!(#function).into();
    }
    Error::new(
        proc_macro2::Span::call_site(),
        "#[best_effort_boundary] may only be applied to free or impl functions",
    )
    .to_compile_error()
    .into()
}

fn transform_capability_boundary(item: TokenStream, config: CapabilityBoundaryAttr) -> TokenStream {
    if config.category.value() != "capability_gated" {
        return Error::new_spanned(
            &config.category,
            "capability_boundary category must be `capability_gated`",
        )
        .to_compile_error()
        .into();
    }

    if let Ok(mut function) = syn::parse::<ItemFn>(item.clone()) {
        if let Err(error) = validate_capability_boundary_signature(&function, &config) {
            return error.to_compile_error().into();
        }
        let capability = config.capability;
        function.block.stmts.insert(
            0,
            parse_quote! {
                let _: ::aura_core::BoundaryDeclarationCategory =
                    ::aura_core::BoundaryDeclarationCategory::CapabilityGated;
            },
        );
        function.block.stmts.insert(
            1,
            parse_quote! {
                let _: &'static str = #capability;
            },
        );
        return quote!(#function).into();
    }

    if let Ok(mut function) = syn::parse::<ImplItemFn>(item.clone()) {
        if let Err(error) = validate_capability_boundary_impl_signature(&function, &config) {
            return error.to_compile_error().into();
        }
        let capability = config.capability;
        function.block.stmts.insert(
            0,
            parse_quote! {
                let _: ::aura_core::BoundaryDeclarationCategory =
                    ::aura_core::BoundaryDeclarationCategory::CapabilityGated;
            },
        );
        function.block.stmts.insert(
            1,
            parse_quote! {
                let _: &'static str = #capability;
            },
        );
        return quote!(#function).into();
    }

    Error::new(
        proc_macro2::Span::call_site(),
        "#[capability_boundary] may only be applied to free or impl functions",
    )
    .to_compile_error()
    .into()
}

fn transform_ownership_lifecycle(item: TokenStream, config: OwnershipLifecycleAttr) -> TokenStream {
    let item_enum = match syn::parse::<ItemEnum>(item.clone()) {
        Ok(item_enum) => item_enum,
        Err(_) => {
            return Error::new(
                proc_macro2::Span::call_site(),
                "#[ownership_lifecycle] may only be applied to enums",
            )
            .to_compile_error()
            .into()
        }
    };

    match build_ownership_lifecycle(item_enum, config) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn fn_arg_contains_operation_context(arg: &FnArg) -> bool {
    match arg {
        FnArg::Typed(PatType { ty, .. }) => type_contains_operation_context(ty),
        FnArg::Receiver(_) => false,
    }
}

fn type_contains_operation_context(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => type_path_contains_operation_context(type_path),
        Type::Reference(reference) => type_contains_operation_context(&reference.elem),
        Type::Paren(paren) => type_contains_operation_context(&paren.elem),
        Type::Group(group) => type_contains_operation_context(&group.elem),
        Type::Slice(slice) => type_contains_operation_context(&slice.elem),
        Type::Tuple(tuple) => tuple.elems.iter().any(type_contains_operation_context),
        _ => false,
    }
}

#[derive(Default)]
struct OwnerBodyValidator {
    first_await_span: Option<proc_macro2::Span>,
    first_handoff_span: Option<proc_macro2::Span>,
    first_terminal_publication_line: Option<usize>,
    raw_runtime_or_effect_await: Option<(proc_macro2::Span, String)>,
    best_effort_before_terminal: Option<(proc_macro2::Span, String)>,
}

impl OwnerBodyValidator {
    fn note_terminal_publication(
        &mut self,
        span: proc_macro2::Span,
        call_name: &str,
        tokens: &str,
    ) {
        if is_terminal_publication_call(call_name, tokens) {
            let line = span.start().line;
            self.first_terminal_publication_line = Some(
                self.first_terminal_publication_line
                    .map_or(line, |existing| existing.min(line)),
            );
        }
    }
}

impl<'ast> Visit<'ast> for OwnerBodyValidator {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        if node.method == "handoff_to_app_workflow" {
            self.first_handoff_span.get_or_insert(node.span());
        }

        let method_name = node.method.to_string();
        let tokens = node.to_token_stream().to_string();
        self.note_terminal_publication(node.span(), &method_name, &tokens);
        visit::visit_expr_method_call(self, node);
    }

    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(call_name) = expr_call_name(&node.func) {
            let tokens = node.to_token_stream().to_string();
            self.note_terminal_publication(node.span(), &call_name, &tokens);
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast ExprAwait) {
        self.first_await_span.get_or_insert(node.span());

        if let Some(method_call) = method_call_on_ident(&node.base, "runtime")
            .or_else(|| method_call_on_ident(&node.base, "effects"))
        {
            self.raw_runtime_or_effect_await
                .get_or_insert((node.span(), method_call.to_token_stream().to_string()));
        }

        if let Some(call_name) = awaited_call_name(&node.base) {
            if call_name.starts_with("best_effort_") {
                let await_line = node.span().start().line;
                let terminal_seen = self
                    .first_terminal_publication_line
                    .is_some_and(|line| line <= await_line);
                if !terminal_seen {
                    self.best_effort_before_terminal
                        .get_or_insert((node.span(), call_name));
                }
            }
        }

        visit::visit_expr_await(self, node);
    }
}

#[derive(Default)]
struct BestEffortBodyValidator {
    primary_publication_call: Option<(proc_macro2::Span, String)>,
    raw_awaited_side_effect: Option<(proc_macro2::Span, String)>,
}

impl<'ast> Visit<'ast> for BestEffortBodyValidator {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if let Some(call_name) = expr_call_name(&node.func) {
            if is_primary_lifecycle_publication_name(&call_name) {
                self.primary_publication_call
                    .get_or_insert((node.span(), call_name));
            }
        }
        visit::visit_expr_call(self, node);
    }

    fn visit_expr_await(&mut self, node: &'ast ExprAwait) {
        if let Some(method_call) = method_call_on_ident(&node.base, "effects") {
            let method_name = method_call.method.to_string();
            if matches!(
                method_name.as_str(),
                "send_envelope" | "join_channel" | "create_channel"
            ) {
                self.raw_awaited_side_effect
                    .get_or_insert((node.span(), method_call.to_token_stream().to_string()));
            }
        }

        if let Some(call_name) = awaited_call_name(&node.base) {
            if is_primary_lifecycle_publication_name(&call_name) {
                self.primary_publication_call
                    .get_or_insert((node.span(), call_name));
            }
        }

        visit::visit_expr_await(self, node);
    }
}

fn function_contains_call(block: &Block, call_name: &str) -> bool {
    struct CallFinder<'a> {
        call_name: &'a str,
        found: bool,
    }

    impl<'ast> Visit<'ast> for CallFinder<'_> {
        fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
            if node.method == self.call_name {
                self.found = true;
            }
            visit::visit_expr_method_call(self, node);
        }

        fn visit_expr_call(&mut self, node: &'ast ExprCall) {
            if expr_call_name(&node.func).as_deref() == Some(self.call_name) {
                self.found = true;
            }
            visit::visit_expr_call(self, node);
        }
    }

    let mut finder = CallFinder {
        call_name,
        found: false,
    };
    finder.visit_block(block);
    finder.found
}

fn method_call_on_ident<'a>(expr: &'a Expr, receiver_ident: &str) -> Option<&'a ExprMethodCall> {
    let expr = strip_expression(expr);
    let Expr::MethodCall(method_call) = expr else {
        return None;
    };
    receiver_is_ident(&method_call.receiver, receiver_ident).then_some(method_call)
}

fn receiver_is_ident(expr: &Expr, expected_ident: &str) -> bool {
    match strip_expression(expr) {
        Expr::Path(path) => path.path.is_ident(expected_ident),
        _ => false,
    }
}

fn awaited_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Call(expr_call) => expr_call_name(&expr_call.func),
        Expr::MethodCall(method_call) => Some(method_call.method.to_string()),
        _ => None,
    }
}

fn expr_call_name(expr: &Expr) -> Option<String> {
    match strip_expression(expr) {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

fn is_primary_lifecycle_publication_name(name: &str) -> bool {
    matches!(
        name,
        "publish_authoritative_operation_phase"
            | "publish_authoritative_operation_phase_with_instance"
            | "publish_authoritative_operation_failure"
            | "publish_authoritative_operation_failure_with_instance"
            | "publish_invitation_operation_status"
            | "publish_invitation_operation_failure"
            | "publish_pending_channel_accept_success"
            | "publish_failure"
    )
}

fn is_terminal_publication_call(name: &str, tokens: &str) -> bool {
    is_primary_lifecycle_publication_name(name)
        && (name.contains("failure")
            || tokens.contains("SemanticOperationPhase :: Succeeded")
            || tokens.contains("SemanticOperationPhase :: Failed")
            || tokens.contains("SemanticOperationPhase :: Cancelled")
            || tokens.contains("TerminalOutcome :: Succeeded")
            || tokens.contains("TerminalOutcome :: Failed")
            || tokens.contains("TerminalOutcome :: Cancelled"))
}

fn strip_expression(mut expr: &Expr) -> &Expr {
    loop {
        expr = match expr {
            Expr::Group(ExprGroup { expr, .. })
            | Expr::Paren(ExprParen { expr, .. })
            | Expr::Reference(ExprReference { expr, .. }) => expr,
            _ => return expr,
        };
    }
}

fn type_path_contains_operation_context(type_path: &TypePath) -> bool {
    if type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "OperationContext")
    {
        return true;
    }

    type_path
        .path
        .segments
        .iter()
        .any(|segment| match &segment.arguments {
            PathArguments::AngleBracketed(arguments) => {
                arguments.args.iter().any(|arg| match arg {
                    GenericArgument::Type(inner_ty) => type_contains_operation_context(inner_ty),
                    _ => false,
                })
            }
            _ => false,
        })
}
