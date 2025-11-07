//! Integrity Middleware

use super::handler::{StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult};
use aura_types::AuraError;

pub struct IntegrityMiddleware {
    check_on_retrieve: bool,
}

impl IntegrityMiddleware {
    pub fn new() -> Self {
        Self {
            check_on_retrieve: true,
        }
    }

    fn calculate_checksum(&self, data: &[u8]) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }
}

impl Default for IntegrityMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for IntegrityMiddleware {
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
                let checksum = self.calculate_checksum(&data);
                metadata.insert("checksum".to_string(), checksum);

                let store_operation = StorageOperation::Store {
                    chunk_id,
                    data,
                    metadata,
                };
                next.execute(store_operation, effects)
            }

            StorageOperation::Retrieve { chunk_id: _ } => {
                let result = next.execute(operation, effects)?;

                if let StorageResult::Retrieved { data, metadata, .. } = &result {
                    if self.check_on_retrieve {
                        if let Some(stored_checksum) = metadata.get("checksum") {
                            let calculated_checksum = self.calculate_checksum(data);
                            if stored_checksum != &calculated_checksum {
                                return Err(MiddlewareError::General {
                                    message: "Checksum mismatch detected".to_string(),
                                });
                            }
                        }
                    }
                }

                Ok(result)
            }

            _ => next.execute(operation, effects),
        }
    }

    fn middleware_name(&self) -> &'static str {
        "IntegrityMiddleware"
    }
}
