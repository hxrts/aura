use dioxus::prelude::*;
use lumen_blocks::components::button::{
    Button as LbButton, ButtonSize as LbButtonSize, ButtonVariant as LbButtonVariant,
};
use lumen_blocks::components::dialog::{
    DialogContent as LbDialogContent, DialogDescription as LbDialogDescription,
    DialogRoot as LbDialogRoot, DialogTitle as LbDialogTitle,
};
use lumen_blocks::components::input::Input as LbInput;
use lumen_blocks::components::label::Label as LbLabel;

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
    pub input_label: Option<String>,
    pub input_value: Option<String>,
    pub enter_label: String,
}

fn map_button_variant(variant: ButtonVariant) -> LbButtonVariant {
    match variant {
        ButtonVariant::Primary => LbButtonVariant::Primary,
        ButtonVariant::Secondary => LbButtonVariant::Outline,
    }
}

fn pill_tone_class(tone: PillTone) -> &'static str {
    match tone {
        PillTone::Neutral => "text-slate-300 bg-slate-800 border-slate-700",
        PillTone::Info => "text-sky-300 bg-sky-500/15 border-sky-500/30",
        PillTone::Success => "text-emerald-300 bg-emerald-500/15 border-emerald-500/30",
    }
}

#[component]
pub fn UiTabButton(
    label: &'static str,
    active: bool,
    on_click: EventHandler<MouseEvent>,
) -> Element {
    let variant = if active {
        LbButtonVariant::Primary
    } else {
        LbButtonVariant::Outline
    };

    rsx! {
        LbButton {
            variant,
            size: LbButtonSize::Small,
            class: "text-xs uppercase tracking-[0.04em]",
            on_click: move |evt| on_click.call(evt),
            "{label}"
        }
    }
}

#[component]
pub fn UiCard(
    title: String,
    subtitle: Option<String>,
    extra_class: Option<String>,
    children: Element,
) -> Element {
    let card_class = format!(
        "rounded-xl border border-border bg-card {}",
        extra_class.unwrap_or_default()
    );
    rsx! {
        section {
            class: "{card_class}",
            header {
                class: "px-3 py-2.5 border-b border-border",
                h3 { class: "m-0 text-xs font-semibold uppercase tracking-[0.08em] text-card-foreground", "{title}" }
                if let Some(subtitle) = subtitle {
                    p { class: "m-0 mt-1 text-xs text-muted-foreground", "{subtitle}" }
                }
            }
            div {
                class: "p-3 space-y-2 text-sm text-card-foreground",
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
    let tone_class = pill_tone_class(tone);
    rsx! {
        span {
            class: "inline-flex h-6 items-center rounded-full border px-2 text-[0.65rem] uppercase tracking-[0.06em] {tone_class}",
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
                class: Some("w-full max-w-xl shadow-2xl p-0 overflow-hidden".to_string()),
                div {
                    class: "px-4 py-3 border-b border-border flex items-center justify-between gap-3",
                    LbDialogTitle {
                        id: Some("aura-modal-title".to_string()),
                        class: Some("m-0 text-sm font-semibold text-card-foreground".to_string()),
                        "{modal.title}"
                    }
                    span { class: "text-[0.66rem] uppercase tracking-[0.06em] text-muted-foreground", "Esc closes" }
                }
                div {
                    class: "px-4 py-3 space-y-2 text-sm text-card-foreground",
                    LbDialogDescription {
                        id: Some("aura-modal-description".to_string()),
                        class: Some("sr-only".to_string()),
                        "Aura modal dialog"
                    }
                    for line in modal.details {
                        p { class: "m-0 whitespace-pre-wrap break-words", "{line}" }
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
                                    on_input_change.call(evt.value().to_string());
                                },
                            }
                        }
                    }
                }
                div {
                    class: "px-4 py-3 border-t border-border flex items-center justify-end gap-2",
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
