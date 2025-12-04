//! # Reusable Components
//!
//! Declarative UI components for building screens.

mod card;
mod command_palette;
mod empty_state;
mod form_modal;
mod key_hints;
mod list;
mod message_bubble;
mod message_input;
mod modal;
mod panel;
mod scrollable;
mod status_indicator;
mod text_input;
mod text_styled;
mod textarea;
mod toast;

pub use card::{CardFooter, CardHeader, CardStyle, SimpleCard};
pub use command_palette::{CommandItem, CommandPalette, PaletteCommand};
pub use empty_state::{EmptyState, LoadingState, NoResults};
pub use form_modal::{FormField, FormFieldComponent, FormModal, FormModalState};
pub use key_hints::KeyHintsBar;
pub use list::{navigate_list, List, ListEntry, ListItem, ListNavigation};
pub use message_bubble::{CompactMessage, MessageBubble, MessageGroupHeader, SystemMessage};
pub use message_input::{MessageInput, MessageInputState};
pub use modal::{ConfirmModal, InputModal};
pub use panel::{Panel, PanelStyle};
pub use scrollable::{calculate_scroll, ScrollDirection, Scrollable};
pub use status_indicator::{ProgressDots, Status, StatusDot, StatusIndicator};
pub use text_input::TextInput;
pub use text_styled::{Badge, Divider, Heading, KeyValue, StyledText, TextStyle};
pub use textarea::{Textarea, TextareaState};
pub use toast::{StatusBar, Toast, ToastContainer, ToastLevel, ToastMessage};
