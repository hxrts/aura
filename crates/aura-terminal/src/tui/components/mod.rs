//! # Reusable Components
//!
//! Declarative UI components for building screens.
//!
//! Screen-specific modals have been moved to their respective screen directories:
//! - `screens/chat/` - ChannelInfoModal, ChatCreateModal
//! - `screens/invitations/` - InvitationCodeModal, InvitationCreateModal, InvitationImportModal
//! - `screens/recovery/` - GuardianSetupModal, ThresholdModal

mod account_setup_modal_template;
mod code_display_modal;
mod command_palette;
mod contact_multi_select;
mod contact_select_modal_template;
mod device_select_modal;
#[cfg(feature = "development")]
mod demo_hint;
mod detail_panel;
mod discovered_peers;
mod empty_state;
mod footer;
mod form_modal_template;
mod help_data;
mod help_modal;
mod list;
mod list_panel;
mod message_bubble;
mod message_input;
mod message_panel;
mod modal;
mod modal_primitives;
mod nav_bar;
mod panel;
mod scrollable;
mod selectable_item;
mod status_indicator;
mod tab_bar;
mod text_input;
mod text_input_modal_template;
mod text_styled;
mod textarea;
mod threshold_selector;
mod toast;

pub use account_setup_modal_template::{AccountSetupModal, AccountSetupState};
pub use code_display_modal::{copy_to_clipboard, CodeDisplayModal, CodeDisplayStatus};
pub use command_palette::{CommandItem, CommandPalette, PaletteCommand};
pub use contact_multi_select::{
    contact_multi_select, ContactMultiSelectItem, ContactMultiSelectProps,
};
pub use contact_select_modal_template::{ContactSelectModal, ContactSelectState};
pub use device_select_modal::DeviceSelectModal;
#[cfg(feature = "development")]
pub use demo_hint::{DemoHintBar, DemoInviteCodes};
pub use detail_panel::DetailPanel;
pub use discovered_peers::{
    DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState, InvitePeerCallback,
    PeerInvitationStatus,
};
pub use empty_state::{EmptyState, LoadingState, NoResults};
pub use footer::{EmptyFooter, Footer, FooterProps};
pub use form_modal_template::{FormField, FormFieldComponent, FormModal, FormModalState};
pub use help_data::{get_help_commands, get_help_commands_for_screen, HelpCommand};
pub use help_modal::{HelpModal, HelpModalState};
pub use list::{navigate_list, List, ListEntry, ListItem, ListNavigation};
pub use list_panel::ListPanel;
pub use message_bubble::{CompactMessage, MessageBubble, MessageGroupHeader, SystemMessage};
pub use message_input::{MessageInput, MessageInputState};
pub use message_panel::MessagePanel;
pub use modal::{ConfirmModal, InputModal, ModalContent, ModalFrame};
pub use modal_primitives::{
    key_hint_group, labeled_input, modal_footer, modal_header, multi_select_list, status_message,
    LabeledInputProps, ModalFooterProps, ModalHeaderProps, ModalStatus, MultiSelectListProps,
    SelectableItem,
};
pub use nav_bar::{NavBar, NavBarProps};
pub use panel::{Panel, PanelStyle};
pub use scrollable::{calculate_scroll, ScrollDirection, Scrollable};
pub use selectable_item::SimpleSelectableItem;
pub use status_indicator::{
    DeliveryStatusIndicator, ProgressDots, Status, StatusDot, StatusIndicator, SyncIndicatorStatus,
    SyncStatusIndicator,
};
pub use tab_bar::{TabBar, TabBarProps, TabItem};
pub use text_input::TextInput;
pub use text_input_modal_template::{TextInputModal, TextInputState};
pub use text_styled::{Badge, Divider, Heading, KeyValue, StyledText, TextStyle};
pub use textarea::{Textarea, TextareaState};
pub use threshold_selector::{threshold_selector, ThresholdSelectorProps};
pub use toast::{
    StatusBar, Toast, ToastContainer, ToastContent, ToastFrame, ToastLevel, ToastMessage,
};
