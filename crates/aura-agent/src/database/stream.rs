//! Tokio-specific fact stream receiver.
//!
//! Runtime-specific wrapper that implements `FactStreamReceiver` for tokio broadcast channels.

use aura_core::{
    effects::indexed::{FactStreamReceiver, IndexedFact},
    AuraError,
};
use std::future::Future;
use std::pin::Pin;

/// Runtime-specific wrapper that implements `FactStreamReceiver` for tokio broadcast channels.
///
/// This adapter allows the Layer 3 (Implementation) to use tokio's concrete broadcast
/// receiver while maintaining compatibility with the runtime-agnostic `FactStreamReceiver`
/// trait defined in Layer 1 (Foundation).
pub struct TokioFactStreamReceiver {
    receiver: tokio::sync::broadcast::Receiver<Vec<IndexedFact>>,
}

impl TokioFactStreamReceiver {
    /// Create a new tokio fact stream receiver wrapper.
    pub fn new(receiver: tokio::sync::broadcast::Receiver<Vec<IndexedFact>>) -> Self {
        Self { receiver }
    }
}

impl FactStreamReceiver for TokioFactStreamReceiver {
    fn recv(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<IndexedFact>, AuraError>> + Send + '_>> {
        Box::pin(async move {
            self.receiver
                .recv()
                .await
                .map_err(|e| AuraError::internal(format!("Fact stream recv error: {}", e)))
        })
    }

    fn try_recv(&mut self) -> Result<Option<Vec<IndexedFact>>, AuraError> {
        use tokio::sync::broadcast::error::TryRecvError;
        match self.receiver.try_recv() {
            Ok(facts) => Ok(Some(facts)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Lagged(n)) => Err(AuraError::internal(format!(
                "Fact stream lagged by {} messages",
                n
            ))),
            Err(TryRecvError::Closed) => Err(AuraError::internal("Fact stream closed")),
        }
    }
}
