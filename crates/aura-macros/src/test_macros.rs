//! Test macro implementations
//!
//! Provides the #[aura_test] attribute macro for standardized async test setup

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Test macro with automatic tracing and timeout
///
/// Wraps tokio::test with aura-specific setup:
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
    let fn_name = &sig.ident;

    // Check if function is async
    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            &sig.fn_token,
            "#[aura_test] can only be used on async functions",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        #(#attrs)*
        #[::tokio::test]
        #vis #sig {
            // Initialize tracing once (safe to call multiple times)
            let _guard = ::aura_testkit::init_test_tracing();

            // Run test with timeout
            ::tokio::time::timeout(
                ::std::time::Duration::from_secs(30),
                async move #body
            )
            .await
            .unwrap_or_else(|_| {
                panic!("Test '{}' timed out after 30 seconds", stringify!(#fn_name))
            })
        }
    };

    expanded.into()
}
