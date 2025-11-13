#![allow(clippy::type_complexity)]

//! Aura Choreography Proc Macros
//!
//! This crate provides the `choreography!` macro that enhances
//! rumpsteak-aura choreographies with Aura-specific guard chain and
//! journal coupling integration through a typed extension system.
//!
//! The macro is built as a composable layer on top of rumpsteak-aura's proven
//! choreographic infrastructure, focusing on Aura-specific concerns
//! like capability guards, flow management, and journal integration.
//!
//! # Example
//!
//! ```ignore
//! use aura_macros::choreography;
//!
//! choreography! {
//!     #[namespace = "threshold_ceremony"]
//!     protocol ThresholdCeremony {
//!         roles: Coordinator, Signers;
//!
//!         Coordinator[guard_capability = "coordinate_signing",
//!                    flow_cost = 200,
//!                    journal_facts = "threshold_initiated"]
//!         -> Signers: SignRequest;
//!
//!         Signers[guard_capability = "participate_signing",
//!                flow_cost = 150]
//!         -> Coordinator: NonceCommit;
//!     }
//! }
//! ```

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::Result;

mod extensions;
mod parsing;
mod rumpsteak_wrapper;

use rumpsteak_wrapper::RumpsteakAuraWrapper;

/// Aura choreography macro that enhances rumpsteak-aura with Aura-specific capabilities
///
/// This is Aura's primary choreography macro that extends rumpsteak-aura's choreographic
/// programming with capability guards, flow cost management, and journal coupling through
/// a typed extension system.
///
/// # Supported Annotations
///
/// - `guard_capability = "capability_name"` - Required capability for the operation
/// - `flow_cost = <number>` - Flow cost for the communication
/// - `journal_facts = "description"` - Journal facts to add
/// - `journal_merge = true` - Enable journal merge operation
///
/// # Generated Code
///
/// The macro generates:
/// - Standard rumpsteak-aura session types and role definitions
/// - Typed extension effects for Aura-specific functionality
/// - Guard profiles for each annotated message type
/// - Role-specific execution functions with guard integration
/// - Journal coupling code for fact management
/// - Helper functions for AuraHandler setup
///
/// # Example
///
/// ```ignore
/// use aura_macros::choreography;
///
/// choreography! {
///     #[namespace = "threshold_ceremony"]
///     protocol ThresholdCeremony {
///         roles: Coordinator, Signers;
///
///         Coordinator[guard_capability = "coordinate_signing",
///                    flow_cost = 200,
///                    journal_facts = "threshold_initiated"]
///         -> Signers: SignRequest;
///     }
/// }
/// ```
#[proc_macro]
pub fn choreography(input: TokenStream) -> TokenStream {
    match aura_choreography_impl(input) {
        Ok(output) => output,
        Err(err) => err.to_compile_error().into(),
    }
}

/// Legacy alias for the choreography macro
///
/// This alias maintains backward compatibility for existing code that uses
/// the `choreography!` name. New code should prefer `choreography!`.
#[proc_macro]
pub fn aura_choreography(input: TokenStream) -> TokenStream {
    choreography(input)
}

fn aura_choreography_impl(input: TokenStream) -> Result<TokenStream> {
    let protocol_input = TokenStream2::from(input);

    // Use the new rumpsteak-aura direct wrapper approach
    // This leverages rumpsteak-aura's parser directly while adding Aura extensions
    let wrapper = RumpsteakAuraWrapper::new(protocol_input)?;
    let output = wrapper.generate()?;

    Ok(output.into())
}

#[cfg(test)]
mod tests {
    use crate::rumpsteak_wrapper::RumpsteakAuraWrapper;
    use quote::quote;

    #[test]
    fn test_wrapper_creation_success() {
        let input = quote! {
            #[namespace = "test"]
            choreography TestProtocol {
                roles: Alice, Bob;
                Alice -> Bob: Message;
            }
        };

        let wrapper = RumpsteakAuraWrapper::new(input);
        assert!(wrapper.is_ok(), "Wrapper should be created successfully");
    }

    #[test]
    fn test_aura_extension_parsing() {
        let input = quote! {
            choreography TestMacro {
                roles: Alice, Bob;
                Alice[guard_capability = "send", flow_cost = 50] -> Bob: TestMessage;
            }
        };

        let wrapper = RumpsteakAuraWrapper::new(input);
        assert!(wrapper.is_ok(), "Wrapper should parse Aura annotations");
    }

    #[test]
    fn test_journal_annotation_parsing() {
        let input = quote! {
            choreography JournalTest {
                roles: Client, Server;
                Client[journal_facts = "request_logged"] -> Server: Request;
            }
        };

        let wrapper = RumpsteakAuraWrapper::new(input);
        assert!(
            wrapper.is_ok(),
            "Wrapper should parse journal_facts annotation"
        );
    }

    #[test]
    fn test_parsing_integration() {
        // Test that the wrapper can be created and generate code
        let input = quote! {
            choreography Simple {
                roles: A, B;
                A -> B: Msg;
            }
        };

        let wrapper = RumpsteakAuraWrapper::new(input);
        assert!(wrapper.is_ok(), "Wrapper creation should succeed");

        // Test code generation (without executing proc-macro)
        let wrapper = wrapper.unwrap();
        let result = wrapper.generate();
        assert!(result.is_ok(), "Code generation should succeed");
    }

    #[test]
    fn test_multiple_annotations() {
        let input = quote! {
            choreography MultiAnnotation {
                roles: Sender, Receiver;
                Sender[guard_capability = "send", flow_cost = 100, journal_facts = "sent"]
                -> Receiver: ComplexMessage;
            }
        };

        let wrapper = RumpsteakAuraWrapper::new(input);
        assert!(
            wrapper.is_ok(),
            "Wrapper should handle multiple annotations"
        );
    }
}
