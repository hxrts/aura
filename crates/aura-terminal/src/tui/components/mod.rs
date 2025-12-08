//! # Reusable Components
//!
//! Declarative UI components for building screens.

mod account_setup;
mod card;
mod chat_create_modal;
mod command_palette;
mod contact_select_modal;
mod demo_hint;
mod discovered_peers;
mod empty_state;
mod form_modal;
mod invitation_code_modal;
mod invitation_create_modal;
mod invitation_import_modal;
mod key_hints;
mod list;
mod message_bubble;
mod message_input;
mod modal;
mod panel;
mod scrollable;
mod status_indicator;
mod text_input;
mod text_input_modal;
mod text_styled;
mod textarea;
mod toast;

pub use account_setup::{AccountSetupModal, AccountSetupState};
pub use card::{CardFooter, CardHeader, CardStyle, SimpleCard};
pub use chat_create_modal::{ChatCreateModal, ChatCreateState, CreateChatCallback};
pub use command_palette::{CommandItem, CommandPalette, PaletteCommand};
pub use contact_select_modal::{ContactSelectModal, ContactSelectState};
pub use demo_hint::{DemoHintBar, DemoInviteCodes};
pub use discovered_peers::{
    DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState, InvitePeerCallback,
};
pub use empty_state::{EmptyState, LoadingState, NoResults};
pub use form_modal::{FormField, FormFieldComponent, FormModal, FormModalState};
pub use invitation_code_modal::{InvitationCodeModal, InvitationCodeState};
pub use invitation_create_modal::{
    CancelCallback, CreateInvitationCallback, InvitationCreateModal, InvitationCreateState,
};
pub use invitation_import_modal::{ImportCallback, InvitationImportModal, InvitationImportState};
pub use key_hints::KeyHintsBar;
pub use list::{navigate_list, List, ListEntry, ListItem, ListNavigation};
pub use message_bubble::{CompactMessage, MessageBubble, MessageGroupHeader, SystemMessage};
pub use message_input::{MessageInput, MessageInputState};
pub use modal::{ConfirmModal, InputModal};
pub use panel::{Panel, PanelStyle};
pub use scrollable::{calculate_scroll, ScrollDirection, Scrollable};
pub use status_indicator::{ProgressDots, Status, StatusDot, StatusIndicator};
pub use text_input::TextInput;
pub use text_input_modal::{TextInputModal, TextInputState};
pub use text_styled::{Badge, Divider, Heading, KeyValue, StyledText, TextStyle};
pub use textarea::{Textarea, TextareaState};
pub use toast::{StatusBar, Toast, ToastContainer, ToastLevel, ToastMessage};
