//! Access Control Middleware

use super::handler::{StorageHandler, StorageOperation, StorageResult};
use super::stack::StorageMiddleware;
use aura_protocol::effects::AuraEffects;
use aura_protocol::middleware::{MiddlewareContext, MiddlewareError, MiddlewareResult};
use aura_types::AuraError;
use std::collections::{HashMap, HashSet};

pub struct AccessControlMiddleware {
    permissions: HashMap<String, HashSet<String>>, // user_id -> allowed operations
}

impl AccessControlMiddleware {
    pub fn new() -> Self {
        Self {
            permissions: HashMap::new(),
        }
    }

    pub fn grant_permission(mut self, user_id: String, operation: String) -> Self {
        self.permissions
            .entry(user_id)
            .or_insert_with(HashSet::new)
            .insert(operation);
        self
    }
}

impl Default for AccessControlMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageMiddleware for AccessControlMiddleware {
    fn process(
        &mut self,
        operation: StorageOperation,
        context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn StorageHandler,
    ) -> MiddlewareResult<StorageResult> {
        let user_id = context
            .metadata
            .get("device_id")
            .map(|s| s.as_str())
            .unwrap_or("anonymous");
        let operation_name = match &operation {
            StorageOperation::Store { .. } => "store",
            StorageOperation::Retrieve { .. } => "retrieve",
            StorageOperation::Delete { .. } => "delete",
            _ => "read",
        };

        if let Some(user_permissions) = self.permissions.get(user_id) {
            if !user_permissions.contains(operation_name) && !user_permissions.contains("*") {
                return Err(MiddlewareError::General {
                    message: format!("Access denied for operation: {}", operation_name),
                });
            }
        } else {
            return Err(MiddlewareError::General {
                message: format!("Access denied for operation: {}", operation_name),
            });
        }

        next.execute(operation, effects)
    }

    fn middleware_name(&self) -> &'static str {
        "AccessControlMiddleware"
    }
}
