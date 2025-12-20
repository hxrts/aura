//! # Signal Forwarder (ViewState → ReactiveEffects)
//!
//! When the `signals` feature is enabled, Aura's `ViewState` uses
//! `futures-signals` (`Mutable<T>`) as the source of truth for UI state.
//!
//! The terminal TUI (and other consumers) read from the unified `ReactiveEffects`
//! signal graph (`Signal<T>`). To keep these consistent, we run background tasks
//! that forward every `ViewState` update into the corresponding `Signal<T>`.

#![cfg(feature = "signals")]

use std::sync::Arc;

use futures::StreamExt;
use futures_signals::signal::SignalExt;
use tokio::task::JoinHandle;

use aura_core::effects::reactive::ReactiveEffects;
use aura_effects::ReactiveHandler;

use crate::signal_defs::{
    BLOCKS_SIGNAL, BLOCK_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL, INVITATIONS_SIGNAL,
    NEIGHBORHOOD_SIGNAL, RECOVERY_SIGNAL,
};
use crate::views::ViewState;

// Logging macro that works with or without tracing feature
macro_rules! log_warn {
    ($($arg:tt)*) => {
        #[cfg(feature = "instrumented")]
        tracing::warn!($($arg)*);
        #[cfg(not(feature = "instrumented"))]
        {
            let _ = format_args!($($arg)*);
        }
    };
}

/// Forwards all `ViewState` signals into the `ReactiveEffects` signal graph.
///
/// Dropping this value aborts the background tasks.
pub struct SignalForwarder {
    handles: Vec<JoinHandle<()>>,
}

impl SignalForwarder {
    pub fn new() -> Self {
        Self { handles: vec![] }
    }

    pub fn start_all(views: &ViewState, reactive: Arc<ReactiveHandler>) -> Self {
        let mut forwarder = Self::new();

        forwarder.forward_contacts(views, reactive.clone());
        forwarder.forward_chat(views, reactive.clone());
        forwarder.forward_recovery(views, reactive.clone());
        forwarder.forward_invitations(views, reactive.clone());
        forwarder.forward_block(views, reactive.clone());
        forwarder.forward_blocks(views, reactive.clone());
        forwarder.forward_neighborhood(views, reactive);

        forwarder
    }

    pub fn stop(&self) {
        for handle in &self.handles {
            handle.abort();
        }
    }


    fn spawn<F>(&mut self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.handles.push(tokio::spawn(fut));
    }

    fn forward_contacts(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.contacts_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*CONTACTS_SIGNAL, state).await {
                    log_warn!("Failed to forward contacts signal: {}", e);
                }
            }
        });
    }

    fn forward_chat(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.chat_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*CHAT_SIGNAL, state).await {
                    log_warn!("Failed to forward chat signal: {}", e);
                }
            }
        });
    }

    fn forward_recovery(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.recovery_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*RECOVERY_SIGNAL, state).await {
                    log_warn!("Failed to forward recovery signal: {}", e);
                }
            }
        });
    }

    fn forward_invitations(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.invitations_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*INVITATIONS_SIGNAL, state).await {
                    log_warn!("Failed to forward invitations signal: {}", e);
                }
            }
        });
    }

    fn forward_block(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.block_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*BLOCK_SIGNAL, state).await {
                    log_warn!("Failed to forward block signal: {}", e);
                }
            }
        });
    }

    fn forward_blocks(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.blocks_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*BLOCKS_SIGNAL, state).await {
                    log_warn!("Failed to forward blocks signal: {}", e);
                }
            }
        });
    }

    fn forward_neighborhood(&mut self, views: &ViewState, reactive: Arc<ReactiveHandler>) {
        let signal = views.neighborhood_signal();
        self.spawn(async move {
            let mut stream = signal.to_stream();
            while let Some(state) = stream.next().await {
                if let Err(e) = reactive.emit(&*NEIGHBORHOOD_SIGNAL, state).await {
                    log_warn!("Failed to forward neighborhood signal: {}", e);
                }
            }
        });
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
