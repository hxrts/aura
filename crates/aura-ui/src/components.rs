//! Reusable UI components built on Dioxus shadcn primitives.
//!
//! Provides styled buttons, cards, pills, modals, and list items that compose
//! into the Aura application screens with consistent visual appearance.

#![allow(clippy::incompatible_msrv)]

use dioxus::prelude::*;
use dioxus_shadcn::components::badge::{Badge as LbBadge, BadgeVariant as LbBadgeVariant};
use dioxus_shadcn::components::button::{
    Button as LbButton, ButtonSize as LbButtonSize, ButtonVariant as LbButtonVariant,
};
use dioxus_shadcn::components::card::{
    Card as LbCard, CardContent as LbCardContent, CardDescription as LbCardDescription,
    CardHeader as LbCardHeader, CardTitle as LbCardTitle,
};
use dioxus_shadcn::components::dialog::{
    DialogContent as LbDialogContent, DialogDescription as LbDialogDescription,
    DialogRoot as LbDialogRoot, DialogTitle as LbDialogTitle,
};
use dioxus_shadcn::components::input::Input as LbInput;
use dioxus_shadcn::components::label::Label as LbLabel;

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
pub struct ModalView {
    pub title: String,
    pub details: Vec<String>,
    pub keybind_rows: Vec<(String, String)>,
    pub input_label: Option<String>,
    pub input_value: Option<String>,
    pub enter_label: String,
}

#[derive(Clone, PartialEq)]
pub struct AuthorityPickerItem {
    pub id: String,
    pub label: String,
    pub is_current: bool,
    pub is_selected: bool,
}

fn map_button_variant(variant: ButtonVariant) -> LbButtonVariant {
    match variant {
        ButtonVariant::Primary => LbButtonVariant::Default,
        ButtonVariant::Secondary => LbButtonVariant::Outline,
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
            format!("flex h-full min-h-0 flex-col {extra_class}")
        }
        _ => "flex h-full min-h-0 flex-col".to_string(),
    };

    rsx! {
        LbCard {
            class: Some(card_class),
            LbCardHeader {
                class: Some("gap-1 border-b border-border pb-4".to_string()),
                LbCardTitle {
                    class: Some("text-xs font-semibold uppercase tracking-[0.08em]".to_string()),
                    "{title}"
                }
                if let Some(subtitle) = subtitle {
                    LbCardDescription { "{subtitle}" }
                }
            }
            LbCardContent {
                class: Some("flex flex-1 min-h-0 flex-col gap-2 text-sm".to_string()),
                {children}
            }
        }
    }
}

#[component]
pub fn UiButton(
    label: String,
    variant: ButtonVariant,
    on_click: EventHandler<MouseEvent>,
) -> Element {
    rsx! {
        LbButton {
            variant: map_button_variant(variant),
            size: LbButtonSize::Small,
            on_click: move |evt| on_click.call(evt),
            "{label}"
        }
    }
}

#[component]
pub fn UiListButton(label: String, active: bool, on_click: EventHandler<MouseEvent>) -> Element {
    let variant = if active {
        LbButtonVariant::Secondary
    } else {
        LbButtonVariant::Ghost
    };
    let class = if active {
        "h-9 w-full justify-start rounded-md pl-4 text-left bg-accent text-foreground shadow-none"
    } else {
        "h-9 w-full justify-start rounded-md pl-4 text-left text-muted-foreground shadow-none hover:bg-accent/60 hover:text-foreground"
    };

    rsx! {
        LbButton {
            variant,
            size: LbButtonSize::Small,
            full_width: true,
            aria_pressed: Some(active),
            class: "{class}",
            on_click: move |evt| on_click.call(evt),
            "{label}"
        }
    }
}

#[component]
pub fn UiPill(label: String, tone: PillTone) -> Element {
    rsx! {
        LbBadge {
            variant: pill_tone_variant(tone),
            class: Some("h-6 rounded-full px-2 text-[0.65rem] uppercase tracking-[0.06em]".to_string()),
            "{label}"
        }
    }
}

#[component]
pub fn UiListItem(label: String, secondary: Option<String>, active: bool) -> Element {
    let class = if active {
        "rounded-lg border border-primary/40 bg-primary/10 px-2.5 py-2"
    } else {
        "rounded-lg border border-border bg-background/60 px-2.5 py-2"
    };
    rsx! {
        div {
            class: "{class}",
            p { class: "m-0 text-sm text-foreground", "{label}" }
            if let Some(secondary) = secondary {
                p { class: "m-0 mt-1 text-xs text-muted-foreground whitespace-pre-wrap break-words", "{secondary}" }
            }
        }
    }
}

#[component]
pub fn UiModal(
    modal: ModalView,
    on_cancel: EventHandler<()>,
    on_confirm: EventHandler<()>,
    on_input_change: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| Some(true));

    rsx! {
        LbDialogRoot {
            id: Some("aura-modal-root".to_string()),
            open,
            on_open_change: move |is_open| {
                open.set(Some(is_open));
                if !is_open {
                    on_cancel.call(());
                }
            },
            LbDialogContent {
                id: Some("aura-modal-content".to_string()),
                class: Some("aura-modal-fade w-full max-w-xl bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-center justify-between gap-3",
                    LbDialogTitle {
                        id: Some("aura-modal-title".to_string()),
                        class: Some("m-0 text-sm font-semibold text-card-foreground".to_string()),
                        "{modal.title}"
                    }
                    span { class: "text-[0.66rem] uppercase tracking-[0.06em] text-muted-foreground", "Esc closes" }
                }
                div {
                    class: "bg-card px-4 py-3 space-y-2 text-sm text-card-foreground",
                    LbDialogDescription {
                        id: Some("aura-modal-description".to_string()),
                        class: Some("sr-only".to_string()),
                        "Aura modal dialog"
                    }
                    for line in modal.details {
                        p { class: "m-0 whitespace-pre-wrap break-words", "{line}" }
                    }
                    if !modal.keybind_rows.is_empty() {
                        div {
                            class: "rounded-lg border border-border bg-background/70 divide-y divide-border overflow-hidden",
                            for (keys, description) in modal.keybind_rows {
                                div {
                                    class: "flex items-center justify-between gap-3 px-3 py-2",
                                    kbd {
                                        class: "rounded border border-border bg-muted px-2 py-1 text-[0.68rem] uppercase tracking-[0.08em] text-foreground whitespace-nowrap",
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
                    if let Some(input_label) = modal.input_label {
                        div {
                            class: "pt-1 space-y-1",
                            LbLabel {
                                for_id: Some("aura-modal-input".to_string()),
                                class: Some("text-[0.7rem] uppercase tracking-[0.06em] text-muted-foreground".to_string()),
                                "{input_label}"
                            }
                            LbInput {
                                id: Some("aura-modal-input".to_string()),
                                value: modal.input_value.clone().unwrap_or_default(),
                                readonly: false,
                                full_width: true,
                                class: Some("font-mono".to_string()),
                                on_input: move |evt: FormEvent| {
                                    on_input_change.call(evt.value());
                                },
                            }
                        }
                    }
                }
                div {
                    class: "bg-card px-4 py-3 border-t border-border flex items-center justify-end gap-2",
                    UiButton {
                        label: "Cancel".to_string(),
                        variant: ButtonVariant::Secondary,
                        on_click: move |_| on_cancel.call(()),
                    }
                    UiButton {
                        label: modal.enter_label.clone(),
                        variant: ButtonVariant::Primary,
                        on_click: move |_| on_confirm.call(()),
                    }
                }
            }
        }
    }
}

#[component]
pub fn UiDeviceEnrollmentModal(
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
    let status_text = if let Some(error_message) = error_message.clone() {
        error_message
    } else if has_failed {
        "The enrollment ceremony failed.".to_string()
    } else if is_complete {
        format!(
            "Enrollment complete. '{}' is now part of this authority.",
            device_name
        )
    } else {
        format!(
            "Waiting for '{}' to import the enrollment code on the new device.",
            device_name
        )
    };

    rsx! {
        LbDialogRoot {
            id: Some("aura-device-enrollment-modal-root".to_string()),
            open,
            on_open_change: move |is_open| {
                open.set(Some(is_open));
                if !is_open {
                    on_cancel.call(());
                }
            },
            LbDialogContent {
                id: Some("aura-device-enrollment-modal-content".to_string()),
                class: Some("aura-modal-fade w-full max-w-2xl bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-start justify-between gap-3",
                    div {
                        class: "space-y-1",
                        LbDialogTitle {
                            id: Some("aura-device-enrollment-modal-title".to_string()),
                            class: Some("m-0 text-sm font-semibold text-card-foreground".to_string()),
                            "{title}"
                        }
                        p {
                            class: "m-0 text-xs text-muted-foreground",
                            "Out-of-band device enrollment ceremony"
                        }
                    }
                    LbBadge {
                        variant: status_tone,
                        class: Some("h-6 rounded-full px-2 uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                        "{status_label}"
                    }
                }
                div {
                    class: "bg-card px-4 py-4 space-y-4 text-sm text-card-foreground",
                    div {
                        class: "grid gap-2 md:grid-cols-3",
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Progress" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{accepted_count}/{total_count.max(1)} accepted" }
                        }
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Threshold" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{threshold.max(1)} required" }
                        }
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Device" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{device_name}" }
                        }
                    }
                    if let Some(ceremony_id) = ceremony_id {
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Ceremony Id" }
                            p {
                                class: "m-0 mt-1 break-all font-mono text-xs text-foreground",
                                "{ceremony_id}"
                            }
                        }
                    }
                    div {
                        class: "rounded-xl border border-border bg-background px-4 py-4",
                        p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Enrollment Code" }
                        p {
                            class: "m-0 mt-3 break-all font-mono text-sm leading-6 text-foreground",
                            "{enrollment_code}"
                        }
                    }
                    div {
                        class: "rounded-lg border border-border bg-background/70 px-3 py-3",
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
                    class: "bg-card px-4 py-3 border-t border-border flex flex-wrap items-center justify-between gap-2",
                    div {
                        class: "flex items-center gap-2 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                        span { "c copies" }
                        if !is_complete && !has_failed {
                            span { "esc cancels" }
                        }
                    }
                    div {
                        class: "flex items-center gap-2",
                        UiButton {
                            label: if is_complete || has_failed {
                                "Close".to_string()
                            } else {
                                "Cancel".to_string()
                            },
                            variant: ButtonVariant::Secondary,
                            on_click: move |_| on_cancel.call(()),
                        }
                        UiButton {
                            label: if copied {
                                "Copied".to_string()
                            } else {
                                "Copy Code".to_string()
                            },
                            variant: ButtonVariant::Secondary,
                            on_click: move |_| on_copy.call(()),
                        }
                        UiButton {
                            label: primary_label.clone(),
                            variant: ButtonVariant::Primary,
                            on_click: move |_| on_primary.call(()),
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn UiAuthorityPickerModal(
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
    let selected_id = selected_authority
        .map(|authority| authority.id.clone())
        .unwrap_or_default();

    rsx! {
        LbDialogRoot {
            id: Some("aura-authority-picker-modal-root".to_string()),
            open,
            on_open_change: move |is_open| {
                open.set(Some(is_open));
                if !is_open {
                    on_cancel.call(());
                }
            },
            LbDialogContent {
                id: Some("aura-authority-picker-modal-content".to_string()),
                class: Some("aura-modal-fade w-full max-w-2xl bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "bg-card px-4 py-3 border-b border-border flex items-start justify-between gap-3",
                    div {
                        class: "space-y-1",
                        LbDialogTitle {
                            id: Some("aura-authority-picker-modal-title".to_string()),
                            class: Some("m-0 text-sm font-semibold text-card-foreground".to_string()),
                            "{title}"
                        }
                        p {
                            class: "m-0 text-xs text-muted-foreground",
                            "Choose which local authority the web runtime should reload into."
                        }
                    }
                    LbBadge {
                        variant: LbBadgeVariant::Secondary,
                        class: Some("h-6 rounded-full px-2 uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                        "{authority_count} available"
                    }
                }
                div {
                    class: "bg-card px-4 py-4 space-y-4 text-sm text-card-foreground",
                    div {
                        class: "grid gap-2 md:grid-cols-3",
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Current Authority" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{current_label}" }
                            p {
                                class: "m-0 mt-1 break-all font-mono text-[0.72rem] text-muted-foreground",
                                "{current_id}"
                            }
                        }
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Selected" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{selected_label}" }
                            if !selected_id.is_empty() {
                                p {
                                    class: "m-0 mt-1 break-all font-mono text-[0.72rem] text-muted-foreground",
                                    "{selected_id}"
                                }
                            }
                        }
                        div {
                            class: "rounded-lg border border-border bg-background/70 px-3 py-2",
                            p { class: "m-0 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground", "Policy" }
                            p { class: "m-0 mt-1 text-sm text-foreground", "{mfa_policy}" }
                            p {
                                class: "m-0 mt-1 text-[0.72rem] text-muted-foreground",
                                "Multifactor requirements are authority-scoped."
                            }
                        }
                    }
                    div {
                        class: "rounded-xl border border-border bg-background/70 overflow-hidden",
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
                                    class: "rounded-lg border border-dashed border-border px-4 py-6 text-center text-sm text-muted-foreground",
                                    "No authorities are available in this browser profile."
                                }
                            } else {
                                for (index, authority) in authorities.into_iter().enumerate() {
                                    button {
                                        r#type: "button",
                                        class: if authority.is_selected {
                                            "flex w-full items-start justify-between rounded-lg border border-primary/40 bg-primary/10 px-3 py-3 text-left"
                                        } else {
                                            "flex w-full items-start justify-between rounded-lg border border-border bg-background px-3 py-3 text-left hover:bg-accent/50"
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
                                                    class: Some("h-6 rounded-full px-2 uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
                                                    "Current"
                                                }
                                            }
                                            if authority.is_selected {
                                                LbBadge {
                                                    variant: LbBadgeVariant::Outline,
                                                    class: Some("h-6 rounded-full px-2 uppercase tracking-[0.08em] text-[0.62rem]".to_string()),
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
                    class: "bg-card px-4 py-3 border-t border-border flex flex-wrap items-center justify-between gap-2",
                    div {
                        class: "flex items-center gap-2 text-[0.68rem] uppercase tracking-[0.08em] text-muted-foreground",
                        span { "↑↓ choose" }
                        span { "enter reload" }
                        span { "esc cancel" }
                    }
                    div {
                        class: "flex items-center gap-2",
                        UiButton {
                            label: "Cancel".to_string(),
                            variant: ButtonVariant::Secondary,
                            on_click: move |_| on_cancel.call(()),
                        }
                        UiButton {
                            label: "Switch".to_string(),
                            variant: ButtonVariant::Primary,
                            on_click: move |_| on_confirm.call(()),
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
            class: "shrink-0 overflow-hidden border-t border-border bg-background px-4 py-3 text-xs tracking-[0.02em] text-muted-foreground sm:px-6",
            div {
                class: "flex h-9 min-w-0 items-center justify-between gap-3 overflow-hidden",
                span { class: "min-w-0 truncate whitespace-nowrap text-card-foreground leading-none", "{left}" }
                div {
                    class: "flex min-w-0 items-center justify-end gap-2 overflow-hidden whitespace-nowrap",
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
            class: "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border border-border bg-background/70 px-3",
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
