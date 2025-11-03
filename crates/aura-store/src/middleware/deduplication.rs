//! Deduplication Middleware
//!
//! Prevents storing duplicate data by using content-based addressing.

use super::stack::StorageMiddleware;
use super::handler::{StorageHandler, StorageOperation, StorageResult};
use aura_types::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::{HashMap, HashSet};

/// Deduplication middleware that prevents storing duplicate content
pub struct DeduplicationMiddleware {
    content_hashes: HashMap<String, String>, // content_hash -> chunk_id
    chunk_refs: HashMap<String, HashSet<String>>, // chunk_id -> set of content_hashes
}

impl DeduplicationMiddleware {
    pub fn new() -> Self {
        Self {
            content_hashes: HashMap::new(),
            chunk_refs: HashMap::new(),
        }
    }
    
    fn calculate_content_hash(&self, data: &[u8]) -> String {
        // Simple hash calculation (in production, use SHA-256 or Blake3)
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

impl Default for DeduplicationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for DeduplicationMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match operation {
            StorageOperation::Store { chunk_id, data, mut metadata } => {
                let content_hash = self.calculate_content_hash(&data);
                
                // Check if we already have this content
                if let Some(existing_chunk_id) = self.content_hashes.get(&content_hash) {
                    // Content already exists, create a reference instead of storing
                    self.chunk_refs.entry(existing_chunk_id.clone())
                        .or_insert_with(HashSet::new)
                        .insert(chunk_id.clone());
                    
                    metadata.insert("deduplicated".to_string(), "true".to_string());
                    metadata.insert("content_hash".to_string(), content_hash);
                    metadata.insert("original_chunk_id".to_string(), existing_chunk_id.clone());
                    
                    Ok(StorageResult::Stored {
                        chunk_id,
                        size: data.len(),
                    })
                } else {
                    // New content, store it and record the hash
                    self.content_hashes.insert(content_hash.clone(), chunk_id.clone());
                    metadata.insert("content_hash".to_string(), content_hash);
                    
                    let store_operation = StorageOperation::Store { chunk_id, data, metadata };
                    next.execute(store_operation, effects)
                }
            }
            
            _ => next.execute(operation, effects),
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "DeduplicationMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("unique_content_hashes".to_string(), self.content_hashes.len().to_string());
        info.insert("total_chunk_refs".to_string(), self.chunk_refs.len().to_string());
        info
    }
}