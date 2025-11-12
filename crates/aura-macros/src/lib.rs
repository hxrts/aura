//! Aura Choreography Proc Macros
//!
//! This crate provides the `aura_choreography!` macro that enhances
//! rumpsteak-aura choreographies with Aura-specific guard chain and
//! journal coupling integration.
//!
//! The macro is built as a wrapper around rumpsteak-aura's proven
//! `choreography!` infrastructure, focusing on Aura-specific concerns
//! like capability guards, flow management, and journal integration.
//!
//! # Example
//!
//! ```ignore
//! use aura_macros::aura_choreography;
//!
//! aura_choreography! {
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

mod annotations;
mod codegen;
mod parsing;
mod wrapper;

use wrapper::AuraChoreographyWrapper;

/// Aura choreography macro that wraps rumpsteak-aura with Aura-specific enhancements
///
/// This macro takes a choreography specification with Aura-specific annotations
/// and generates both standard rumpsteak-aura session types and Aura integration code
/// including guard chains, flow management, and journal coupling.
///
/// # Supported Annotations
///
/// - `@guard_capability = "capability_name"` - Required capability for the operation
/// - `@flow_cost = <number>` - Flow cost for the communication
/// - `@journal_facts = "description"` - Journal facts to add
/// - `@journal_merge = true` - Enable journal merge operation
///
/// # Generated Code
///
/// The macro generates:
/// - Standard rumpsteak-aura session types and role definitions
/// - Guard profiles for each annotated message type
/// - Role-specific execution functions with guard integration
/// - Journal coupling code for fact management
/// - Helper functions for AuraHandlerAdapter setup
#[proc_macro]
pub fn aura_choreography(input: TokenStream) -> TokenStream {
    match aura_choreography_impl(input) {
        Ok(output) => output,
        Err(err) => err.to_compile_error().into(),
    }
}

fn aura_choreography_impl(input: TokenStream) -> Result<TokenStream> {
    let protocol_input = TokenStream2::from(input);

    // Use the new wrapper approach that delegates to rumpsteak-aura
    // while adding Aura-specific enhancements
    let wrapper = AuraChoreographyWrapper::new(protocol_input)?;
    let output = wrapper.generate()?;

    Ok(output.into())
}

#[cfg(test)]
mod tests {
    use crate::parsing::AuraProtocolParser;
    use crate::wrapper::AuraChoreographyWrapper;
    use quote::quote;

    #[test]
    fn test_basic_protocol_parsing() {
        let input = quote! {
            #[namespace = "test"]
            protocol TestProtocol {
                roles: Alice, Bob;

                Alice[guard_capability = "send_message",
                      flow_cost = 100]
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
    fn test_aura_choreography_wrapper() {
        let input = quote! {
            #[namespace = "test_macro"]
            protocol TestMacro {
                roles: Alice, Bob;

                Alice[guard_capability = "send",
                      flow_cost = 50]
                -> Bob: TestMessage(u32);
            }
        };

        let wrapper = AuraChoreographyWrapper::new(input);
        assert!(wrapper.is_ok(), "Wrapper should be created successfully");

        let result = wrapper.unwrap().generate();
        assert!(result.is_ok(), "Wrapper should generate valid code");
    }
}
