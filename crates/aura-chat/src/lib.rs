//! # Aura Chat - Secure Messaging Layer
//!
//! This crate provides secure chat functionality built on top of the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Chat implementation that integrates with:
//! - **Layer 1** (`aura-core`): Authority-first identity model, effect traits
//! - **Layer 6** (`aura-agent`): Agent runtime and effect system
//! - **Layer 5** (`aura-transport`): AMP protocol for secure messaging
//!
//! ## Architecture
//!
//! - **Authority-First Design**: Chat groups are tied to AuthorityIds, not device IDs
//! - **AMP Integration**: Uses AMP channels for secure, encrypted messaging
//! - **Effect System**: All operations go through the effect system for testability
//! - **CRDT Support**: Message history is eventually consistent via AMP's fact-based storage
//!
//! ## Key Components
//!
//! - **ChatGroup**: Manages group membership and metadata
//! - **ChatMessage**: Represents individual chat messages
//! - **ChatService**: Core service for chat operations
//! - **ChatHistory**: Message history management and retrieval
//!
//! ## Effect System Compliance
//!
//! This crate follows the **Layer 5 Feature/Protocol Implementation** pattern:
//! - All operations flow through effect traits (`StorageEffects`, `RandomEffects`, `TimeEffects`)
//! - Requires `EffectContext` for all async operations (tracing/correlation)
//! - No direct system calls or global state
//! - Deterministic testing via mock effect implementations
//!
//! ## Usage
//!
//! ```ignore
//! use aura_chat::{ChatService, ChatGroup, ChatMessage};
//! use aura_core::context::EffectContext;
//! use std::sync::Arc;
//!
//! // Create chat service with effect system
//! let chat_service = ChatService::new(Arc::new(effect_system));
//!
//! // All operations require EffectContext for tracing
//! let ctx = EffectContext::new(authority_id);
//!
//! // Create group chat
//! let group = chat_service.create_group(
//!     &ctx,
//!     "Alice & Friends",
//!     creator_authority,
//!     initial_members
//! ).await?;
//!
//! // Send message
//! let message = chat_service.send_message(
//!     &ctx,
//!     &group.id,
//!     sender_authority,
//!     "Hello everyone!".to_string()
//! ).await?;
//!
//! // Get message history
//! let history = chat_service.get_history(&ctx, &group.id, None, None).await?;
//! ```

use aura_core::AuraError;

pub mod group;
pub mod history;
pub mod service;
pub mod types;

pub use group::ChatGroup;
pub use history::ChatHistory;
pub use service::ChatService;
pub use types::*;

/// Chat-specific errors
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// Group not found
    #[error("Chat group not found: {group_id}")]
    GroupNotFound { group_id: String },

    /// User not authorized for chat operation
    #[error("Not authorized for chat operation: {reason}")]
    NotAuthorized { reason: String },

    /// Message not found
    #[error("Message not found: {message_id}")]
    MessageNotFound { message_id: String },

    /// Invalid group configuration
    #[error("Invalid group configuration: {reason}")]
    InvalidGroup { reason: String },

    /// Transport layer error
    #[error("Transport error: {error}")]
    Transport { error: String },

    /// Serialization error
    #[error("Serialization error: {error}")]
    Serialization { error: String },

    /// Core Aura error
    #[error("Core error: {0}")]
    Core(#[from] AuraError),
}

impl From<ChatError> for AuraError {
    fn from(error: ChatError) -> Self {
        match error {
            ChatError::GroupNotFound { group_id } => {
                AuraError::not_found(format!("Chat group not found: {}", group_id))
            }
            ChatError::NotAuthorized { reason } => AuraError::permission_denied(reason),
            ChatError::MessageNotFound { message_id } => {
                AuraError::not_found(format!("Message not found: {}", message_id))
            }
            ChatError::InvalidGroup { reason } => AuraError::invalid(reason),
            ChatError::Transport { error } => AuraError::network(error),
            ChatError::Serialization { error } => AuraError::serialization(error),
            ChatError::Core(aura_error) => aura_error,
        }
    }
}
