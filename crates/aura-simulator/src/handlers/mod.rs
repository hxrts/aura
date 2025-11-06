//! Simulator handler implementations

pub mod core;

pub use core::CoreSimulatorHandler;

use crate::middleware::{SimulatorContext, SimulatorHandler, SimulatorOperation};
use serde_json::{json, Value};

/// No-op handler for testing
pub struct NoOpSimulatorHandler;

impl SimulatorHandler for NoOpSimulatorHandler {
    fn handle(
        &self,
        _operation: SimulatorOperation,
        context: &SimulatorContext,
    ) -> crate::middleware::Result<Value> {
        Ok(json!({
            "handler": "noop",
            "scenario_id": context.scenario_id,
            "timestamp": context.timestamp.as_millis(),
            "status": "handled"
        }))
    }

    fn name(&self) -> &str {
        "noop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_noop_handler() {
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = handler.handle(
            SimulatorOperation::ExecuteTick {
                tick_number: 1,
                delta_time: Duration::from_millis(100),
            },
            &context,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert_eq!(value["handler"], "noop");
        assert_eq!(value["status"], "handled");
    }

    #[test]
    fn test_handler_name() {
        let noop_handler = NoOpSimulatorHandler;
        assert_eq!(noop_handler.name(), "noop");
    }
}
