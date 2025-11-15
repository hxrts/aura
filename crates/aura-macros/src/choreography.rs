//! Full-featured choreography! macro implementation
//!
//! This module provides the choreography! macro that inherits ALL rumpsteak-aura
//! features while providing a foundation for future Aura-specific extensions.
//! 
//! Following the external-demo pattern, we use an empty extension registry to
//! avoid buggy extensions while supporting all standard rumpsteak-aura features.

use proc_macro2::TokenStream;
use rumpsteak_aura_choreography::{
    extensions::ExtensionRegistry,
    parse_and_generate_with_extensions,
};

// Note: AST extraction system moved to aura-mpst to avoid circular dependency

/// Implementation of the Aura choreography! macro
/// 
/// Inherits ALL rumpsteak-aura features including:
/// - Choice constructs  
/// - Loop constructs
/// - Parameterized roles with [N] and [*] syntax
/// - Protocol composition
/// - Session type safety
/// - Choreographic projection
/// 
/// Uses the external-demo pattern: empty extension registry for full feature inheritance
pub fn choreography_impl(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Standard rumpsteak-aura with empty registry (external-demo pattern)
    choreography_impl_standard(input)
}

/// Standard rumpsteak-aura implementation with empty extension registry
/// 
/// This follows the external-demo pattern exactly and provides full
/// rumpsteak-aura feature inheritance without extension conflicts.
pub fn choreography_impl_standard(input: TokenStream) -> Result<TokenStream, syn::Error> {
    // Convert token stream to string for parsing
    let input_str = input.to_string();
    
    // Create empty extension registry to avoid buggy timeout extension
    // This follows the external-demo pattern and ensures we inherit ALL
    // standard rumpsteak-aura features without extension conflicts
    let registry = ExtensionRegistry::new();
    
    // Parse and generate code with full rumpsteak-aura feature inheritance
    match parse_and_generate_with_extensions(&input_str, &registry) {
        Ok(tokens) => Ok(tokens),
        Err(err) => {
            let error_msg = err.to_string();
            Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("Choreography compilation error: {}", error_msg),
            ))
        }
    }
}
