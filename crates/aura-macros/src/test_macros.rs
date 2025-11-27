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
    let fn_name = &sig.ident;

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
        #[test]
        #vis #sync_sig {
            // Initialize tracing once (safe to call multiple times)
            let _guard = ::aura_testkit::init_test_tracing();

            ::aura_macros::internal::block_on_with_timeout(
                ::std::time::Duration::from_secs(30),
                async move #body,
                stringify!(#fn_name),
            )
        }
    };

    expanded.into()
}

/// Internal helpers used by generated test wrappers
#[doc(hidden)]
pub mod internal {
    use async_io::Timer;
    use futures::pin_mut;
    use futures::{future, Future};

    #[allow(dead_code)]
    pub fn block_on_with_timeout<F, T>(duration: std::time::Duration, fut: F, name: &str) -> T
    where
        F: Future<Output = T>,
    {
        async_io::block_on(async {
            let timer = Timer::after(duration);
            pin_mut!(timer);
            pin_mut!(fut);

            match future::select(fut, timer).await {
                future::Either::Left((result, _)) => result,
                future::Either::Right((_, _)) => {
                    panic!("Test '{}' timed out after {:?}", name, duration)
                }
            }
        })
    }
}
