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
    rsx! {
        LbCard {
            class: Some(extra_class.unwrap_or_default()),
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
                class: Some("space-y-2 text-sm".to_string()),
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
                class: Some("w-full max-w-xl bg-card text-card-foreground shadow-2xl p-0 overflow-hidden".to_string()),
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
pub fn UiFooter(left: String, right: String) -> Element {
    rsx! {
        footer {
            class: "mx-3 mb-3 mt-1 rounded-xl border border-border bg-card text-muted-foreground text-xs tracking-[0.02em] flex justify-between gap-3 px-4 py-2.5 flex-wrap",
            span { class: "text-card-foreground", "{left}" }
            span { "{right}" }
        }
    }
}
