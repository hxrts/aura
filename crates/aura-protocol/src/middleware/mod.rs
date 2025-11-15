//! Middleware Architecture - REMOVED
//!
//! **MIGRATION COMPLETE**: All middleware patterns have been removed from Aura
//! in favor of the unified effect system and explicit dependency injection.
//!
//! ## Migration Summary
//!
//! The middleware system has been completely replaced with:
//!
//! - **Layer 3** (`aura-effects`): Stateless, single-party, context-free handlers
//! - **Layer 4** (`aura-protocol`): Multi-party coordination patterns
//! - **Explicit Dependencies**: Direct effect trait injection instead of wrapper chains
//!
//! ## Migration Path
//!
//! Old middleware patterns have been migrated as follows:
//!
//! - **Circuit breakers** → `ReliabilityCoordinator` in `aura-protocol/effects/reliability`
//! - **Context propagation** → Explicit context utilities in `aura-protocol/effects/propagation`
//! - **Tracing/metrics** → Direct context fields in `AuraContext`
//! - **Error handling** → Direct effect trait error types
//! - **Retry logic** → Coordination patterns in `ReliabilityEffects`
//!
//! ## Architecture Benefits
//!
//! - **Zero wrapper overhead**: Direct function calls replace middleware chains
//! - **Compile-time safety**: Effect trait bounds replace runtime middleware dispatch
//! - **Clear layer separation**: Layer 3 vs Layer 4 follows architectural principles
//! - **Explicit dependencies**: No hidden middleware registration or ambient state
//!
//! This file is preserved only for documentation of the migration process.
//! All functional code has been moved to appropriate layer locations.