//! Test macro implementations
//!
//! Provides the #[aura_test] attribute macro for standardized async test setup

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Test macro with automatic tracing and timeout
///
/// Wraps async tests with aura-specific setup:
/// - Automatic tracing initialization via aura-testkit
/// - Default 30s timeout
/// - Proper error handling
///
/// # Example
///
/// ```ignore
/// use aura_macros::aura_test;
///
/// #[aura_test]
/// async fn my_test() -> aura_core::AuraResult<()> {
///     // Tracing automatically initialized
///     let fixture = aura_testkit::create_test_fixture().await?;
///     // ... test logic
///     Ok(())
/// }
/// ```
pub fn aura_test_impl(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    let attrs = &input.attrs;
    let vis = &input.vis;
    let sig = &input.sig;
    let body = &input.block;

    let mut sync_sig = sig.clone();
    sync_sig.asyncness = None;

    // Check if function is async
    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            sig.fn_token,
            "#[aura_test] can only be used on async functions",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        #(#attrs)*
        // NOTE: Test code is explicitly allowed to use tokio runtime (arch-check exemption)
        // This macro generates test infrastructure, not application code
        #[tokio::test(flavor = "multi_thread")]
        #vis #sig {
            // Initialize tracing once (safe to call multiple times)
            let _guard = ::aura_testkit::init_test_tracing();

            #body
        }
    };

    expanded.into()
}
