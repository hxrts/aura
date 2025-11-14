//! Async-native test macro for Aura effect system
//!
//! This module provides the `#[aura_test]` attribute macro that automatically
//! sets up and tears down the effect system for each test, providing a clean
//! testing environment with proper async support.

use proc_macro::{TokenStream as ProcTokenStream};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{parse_quote, Error, ItemFn, Result};

/// Configuration options for the test macro
#[derive(Default, Clone)]
pub struct TestConfig {
    /// Whether to automatically initialize the effect system
    pub auto_init: bool,
    /// Whether to use a scoped container for test isolation
    pub scoped: bool,
    /// Whether to capture and assert on effect calls
    pub capture: bool,
    /// Custom timeout for the test (in seconds)
    pub timeout: Option<u64>,
    /// Whether to use deterministic time
    pub deterministic_time: bool,
}

impl TestConfig {
    /// Parse test attributes
    pub fn from_attrs(attrs: &TokenStream) -> Result<Self> {
        let mut config = Self::default();
        
        // Default configuration
        config.auto_init = true;
        config.scoped = true;
        config.deterministic_time = true;
        
        // Parse attributes if provided - for now just skip complex parsing
        let _attrs_str = attrs.to_string();
        // TODO: Implement proper TokenStream attribute parsing
        /*
        if !attrs.is_empty() {
            for attr in attrs.iter() {
                // Parse each token in the attribute list
                if let Ok(ident) = attr.parse::<syn::Ident>() {
                    match ident.to_string().as_str() {
                        "no_init" => config.auto_init = false,
                        "no_scope" => config.scoped = false,
                        "capture" => config.capture = true,
                        "no_deterministic_time" => config.deterministic_time = false,
                        _ => {} // Unknown attribute, ignore
                    }
                } else if let Ok(assign) = attr.parse::<syn::ExprAssign>() {
                    // Handle assignment expressions like "timeout = 30"
                    if let syn::Expr::Path(path) = &*assign.left {
                        if let Some(ident) = path.path.get_ident() {
                            if ident == "timeout" {
                                if let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Int(int_lit), .. }) = &*assign.right {
                                    if let Ok(timeout_value) = int_lit.base10_parse::<u64>() {
                                        config.timeout = Some(timeout_value);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        */
        
        Ok(config)
    }
}

/// Generate the test harness code
pub fn generate_test_harness(config: TestConfig, mut test_fn: ItemFn) -> Result<TokenStream> {
    // Validate the function signature
    validate_test_function(&test_fn)?;
    
    // Extract the original function name and body
    let fn_name = &test_fn.sig.ident;
    let fn_body = &test_fn.block;
    let fn_attrs = &test_fn.attrs;
    
    // Generate setup code
    let setup_code = generate_setup_code(&config);
    
    // Generate teardown code
    let teardown_code = generate_teardown_code(&config);
    
    // Generate timeout wrapper if specified
    let body_with_timeout = if let Some(timeout_secs) = config.timeout {
        quote! {
            tokio::time::timeout(
                std::time::Duration::from_secs(#timeout_secs),
                async move { #fn_body }
            )
            .await
            .map_err(|_| aura_core::AuraError::invalid("Test timeout exceeded"))?
        }
    } else {
        quote! { #fn_body }
    };
    
    // Generate the complete test function
    let output = if config.auto_init {
        quote! {
            #[::tokio::test]
            #(#fn_attrs)*
            async fn #fn_name() -> ::aura_core::AuraResult<()> {
                // Test setup
                #setup_code
                
                // Run the test body
                let _test_result = #body_with_timeout;
                
                // Test teardown
                #teardown_code
                
                _test_result
            }
        }
    } else {
        // Just add tokio::test attribute
        test_fn.attrs.push(parse_quote!(#[::tokio::test]));
        quote! { #test_fn }
    };
    
    Ok(output)
}

/// Generate setup code based on configuration
fn generate_setup_code(config: &TestConfig) -> TokenStream {
    let mut setup = TokenStream::new();
    
    // Initialize tracing for tests
    setup.extend(quote! {
        let _guard = ::aura_testkit::init_test_tracing();
    });
    
    if config.auto_init {
        // Create effect system
        setup.extend(quote! {
            let _aura_test_device_id = ::aura_core::DeviceId::new();
            let _aura_test_effects = ::aura_protocol::effects::AuraEffectSystemBuilder::new()
                .with_device_id(_aura_test_device_id)
                .with_execution_mode(::aura_protocol::ExecutionMode::Testing)
                .build_sync()?;
        });
        
        // Initialize lifecycle
        setup.extend(quote! {
            _aura_test_effects.initialize_lifecycle().await?;
        });
    }
    
    if config.scoped {
        // Create scoped container
        setup.extend(quote! {
            let _aura_test_fixture = ::aura_protocol::effects::container::TestFixture::new();
            let _aura_test_container = ::std::sync::Arc::new(_aura_test_fixture.with_mocks().await);
            let _aura_test_scope = ::aura_protocol::effects::container::ScopedContainer::new(
                _aura_test_container.clone()
            ).await;
        });
    }
    
    if config.deterministic_time {
        // Set up deterministic time
        setup.extend(quote! {
            ::aura_testkit::time::freeze_time_at_epoch();
        });
    }
    
    if config.capture {
        // Set up effect capture
        setup.extend(quote! {
            let _aura_test_capture = ::aura_testkit::effects::EffectCapture::new();
            ::aura_testkit::effects::install_capture(_aura_test_capture.clone());
        });
    }
    
    setup
}

/// Generate teardown code based on configuration
fn generate_teardown_code(config: &TestConfig) -> TokenStream {
    let mut teardown = TokenStream::new();
    
    if config.auto_init {
        // Shutdown effect system
        teardown.extend(quote! {
            let _ = _aura_test_effects.shutdown_lifecycle().await;
        });
    }
    
    if config.capture {
        // Uninstall effect capture
        teardown.extend(quote! {
            ::aura_testkit::effects::uninstall_capture();
        });
    }
    
    if config.deterministic_time {
        // Reset time
        teardown.extend(quote! {
            ::aura_testkit::time::reset_time();
        });
    }
    
    teardown
}

/// Validate that the function is suitable for use as a test
fn validate_test_function(test_fn: &ItemFn) -> Result<()> {
    // Check that it's an async function
    if test_fn.sig.asyncness.is_none() {
        return Err(Error::new(
            Span::call_site(),
            "aura_test can only be applied to async functions",
        ));
    }
    
    // Check that it takes no parameters
    if !test_fn.sig.inputs.is_empty() {
        return Err(Error::new(
            Span::call_site(),
            "aura_test functions cannot take parameters",
        ));
    }
    
    // Check return type (should be () or Result<(), _>)
    // This is a simplified check - a full implementation would be more thorough
    
    Ok(())
}

/// Generate a test that uses effect snapshots
#[allow(dead_code)]
pub fn generate_snapshot_test(config: TestConfig, mut test_fn: ItemFn) -> Result<TokenStream> {
    let mut enhanced_config = config.clone();
    enhanced_config.capture = true;
    
    // Need to clone test_fn before moving it
    let test_fn_clone = test_fn.clone();
    let base_test = generate_test_harness(enhanced_config, test_fn_clone)?;
    
    // Add snapshot assertion at the end
    let _snapshot_assertion = quote! {
        _aura_test_capture.assert_snapshot();
    };
    
    // Insert the assertion before the final result if capture is enabled
    if config.capture {
        // Find the last statement in the function body and insert assertion before it
        if let Some(_last_stmt) = test_fn.block.stmts.last_mut() {
            // Insert snapshot assertion before the final return/result
            let snapshot_check = syn::parse_quote! {
                _aura_test_capture.assert_snapshot();
            };
            test_fn.block.stmts.insert(test_fn.block.stmts.len() - 1, snapshot_check);
        }
    }
    
    Ok(base_test)
}

/// Implementation of the aura_test macro
pub fn aura_test_impl(attr: ProcTokenStream, item: ProcTokenStream) -> Result<ProcTokenStream> {
    let attr_tokens = TokenStream::from(attr);
    
    // Parse the test function
    let test_fn = syn::parse::<syn::ItemFn>(item)?;
    
    // Parse configuration from attributes
    let config = TestConfig::from_attrs(&attr_tokens)?;
    
    // Generate the test harness
    let output = generate_test_harness(config, test_fn)?;
    
    Ok(output.into())
}