//! Optimizations Module
//!
//! Performance optimizations for the runtime effect system including
//! caching strategies and memory allocation pooling.

pub mod allocations;
pub mod caching;

// Re-export main types
pub use allocations::{Arena, BufferPool, StringInterner};
pub use caching::{CacheKey, CachingNetworkHandler, CachingStorageHandler, EffectCache};
