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
//! - YES Integration with rumpsteak-aura projection
//! - NO effect handler implementations (that's aura-effects)
//! - NO runtime coordination logic (that's aura-protocol)
//! - NO handler composition (that's aura-composition)

use proc_macro::TokenStream;

mod choreography;
mod ceremony_facts;
mod domain_fact;
mod effect_handlers;
mod effect_system;
mod error_types;
mod handler_adapters;
mod test_macros;

/// Full-featured choreography! macro with complete rumpsteak-aura feature inheritance
///
/// This macro inherits ALL standard rumpsteak-aura features including:
/// - Namespace attributes: `#[namespace = "my_protocol"]`
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
/// choreography! {
///     #[namespace = "threshold_ceremony"]
///     choreography ThresholdExample {
///         roles: Coordinator, Signer[N];
///
///         Coordinator -> Signer[*]: StartRequest;
///         Signer[*] -> Coordinator: Commitment;
///     }
/// }
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
