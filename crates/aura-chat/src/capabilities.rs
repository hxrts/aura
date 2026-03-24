#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "chat")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChatCapability {
    #[capability("channel:create")]
    ChannelCreate,
    #[capability("message:send")]
    MessageSend,
}

pub fn evaluation_candidates_for_chat_guard() -> &'static [ChatCapability] {
    ChatCapability::declared_names()
}
