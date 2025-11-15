//! Aura Macros - Proc Macro Crate
//!
//! This crate provides choreography and effect handler macros for the Aura project.
//! 
//! Following the external-demo pattern, our choreography! macro inherits ALL 
//! rumpsteak-aura features automatically while providing a foundation for
//! future Aura-specific enhancements.

use proc_macro::TokenStream;

mod choreography;
mod effect_handlers;
mod handler_adapters;
mod effect_system;
mod error_types;

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
///     trait_name: RandomEffects,
///     mock: {
///         struct_name: MockRandomHandler,
///         state: {
///             seed: u64,
///         },
///         methods: {
///             random_bytes(len: usize) -> Vec<u8> => {
///                 vec![0; len] // deterministic for testing
///             },
///         },
///     },
///     real: {
///         struct_name: RealRandomHandler,
///         methods: {
///             random_bytes(len: usize) -> Vec<u8> => {
///                 let mut bytes = vec![0u8; len];
///                 rand::thread_rng().fill_bytes(&mut bytes);
///                 bytes
///             },
///         },
///     },
/// }
/// ```
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
