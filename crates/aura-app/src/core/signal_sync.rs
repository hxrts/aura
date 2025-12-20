//! # Signal Synchronization Module
//!
//! Automatically forwards ViewState changes to ReactiveEffects signals.
//!
//! ## Architecture
//!
//! ViewState (futures-signals `Mutable<T>`) is the single source of truth.
//! ReactiveEffects signals are derived automatically from ViewState changes.
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                         AppCore                              │
//! │                                                              │
//! │   ViewState (Mutable<T>)  ──auto-forward──>  ReactiveEffects │
//! │        │                                          │          │
//! │   set_contacts()                            CONTACTS_SIGNAL  │
//! │        │                                          │          │
//! │        └──────────> SignalForwarder ──────> emit() │         │
//! │                                                              │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! The forwarder is started automatically when `init_signals()` is called on AppCore.
//! External code should update ViewState (not ReactiveEffects signals directly).
//!
//! ```rust,ignore
//! // CORRECT: Update ViewState
//! app_core.views().set_contacts(new_contacts);
//! // ReactiveEffects CONTACTS_SIGNAL is updated automatically
//!
//! // INCORRECT: Don't emit directly to domain signals
//! // app_core.emit(&*CONTACTS_SIGNAL, new_contacts); // NO!
//! ```

use std::sync::Arc;

use futures::StreamExt;
use futures_signals::signal::SignalExt;
use tokio::task::JoinHandle;

use crate::signal_defs::{
    BLOCKS_SIGNAL, BLOCK_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, RECOVERY_SIGNAL,
};
use crate::views::ViewState;
use aura_core::effects::reactive::ReactiveEffects;
use aura_effects::ReactiveHandler;

// Logging macro that works with or without tracing feature
macro_rules! log_warn {
    ($($arg:tt)*) => {
        #[cfg(feature = "instrumented")]
        tracing::warn!($($arg)*);
        #[cfg(not(feature = "instrumented"))]
        {
            // Suppress unused variable warnings in non-instrumented builds
            let _ = format_args!($($arg)*);
        }
    };
}

/// Manages the forwarding of ViewState changes to ReactiveEffects signals.
///
/// This struct holds the handles to spawned forwarding tasks. When dropped,
/// the tasks are aborted to prevent orphaned async operations.
pub struct SignalForwarder {
    /// Handles to the spawned forwarding tasks
    handles: Vec<JoinHandle<()>>,
}

impl SignalForwarder {
    /// Create a new SignalForwarder (no tasks started yet)
    pub fn new() -> Self {
        Self {
            handles: Vec::new(),
        }
    }

    /// Start forwarding all domain signals from ViewState to ReactiveEffects.
    ///
    /// This spawns a task for each domain signal that:
    /// 1. Subscribes to the ViewState's `Mutable<T>` signal
    /// 2. Forwards each value to the corresponding ReactiveEffects signal
    ///
    /// # Arguments
    ///
    /// * `views` - Reference to the ViewState containing Mutable<T> state
    /// * `reactive` - The ReactiveHandler to emit updates to
    ///
    /// # Note
    ///
    /// The forwarder takes `Arc<ViewState>` because it needs to hold a reference
    /// across async task boundaries. Callers should wrap ViewState in Arc before
    /// starting forwarding.
    pub fn start_all(views: &ViewState, reactive: Arc<ReactiveHandler>) -> Self {
        let mut forwarder = Self::new();

        // Forward contacts: ViewState.contacts -> CONTACTS_SIGNAL
        forwarder.forward_contacts(views, reactive.clone());

        // Forward chat: ViewState.chat -> CHAT_SIGNAL
        forwarder.forward_chat(views, reactive.clone());

        // Forward recovery: ViewState.recovery -> RECOVERY_SIGNAL
        forwarder.forward_recovery(views, reactive.clone());

        // Forward invitations: ViewState.invitations -> INVITATIONS_SIGNAL
        forwarder.forward_invitations(views, reactive.clone());

        // Forward block: ViewState.block -> BLOCK_SIGNAL
        forwarder.forward_block(views, reactive.clone());

        // Forward blocks: ViewState.blocks -> BLOCKS_SIGNAL
        forwarder.forward_blocks(views, reactive.clone());

        // Forward neighborhood: ViewState.neighborhood -> NEIGHBORHOOD_SIGNAL
        forwarder.forward_neighborhood(views, reactive);

        forwarder
    }

    /// Forward contacts state changes
    fn forward_contacts(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.contacts_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(contacts_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*CONTACTS_SIGNAL, contacts_state).await {
                    log_warn!("Failed to forward contacts signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward chat state changes
    fn forward_chat(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.chat_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(chat_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*CHAT_SIGNAL, chat_state).await {
                    log_warn!("Failed to forward chat signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward recovery state changes
    fn forward_recovery(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.recovery_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(recovery_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*RECOVERY_SIGNAL, recovery_state).await {
                    log_warn!("Failed to forward recovery signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward invitations state changes
    fn forward_invitations(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.invitations_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(invitations_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*INVITATIONS_SIGNAL, invitations_state).await {
                    log_warn!("Failed to forward invitations signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward block state changes
    fn forward_block(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.block_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(block_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*BLOCK_SIGNAL, block_state).await {
                    log_warn!("Failed to forward block signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward blocks state changes
    fn forward_blocks(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.blocks_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(blocks_state) = stream.next().await {
                if let Err(e) = reactive.emit(&*BLOCKS_SIGNAL, blocks_state).await {
                    log_warn!("Failed to forward blocks signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Forward neighborhood state changes
    fn forward_neighborhood(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.neighborhood_signal();
        let handle = tokio::spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(neighborhood_state) = stream.next().await {
                if let Err(e) = reactive
                    .emit(&*NEIGHBORHOOD_SIGNAL, neighborhood_state)
                    .await
                {
                    log_warn!("Failed to forward neighborhood signal: {}", e);
                }
            }
        });
        self.handles.push(handle);
    }

    /// Stop all forwarding tasks
    pub fn stop(&self) {
        for handle in &self.handles {
            handle.abort();
        }
    }

    /// Check if all forwarding tasks are still running
    pub fn is_running(&self) -> bool {
        self.handles.iter().all(|h| !h.is_finished())
    }
}

impl Default for SignalForwarder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SignalForwarder {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect() for cleaner error handling
mod tests {
    use super::*;
    use crate::views::ContactsState;
    use std::time::Duration;

    #[tokio::test]
    async fn test_signal_forwarder_creation() {
        let forwarder = SignalForwarder::new();
        assert!(forwarder.handles.is_empty());
    }

    #[tokio::test]
    async fn test_contacts_forwarding() {
        use aura_core::effects::reactive::ReactiveEffects;

        // Create ViewState and ReactiveHandler
        let views = ViewState::default();
        let reactive = Arc::new(ReactiveHandler::new());

        // Register the signal first
        reactive
            .register(&*CONTACTS_SIGNAL, ContactsState::default())
            .await
            .expect("Failed to register signal");

        // Start forwarding
        let _forwarder = SignalForwarder::start_all(&views, reactive.clone());

        // Give the forwarder a moment to start
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Update ViewState
        let mut contacts = ContactsState::default();
        contacts.contacts.push(crate::views::contacts::Contact {
            id: aura_core::identifiers::AuthorityId::new_from_entropy([1u8; 32]),
            nickname: "Alice".to_string(),
            suggested_name: Some("Alice".to_string()),
            is_guardian: false,
            is_resident: false,
            last_interaction: None,
            is_online: false,
        });
        views.set_contacts(contacts.clone());

        // Wait for forwarding to propagate
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Read from ReactiveEffects signal - should have the update
        let result = reactive.read(&*CONTACTS_SIGNAL).await;
        assert!(result.is_ok());
        let forwarded_contacts = result.unwrap();
        assert_eq!(forwarded_contacts.contacts.len(), 1);
        assert_eq!(forwarded_contacts.contacts[0].nickname, "Alice");
    }
}
