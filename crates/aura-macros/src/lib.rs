//! Derive macros for Aura middleware patterns
//! 
//! This crate provides procedural macros that eliminate boilerplate code
//! in the Aura middleware architecture:
//! 
//! - `#[derive(AuraHandler)]` - Generates middleware-compatible handler implementations
//! - `#[derive(AuraConfig)]` - Generates configuration validation, loading, and merging
//! - `#[derive(CrdtState)]` - Generates CRDT state synchronization patterns
//! - `#[derive(AuraMiddleware)]` - Generates middleware wrapper boilerplate

use proc_macro::TokenStream;

mod handler;
mod config;
mod crdt;
mod middleware;

/// Generate a middleware-compatible handler implementation
/// 
/// # Example
/// 
/// ```rust
/// use aura_macros::AuraHandler;
/// 
/// #[derive(AuraHandler)]
/// #[handler(async_trait, error = "AuraError")]
/// struct StorageHandler {
///     #[handler(state)]
///     storage: Arc<RwLock<StorageState>>,
///     #[handler(config)]
///     config: StorageConfig,
/// }
/// ```
#[proc_macro_derive(AuraHandler, attributes(handler))]
pub fn derive_aura_handler(input: TokenStream) -> TokenStream {
    handler::derive_aura_handler_impl(input)
}

/// Generate configuration validation, loading, and merging logic
/// 
/// # Example
/// 
/// ```rust
/// use aura_macros::AuraConfig;
/// 
/// #[derive(AuraConfig)]
/// #[config(validate, merge, defaults, file_format = "toml")]
/// struct StorageConfig {
///     #[config(required, range(1..=100))]
///     max_connections: u32,
///     #[config(default = "30s", validate = "timeout_range")]
///     timeout: Duration,
/// }
/// ```
#[proc_macro_derive(AuraConfig, attributes(config))]
pub fn derive_aura_config(input: TokenStream) -> TokenStream {
    config::derive_aura_config_impl(input)
}

/// Generate CRDT state synchronization patterns
/// 
/// # Example
/// 
/// ```rust
/// use aura_macros::CrdtState;
/// 
/// #[derive(CrdtState)]
/// #[crdt(automerge, conflict_resolution = "last_write_wins")]
/// struct ComponentState {
///     #[crdt(counter)]
///     epoch: u64,
///     #[crdt(set)]
///     devices: BTreeSet<DeviceId>,
///     #[crdt(map)]
///     metadata: BTreeMap<String, String>,
/// }
/// ```
#[proc_macro_derive(CrdtState, attributes(crdt))]
pub fn derive_crdt_state(input: TokenStream) -> TokenStream {
    crdt::derive_crdt_state_impl(input)
}

/// Generate middleware wrapper boilerplate
/// 
/// # Example
/// 
/// ```rust
/// use aura_macros::AuraMiddleware;
/// 
/// #[derive(AuraMiddleware)]
/// #[middleware(handler = "StorageHandler", config = "ObservabilityConfig")]
/// struct ObservabilityMiddleware<H> {
///     inner: H,
///     config: ObservabilityConfig,
/// }
/// ```
#[proc_macro_derive(AuraMiddleware, attributes(middleware))]
pub fn derive_aura_middleware(input: TokenStream) -> TokenStream {
    middleware::derive_aura_middleware_impl(input)
}