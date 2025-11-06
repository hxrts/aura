//! Caching Middleware

use super::stack::StorageMiddleware;
use super::handler::{StorageHandler, StorageOperation, StorageResult};
use aura_protocol::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::{HashMap, VecDeque};

pub struct CachingMiddleware {
    cache: HashMap<String, Vec<u8>>,
    access_order: VecDeque<String>,
    max_cache_size: usize,
}

impl CachingMiddleware {
    pub fn new(max_cache_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: VecDeque::new(),
            max_cache_size,
        }
    }
}

impl Default for CachingMiddleware {
    fn default() -> Self {
        Self::new(1024 * 1024) // 1MB cache
    }
}

impl StorageMiddleware for CachingMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match &operation {
            StorageOperation::Retrieve { chunk_id } => {
                if let Some(cached_data) = self.cache.get(chunk_id) {
                    Ok(StorageResult::Retrieved {
                        chunk_id: chunk_id.clone(),
                        data: cached_data.clone(),
                        metadata: HashMap::new(),
                    })
                } else {
                    let result = next.execute(operation, effects)?;
                    if let StorageResult::Retrieved { ref data, chunk_id: ref retrieved_chunk_id, .. } = result {
                        self.cache.insert(retrieved_chunk_id.clone(), data.clone());
                        self.access_order.push_back(retrieved_chunk_id.clone());
                        
                        // Simple LRU eviction
                        while self.cache.len() > self.max_cache_size {
                            if let Some(old_key) = self.access_order.pop_front() {
                                self.cache.remove(&old_key);
                            }
                        }
                    }
                    Ok(result)
                }
            }
            _ => next.execute(operation, effects),
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "CachingMiddleware"
    }
}