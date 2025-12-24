//! # Invitation Modals
//!
//! Invitation codes are managed from the Contacts workflow (modals), not via a
//! dedicated routed screen. This module keeps the invitation-code modal UI
//! components colocated.

mod invitation_code_modal;
mod invitation_create_modal;
mod invitation_import_modal;

// Modal exports
pub use invitation_code_modal::{InvitationCodeModal, InvitationCodeState};
pub use invitation_create_modal::{
    CancelCallback, CreateInvitationCallback as ModalCreateInvitationCallback,
    InvitationCreateModal,
};
pub use invitation_import_modal::{ImportCallback, InvitationImportModal, InvitationImportState};
