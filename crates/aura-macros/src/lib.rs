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
