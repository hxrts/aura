//! Reusable UI components built on Dioxus shadcn primitives.
//!
//! Provides styled buttons, cards, pills, modals, and list items that compose
//! into the Aura application screens with consistent visual appearance.

#![allow(clippy::incompatible_msrv)]

use crate::dom_ids::RequiredDomId;
use aura_app::ui::contract::{list_item_dom_id, ControlId, FieldId, ListId, ModalId};
use aura_core::types::identifiers::AuthorityId;
use dioxus::prelude::*;
use dioxus_shadcn::components::badge::{Badge as LbBadge, BadgeVariant as LbBadgeVariant};
use dioxus_shadcn::components::card::Card as LbCard;
use dioxus_shadcn::components::dialog::{
    DialogContent as LbDialogContent, DialogOverlay as LbDialogOverlay, DialogRoot as LbDialogRoot,
    DialogTitle as LbDialogTitle,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PillTone {
    Neutral,
    Info,
    Success,
}

#[derive(Clone, PartialEq)]
pub struct ModalInputView {
    pub label: String,
    pub field_id: FieldId,
    pub value: String,
}

#[derive(Clone, PartialEq)]
pub struct ModalValueView {
    pub label: String,
    pub value: String,
}

#[derive(Clone, PartialEq)]
pub struct ModalFooterActionView {
    pub control_id: Option<ControlId>,
    pub label: String,
}

#[derive(Clone, PartialEq)]
pub struct ModalView {
    pub modal_id: ModalId,
    pub title: String,
    pub details: Vec<String>,
    pub keybind_rows: Vec<(String, String)>,
    pub inputs: Vec<ModalInputView>,
    pub values: Vec<ModalValueView>,
    pub selectable_items: Vec<SelectableItem>,
    pub enter_label: String,
    /// Optional shortcut buttons shown in the footer (e.g., demo invitation codes).
    /// Each entry is (label, value) where value is filled into the first input field.
    pub footer_shortcuts: Vec<(String, String)>,
    pub footer_actions: Vec<ModalFooterActionView>,
}

#[derive(Clone, PartialEq)]
pub struct SelectableItem {
    pub index: usize,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, PartialEq)]
pub struct AuthorityPickerItem {
    pub id: AuthorityId,
    pub label: String,
    pub is_current: bool,
    pub is_selected: bool,
}

fn ui_button_class(variant: ButtonVariant) -> &'static str {
    match variant {
        ButtonVariant::Primary => {
            "inline-flex h-8 shrink-0 items-center justify-center whitespace-nowrap rounded-sm bg-primary px-3 text-sm font-medium leading-none text-primary-foreground transition-colors hover:bg-primary/90"
        }
        ButtonVariant::Secondary => {
            "inline-flex h-8 shrink-0 items-center justify-center whitespace-nowrap rounded-sm border border-border bg-background px-3 text-sm font-medium leading-none text-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
        }
    }
}

fn pill_tone_variant(tone: PillTone) -> LbBadgeVariant {
    match tone {
        PillTone::Neutral => LbBadgeVariant::Outline,
        PillTone::Info => LbBadgeVariant::Secondary,
        PillTone::Success => LbBadgeVariant::Default,
    }
}

#[component]
pub fn UiCard(
    title: String,
    subtitle: Option<String>,
    extra_class: Option<String>,
    children: Element,
) -> Element {
    let card_class = match extra_class {
        Some(extra_class) if !extra_class.is_empty() => {
            format!(
                "flex h-full min-h-0 flex-col gap-0 overflow-hidden rounded-sm py-0 {extra_class}"
            )
        }
        _ => "flex h-full min-h-0 flex-col gap-0 overflow-hidden rounded-sm py-0".to_string(),
    };

    rsx! {
        LbCard {
            class: Some(card_class),
            div {
                class: "flex h-[4.5rem] w-full shrink-0 flex-col items-start justify-start border-b-[1px] border-border px-6 py-3 text-left",
                div {
                    class: "flex w-full min-w-0 flex-col items-start text-left",
                    div {
                        class: "w-full text-left text-xs font-sans font-semibold uppercase tracking-[0.08em] truncate",
                        "{title}"
                    }
                    if let Some(subtitle) = subtitle {
                        div {
                            class: "w-full text-left truncate text-sm text-muted-foreground",
                            "{subtitle}"
                        }
                    }
                }
            }
            div {
                class: "flex flex-1 min-h-0 flex-col gap-2 px-6 py-6 text-sm",
                {children}
            }
        }
    }
}

#[component]
pub fn UiCardBody(id: Option<String>, extra_class: Option<String>, children: Element) -> Element {
    let class = match extra_class {
        Some(extra_class) if !extra_class.is_empty() => {
            format!("flex flex-1 min-h-0 min-w-0 flex-col {extra_class}")
        }
        _ => "flex flex-1 min-h-0 min-w-0 flex-col".to_string(),
    };

    rsx! {
        div {
            id,
            class: "{class}",
            {children}
        }
    }
}

#[component]
pub fn UiCardFooter(extra_class: Option<String>, children: Element) -> Element {
    let class = match extra_class {
        Some(extra_class) if !extra_class.is_empty() => {
            format!(
                "mt-auto -mx-6 -mb-6 flex h-[4.5rem] shrink-0 items-center overflow-hidden border-t-[1px] border-border bg-card px-6 py-3 {extra_class}"
            )
        }
        _ => "mt-auto -mx-6 -mb-6 flex h-[4.5rem] shrink-0 items-center overflow-hidden border-t-[1px] border-border bg-card px-6 py-3".to_string(),
    };

    rsx! {
        div {
            class: "{class}",
            {children}
        }
    }
}

#[component]
pub fn UiButton(
    id: Option<String>,
    label: String,
    variant: ButtonVariant,
    width_class: Option<String>,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let class = match width_class {
        Some(width_class) if !width_class.is_empty() => {
            format!("{} {}", ui_button_class(variant), width_class)
        }
        _ => ui_button_class(variant).to_string(),
    };

    rsx! {
        button {
            r#type: "button",
            id,
            class: "{class}",
            onclick: move |evt| onclick.call(evt),
            "{label}"
        }
    }
}

#[component]
pub fn UiListButton(
    id: Option<String>,
    label: String,
    active: bool,
    extra_class: Option<String>,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let base_class = if active {
        "inline-flex h-9 w-full items-center justify-start rounded-sm bg-accent pl-4 text-left text-sm font-medium leading-none text-foreground"
    } else {
        "inline-flex h-9 w-full items-center justify-start rounded-sm pl-4 text-left text-sm font-medium leading-none text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground"
    };
    let class = match extra_class {
        Some(extra_class) if !extra_class.is_empty() => format!("{base_class} {extra_class}"),
        _ => base_class.to_string(),
    };

    rsx! {
        button {
            r#type: "button",
            id,
            aria_pressed: Some(active),
            class: "{class}",
            onclick: move |evt| onclick.call(evt),
            "{label}"
        }
    }
}

#[component]
pub fn UiPill(label: String, tone: PillTone) -> Element {
    rsx! {
        LbBadge {
            variant: pill_tone_variant(tone),
            class: Some("h-6 rounded-full px-2 font-sans text-[0.65rem] uppercase tracking-[0.06em]".to_string()),
            "{label}"
        }
    }
}

#[component]
pub fn UiListItem(label: String, secondary: Option<String>, active: bool) -> Element {
    let class = if active {
        "aura-list-item rounded-sm bg-accent px-2.5 py-2.5 min-w-0 overflow-hidden"
    } else {
        "aura-list-item rounded-sm px-2.5 py-2.5 min-w-0 overflow-hidden transition-colors hover:bg-accent/60"
    };
    rsx! {
        div {
            class: "{class}",
            p { class: "m-0 text-sm text-foreground truncate", "{label}" }
            if let Some(secondary) = secondary {
                p { class: "m-0 mt-1 text-xs text-muted-foreground truncate", "{secondary}" }
            }
        }
    }
}

#[component]
pub fn UiModal(
    modal: ModalView,
    on_cancel: EventHandler<()>,
    on_confirm: EventHandler<()>,
    on_input_change: EventHandler<(FieldId, String)>,
    on_input_focus: EventHandler<FieldId>,
    #[props(default)] on_toggle_selection: Option<EventHandler<usize>>,
    #[props(default)] on_footer_action: Option<EventHandler<usize>>,
) -> Element {
    rsx! {
        div {
            id: modal.modal_id.web_dom_id().to_string(),
            class: "fixed inset-0 z-40 flex items-center justify-center bg-background/95 px-4 backdrop-blur-sm",
            onclick: move |_| on_cancel.call(()),
            div {
                id: "aura-modal-content",
                role: "dialog",
                aria_modal: "true",
                aria_labelledby: "aura-modal-title",
                class: "aura-modal-fade w-full max-w-xl overflow-hidden rounded-sm border border-border bg-card p-0 text-card-foreground shadow-2xl",
                onclick: move |evt| evt.stop_propagation(),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-center justify-between gap-3",
                    h2 {
                        id: "aura-modal-title",
                        class: "m-0 text-sm font-sans font-semibold text-card-foreground",
                        "{modal.title}"
                    }
                    button {
                        r#type: "button",
                        class: "inline-flex h-8 w-8 items-center justify-center rounded-sm text-lg text-muted-foreground hover:text-foreground hover:bg-muted transition-colors",
                        onclick: move |_| on_cancel.call(()),
                        aria_label: "Close",
                        "×"
                    }
                }
                div {
                    class: "bg-card px-4 pt-4 pb-6 space-y-4 text-sm text-card-foreground",
                    p {
                        id: "aura-modal-description",
                        class: "sr-only",
                        "Aura modal dialog"
                    }
                    for line in modal.details {
                        p { class: "m-0 whitespace-pre-wrap break-words", "{line}" }
                    }
                    if !modal.values.is_empty() {
                        div {
                            class: "space-y-3",
                            for value_view in modal.values.clone() {
                                div {
                                    class: "rounded-sm border border-border bg-background/70 px-3 py-3",
                                    p {
                                        class: "m-0 text-[0.7rem] uppercase tracking-[0.06em] text-muted-foreground",
                                        "{value_view.label}"
                                    }
                                    p {
                                        class: "m-0 mt-2 break-all font-mono text-sm text-foreground",
                                        "{value_view.value}"
                                    }
                                }
                            }
                        }
                    }
                    if !modal.keybind_rows.is_empty() {
                        div {
                            class: "rounded-sm border border-border bg-background/70 divide-y divide-border overflow-hidden",
                            for (keys, description) in modal.keybind_rows {
                                div {
                                    class: "flex items-center justify-between gap-3 px-3 py-2",
                                    kbd {
                                        class: "rounded-sm border border-border bg-muted px-2 py-1 font-mono text-[0.68rem] uppercase tracking-[0.08em] text-foreground whitespace-nowrap",
                                        "{keys}"
                                    }
                                    span {
                                        class: "text-xs text-muted-foreground text-right",
                                        "{description}"
                                    }
                                }
                            }
                        }
                    }
                    if !modal.inputs.is_empty() {
                        div {
                            class: "pt-2 space-y-3",
                            for input_view in modal.inputs.clone() {
                                div {
                                    class: "space-y-2",
                                    label {
                                        r#for: "{input_view.field_id.web_dom_id().or_else(|| ControlId::ModalInput.web_dom_id()).required_dom_id(\"modal input field identifier must be defined\")}",
                                        class: "text-[0.7rem] uppercase tracking-[0.06em] text-muted-foreground",
                                        "{input_view.label}"
                                    }
                                    input {
                                        id: "{input_view.field_id.web_dom_id().or_else(|| ControlId::ModalInput.web_dom_id()).required_dom_id(\"modal input field identifier must be defined\")}",
                                        class: "flex h-10 w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none ring-offset-background placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring",
                                        autocomplete: "off",
                                        value: "{input_view.value}",
                                        onfocus: {
                                            let field_id = input_view.field_id;
                                            move |_| on_input_focus.call(field_id)
                                        },
                                        oninput: {
                                            let field_id = input_view.field_id;
                                            move |evt: FormEvent| {
                                                on_input_change.call((field_id, evt.value()));
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                    if !modal.selectable_items.is_empty() {
                        div {
                            class: "pt-2",
                            div {
                                class: "max-h-48 overflow-y-auto rounded-sm border border-border bg-background/70",
                                for item in modal.selectable_items.clone() {
                                    button {
                                        r#type: "button",
                                        class: if item.selected {
                                            "flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-foreground bg-accent/60 transition-colors hover:bg-accent"
                                        } else {
                                            "flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-muted-foreground transition-colors hover:bg-accent/40 hover:text-foreground"
                                        },
                                        onclick: {
                                            let handler = on_toggle_selection;
                                            let idx = item.index;
                                            move |_| {
                                                if let Some(handler) = handler.as_ref() {
                                                    handler.call(idx);
                                                }
                                            }
                                        },
                                        span {
                                            class: "flex h-4 w-4 shrink-0 items-center justify-center rounded-sm border border-border text-xs",
                                            if item.selected { "✓" } else { "" }
                                        }
                                        span { "{item.label}" }
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    class: "bg-card px-4 pt-4 pb-3 border-t border-border flex items-center justify-end gap-2",
                        for (index, action) in modal.footer_actions.clone().into_iter().enumerate() {
                            UiButton {
                                id: action
                                    .control_id
                                    .and_then(ControlId::web_dom_id)
                                    .map(str::to_string),
                                label: action.label,
                                variant: ButtonVariant::Secondary,
                                width_class: None,
                                onclick: {
                                    let handler = on_footer_action;
                                    move |_| {
                                        if let Some(handler) = handler.as_ref() {
                                            handler.call(index);
                                        }
                                    }
                                },
                            }
                        }
                        for (shortcut_label, shortcut_value) in modal.footer_shortcuts.clone() {
                            UiButton {
                                label: shortcut_label,
                                variant: ButtonVariant::Secondary,
                                width_class: None,
                                onclick: {
                                    let value = shortcut_value;
                                    let first_field = modal.inputs.first().map(|i| i.field_id);
                                    move |_| {
                                        if let Some(field_id) = first_field {
                                            on_input_change.call((field_id, value.clone()));
                                        }
                                    }
                                },
                            }
                        }
                        UiButton {
                            id: Some(
                                ControlId::ModalCancelButton
                                    .web_dom_id()
                                    .required_dom_id("ControlId::ModalCancelButton must define a web DOM id")
                                    .to_string(),
                            ),
                            label: "Cancel".to_string(),
                            variant: ButtonVariant::Secondary,
                            width_class: None,
                            onclick: move |_| on_cancel.call(()),
                        }
                        UiButton {
                            id: Some(
                                ControlId::ModalConfirmButton
                                .web_dom_id()
                                .required_dom_id("ControlId::ModalConfirmButton must define a web DOM id")
                                    .to_string(),
                            ),
                            label: modal.enter_label.clone(),
                            variant: ButtonVariant::Primary,
                            width_class: None,
                            onclick: move |_| on_confirm.call(()),
                        }
                }
            }
        }
    }
}

#[component]
pub fn UiDeviceEnrollmentModal(
    modal_id: ModalId,
    title: String,
    enrollment_code: String,
    ceremony_id: Option<String>,
    device_name: String,
    accepted_count: u16,
    total_count: u16,
    threshold: u16,
    is_complete: bool,
    has_failed: bool,
    error_message: Option<String>,
    copied: bool,
    primary_label: String,
    on_cancel: EventHandler<()>,
    on_copy: EventHandler<()>,
    on_primary: EventHandler<()>,
) -> Element {
    let mut open = use_signal(|| Some(true));
    let status_label = if has_failed {
        "Failed"
    } else if is_complete {
        "Complete"
    } else {
        "Pending"
    };
    let status_tone = if has_failed {
        LbBadgeVariant::Destructive
    } else if is_complete {
        LbBadgeVariant::Default
    } else {
        LbBadgeVariant::Secondary
    };
    let status_text = if let Some(error_message) = error_message {
        error_message
    } else if has_failed {
        "The enrollment ceremony failed.".to_string()
    } else if is_complete {
        format!("Enrollment complete. '{device_name}' is now part of this authority.")
    } else {
        format!("Waiting for '{device_name}' to import the enrollment code on the new device.")
    };

    rsx! {
        LbDialogRoot {
            id: Some(modal_id.web_dom_id().to_string()),
            open,
            on_open_change: move |is_open| {
                open.set(Some(is_open));
                if !is_open {
                    on_cancel.call(());
                }
            },
            LbDialogOverlay {
                class: Some("bg-background/95 backdrop-blur-sm".to_string()),
            }
            LbDialogContent {
                id: Some("aura-device-enrollment-modal-content".to_string()),
                class: Some("aura-modal-fade w-full max-w-2xl rounded-sm border border-border bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-start justify-between gap-3",
                    div {
                        class: "flex-1 space-y-1",
                        LbDialogTitle {
                            id: Some("aura-device-enrollment-modal-title".to_string()),
                            class: Some("m-0 text-sm font-sans font-semibold text-card-foreground".to_string()),
                            "{title}"
                        }
                        p {
                            class: "m-0 text-xs text-muted-foreground",
                            "Out-of-band device enrollment ceremony"
                        }
                    }
                    div {
                        class: "flex items-center gap-2",
                        LbBadge {
                            variant: status_tone,
                            class: Some("h-6 rounded-full px-2 font-sans uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                            "{status_label}"
                        }
                        button {
                            r#type: "button",
                            class: "inline-flex h-8 w-8 items-center justify-center rounded-sm text-lg text-muted-foreground hover:text-foreground hover:bg-muted transition-colors",
                            onclick: move |_| {
                                open.set(Some(false));
                                on_cancel.call(());
                            },
                            aria_label: "Close",
                            "×"
                        }
                    }
                }
                div {
                    class: "bg-card px-4 pt-3 pb-5 space-y-4 text-sm text-card-foreground",
                    div {
                        class: "grid gap-2 md:grid-cols-3",
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Progress" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{accepted_count}/{total_count.max(1)} accepted" }
                        }
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Threshold" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{threshold.max(1)} required" }
                        }
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Device" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{device_name}" }
                        }
                    }
                    if let Some(ceremony_id) = ceremony_id {
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Ceremony Id" }
                            p {
                                class: "m-0 mt-1 break-all font-mono text-xs text-foreground",
                                "{ceremony_id}"
                            }
                        }
                    }
                    div {
                        class: "rounded-sm border border-border bg-background px-4 py-4",
                        p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Enrollment Code" }
                        p {
                            class: "m-0 mt-3 break-all font-mono text-sm leading-6 text-foreground",
                            "{enrollment_code}"
                        }
                    }
                    div {
                        class: "rounded-sm border border-border bg-background/70 px-3 py-3",
                        p { class: "m-0 text-sm text-foreground", "{status_text}" }
                        if copied {
                            p {
                                class: "m-0 mt-2 text-xs uppercase tracking-[0.08em] text-muted-foreground",
                                "Copied to clipboard"
                            }
                        }
                    }
                }
                div {
                    class: "bg-card px-4 pt-4 pb-3 border-t border-border flex flex-wrap items-center justify-between gap-2",
                    div {
                        class: "flex items-center gap-2 font-mono text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                        span { "c copies" }
                        if !is_complete && !has_failed {
                            span { "esc cancels" }
                        }
                    }
                    div {
                        class: "flex items-center gap-2",
                        UiButton {
                            id: Some(
                                ControlId::DeviceEnrollmentCancelButton
                                    .web_dom_id()
                                    .required_dom_id(
                                        "ControlId::DeviceEnrollmentCancelButton must define a web DOM id",
                                    )
                                    .to_string(),
                            ),
                            label: if is_complete || has_failed {
                                "Close".to_string()
                            } else {
                                "Cancel".to_string()
                            },
                            variant: ButtonVariant::Secondary,
                            width_class: Some("w-[7.5rem]".to_string()),
                            onclick: move |_| on_cancel.call(()),
                        }
                        UiButton {
                            id: Some(
                                ControlId::ModalCopyButton
                                    .web_dom_id()
                                    .required_dom_id("ControlId::ModalCopyButton must define a web DOM id")
                                    .to_string(),
                            ),
                            label: if copied {
                                "Copied".to_string()
                            } else {
                                "Copy Code".to_string()
                            },
                            variant: ButtonVariant::Secondary,
                            width_class: Some("w-[7.5rem]".to_string()),
                            onclick: move |_| on_copy.call(()),
                        }
                        UiButton {
                            id: Some(
                                ControlId::DeviceEnrollmentPrimaryButton
                                    .web_dom_id()
                                    .required_dom_id(
                                        "ControlId::DeviceEnrollmentPrimaryButton must define a web DOM id",
                                    )
                                    .to_string(),
                            ),
                            label: primary_label,
                            variant: ButtonVariant::Primary,
                            width_class: Some("w-[9rem]".to_string()),
                            onclick: move |_| on_primary.call(()),
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn UiAuthorityPickerModal(
    modal_id: ModalId,
    title: String,
    current_label: String,
    current_id: String,
    mfa_policy: String,
    authorities: Vec<AuthorityPickerItem>,
    on_cancel: EventHandler<()>,
    on_select: EventHandler<usize>,
    on_confirm: EventHandler<()>,
) -> Element {
    let mut open = use_signal(|| Some(true));
    let authority_count = authorities.len();
    let selected_authority = authorities.iter().find(|authority| authority.is_selected);
    let selected_label = selected_authority
        .map(|authority| authority.label.clone())
        .unwrap_or_else(|| "No authority selected".to_string());
    let selected_id = selected_authority.map(|authority| authority.id.to_string());

    rsx! {
        LbDialogRoot {
            id: Some(modal_id.web_dom_id().to_string()),
            open,
            on_open_change: move |is_open| {
                open.set(Some(is_open));
                if !is_open {
                    on_cancel.call(());
                }
            },
            LbDialogOverlay {
                class: Some("bg-background/95 backdrop-blur-sm".to_string()),
            }
            LbDialogContent {
                id: Some("aura-authority-picker-modal-content".to_string()),
                class: Some("aura-modal-fade w-full max-w-2xl rounded-sm border border-border bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-start justify-between gap-3",
                    div {
                        class: "flex-1 space-y-1",
                        LbDialogTitle {
                            id: Some("aura-authority-picker-modal-title".to_string()),
                            class: Some("m-0 text-sm font-sans font-semibold text-card-foreground".to_string()),
                            "{title}"
                        }
                        p {
                            class: "m-0 text-xs text-muted-foreground",
                            "Choose which local authority the web runtime should reload into."
                        }
                    }
                    div {
                        class: "flex items-center gap-2",
                        LbBadge {
                            variant: LbBadgeVariant::Secondary,
                            class: Some("h-6 rounded-full px-2 font-sans uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                            "{authority_count} available"
                        }
                        button {
                            r#type: "button",
                            class: "inline-flex h-8 w-8 items-center justify-center rounded-sm text-lg text-muted-foreground hover:text-foreground hover:bg-muted transition-colors",
                            onclick: move |_| {
                                open.set(Some(false));
                                on_cancel.call(());
                            },
                            aria_label: "Close",
                            "×"
                        }
                    }
                }
                div {
                    class: "bg-card px-4 pt-3 pb-5 space-y-4 text-sm text-card-foreground",
                    div {
                        class: "grid gap-2 md:grid-cols-3",
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Current Authority" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{current_label}" }
                            p {
                                class: "m-0 mt-1 break-all font-mono text-[0.72rem] text-muted-foreground",
                                "{current_id}"
                            }
                        }
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Selected" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{selected_label}" }
                            if let Some(selected_id) = selected_id.as_ref() {
                                p {
                                    class: "m-0 mt-1 break-all font-mono text-[0.72rem] text-muted-foreground",
                                    "{selected_id}"
                                }
                            }
                        }
                        div {
                            class: "rounded-sm border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Policy" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{mfa_policy}" }
                            p {
                                class: "m-0 mt-1 text-[0.72rem] text-muted-foreground",
                                "Multifactor requirements are authority-scoped."
                            }
                        }
                    }
                    div {
                        class: "rounded-sm border border-border bg-background/70 overflow-hidden",
                        div {
                            class: "border-b border-border px-4 py-3",
                            p {
                                class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                                "Stored Authorities"
                            }
                        }
                        div {
                            class: "max-h-[18rem] overflow-y-auto p-2 space-y-2",
                            if authorities.is_empty() {
                                div {
                                    class: "rounded-sm border border-dashed border-border px-4 py-6 text-center text-sm text-muted-foreground",
                                    "No authorities are available in this browser profile."
                                }
                            } else {
                                for (index, authority) in authorities.into_iter().enumerate() {
                                    button {
                                        r#type: "button",
                                        id: list_item_dom_id(
                                            ListId::Authorities,
                                            &authority.id.to_string(),
                                        ),
                                        class: if authority.is_selected {
                                            "flex w-full items-start justify-between rounded-sm border border-primary/40 bg-primary/10 px-3 py-3 text-left"
                                        } else {
                                            "flex w-full items-start justify-between rounded-sm border border-border bg-background px-3 py-3 text-left hover:bg-accent/50"
                                        },
                                        onclick: move |_| on_select.call(index),
                                        div {
                                            class: "min-w-0 space-y-1",
                                            p {
                                                class: "m-0 text-sm text-foreground",
                                                "{authority.label}"
                                            }
                                            p {
                                                class: "m-0 break-all font-mono text-[0.72rem] text-muted-foreground",
                                                "{authority.id}"
                                            }
                                        }
                                        div {
                                            class: "flex shrink-0 items-center gap-2 pl-3",
                                            if authority.is_current {
                                                LbBadge {
                                                    variant: LbBadgeVariant::Secondary,
                                                    class: Some("h-6 rounded-full px-2 font-sans uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                                                    "Current"
                                                }
                                            }
                                            if authority.is_selected {
                                                LbBadge {
                                                    variant: LbBadgeVariant::Outline,
                                                    class: Some("h-6 rounded-full px-2 font-sans uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                                                    "Selected"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div {
                    class: "bg-card px-4 pt-4 pb-3 border-t border-border flex flex-wrap items-center justify-between gap-2",
                    div {
                        class: "flex items-center gap-2 font-mono text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                        span { "↑↓ choose" }
                        span { "enter reload" }
                        span { "esc cancel" }
                    }
                    div {
                        class: "flex items-center gap-2",
                        UiButton {
                            id: Some(
                                ControlId::AuthorityPickerCancelButton
                                    .web_dom_id()
                                    .required_dom_id(
                                        "ControlId::AuthorityPickerCancelButton must define a web DOM id",
                                    )
                                    .to_string(),
                            ),
                            label: "Cancel".to_string(),
                            variant: ButtonVariant::Secondary,
                            width_class: None,
                            onclick: move |_| on_cancel.call(()),
                        }
                        UiButton {
                            id: Some(
                                ControlId::AuthorityPickerConfirmButton
                                    .web_dom_id()
                                    .required_dom_id(
                                        "ControlId::AuthorityPickerConfirmButton must define a web DOM id",
                                    )
                                    .to_string(),
                            ),
                            label: "Switch".to_string(),
                            variant: ButtonVariant::Primary,
                            width_class: None,
                            onclick: move |_| on_confirm.call(()),
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn UiFooter(
    left: String,
    network_status: String,
    peer_count: String,
    online_count: String,
) -> Element {
    rsx! {
        footer {
            class: "shrink-0 overflow-hidden bg-background px-4 pt-0 pb-6 text-xs tracking-[0.02em] text-muted-foreground sm:px-6",
            div {
                class: "flex h-8 min-w-0 items-center justify-between gap-3 overflow-hidden",
                span { class: "min-w-0 truncate whitespace-nowrap text-card-foreground leading-none", "{left}" }
                div {
                    class: "flex min-w-0 items-center justify-end gap-2 overflow-hidden whitespace-nowrap px-6",
                    FooterStatusItem { label: "Network", value: network_status }
                    FooterStatusItem { label: "Peers", value: peer_count }
                    FooterStatusItem { label: "Online", value: online_count }
                }
            }
        }
    }
}

#[component]
fn FooterStatusItem(label: &'static str, value: String) -> Element {
    rsx! {
        div {
            class: "inline-flex shrink-0 items-center gap-1.5 px-1",
            span {
                class: "text-[0.62rem] uppercase tracking-[0.08em] text-muted-foreground",
                "{label}"
            }
            span {
                class: "text-[0.72rem] text-foreground leading-none",
                "{value}"
            }
        }
    }
}
