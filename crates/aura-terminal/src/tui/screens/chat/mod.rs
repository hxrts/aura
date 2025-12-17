//! # Chat Screen Module
//!
//! Main chat interface with channel list, messages, and input.

mod channel_info_modal;
mod chat_create_modal;
mod screen;
mod topic_modal;

// Screen exports
pub use screen::{run_chat_screen, ChatFocus, ChatScreen};

// Modal exports
pub use channel_info_modal::ChannelInfoModal;
pub use chat_create_modal::{ChatCreateModal, ChatCreateState, CreateChatCallback};
