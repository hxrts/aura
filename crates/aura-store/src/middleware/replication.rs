//! Replication Middleware

use super::handler::{StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareResult};

pub struct ReplicationMiddleware {
    replication_factor: u32,
    replica_nodes: Vec<String>,
}

impl ReplicationMiddleware {
    pub fn new(replication_factor: u32) -> Self {
        Self {
            replication_factor,
            replica_nodes: Vec::new(),
        }
    }

    pub fn add_replica_node(mut self, node: String) -> Self {
        self.replica_nodes.push(node);
        self
    }
}

impl Default for ReplicationMiddleware {
    fn default() -> Self {
        Self::new(3)
    }
}

impl StorageMiddleware for ReplicationMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        match operation {
            StorageOperation::Store {
                chunk_id,
                data,
                mut metadata,
            } => {
                // Store locally first
                let local_result = next.execute(
                    StorageOperation::Store {
                        chunk_id: chunk_id.clone(),
                        data: data.clone(),
                        metadata: metadata.clone(),
                    },
                    effects,
                )?;

                // Add replication metadata
                metadata.insert("replicated".to_string(), "true".to_string());
                metadata.insert(
                    "replication_factor".to_string(),
                    self.replication_factor.to_string(),
                );

                // TODO: Replicate to other nodes
                // In a real implementation, this would send the data to replica nodes

                Ok(local_result)
            }
            _ => next.execute(operation, effects),
        }
    }

    fn middleware_name(&self) -> &'static str {
        "ReplicationMiddleware"
    }
}
