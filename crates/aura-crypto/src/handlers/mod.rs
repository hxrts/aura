//! Crypto operation handlers

/// No-op handler for testing
pub struct NoOpHandler;

impl crate::middleware::CryptoHandler for NoOpHandler {
    fn handle(
        &self,
        operation: crate::middleware::CryptoOperation,
        _context: &crate::middleware::CryptoContext,
    ) -> crate::Result<serde_json::Value> {
        Ok(serde_json::json!({
            "operation": format!("{:?}", operation),
            "handler": "no_op",
            "success": true
        }))
    }
}
