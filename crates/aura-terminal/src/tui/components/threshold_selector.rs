//! Threshold selection component (shared across wizards).

use iocraft::prelude::*;

use crate::tui::theme::{Icons, Spacing, Theme};

#[derive(Clone, Debug)]
pub struct ThresholdSelectorProps {
    pub prompt: String,
    pub subtext: Option<String>,
    pub k: u8,
    pub n: u8,
    pub low_hint: Option<String>,
    pub show_hint: bool,
}

impl ThresholdSelectorProps {
    pub fn new(prompt: impl Into<String>, k: u8, n: u8) -> Self {
        Self {
            prompt: prompt.into(),
            subtext: None,
            k,
            n,
            low_hint: None,
            show_hint: true,
        }
    }
}

#[must_use]
pub fn threshold_selector(props: &ThresholdSelectorProps) -> impl Into<AnyElement<'static>> {
    let k = props.k;
    let n = props.n;
    let hint = if props.show_hint {
        Some(if k == 1 {
            props
                .low_hint
                .clone()
                .unwrap_or_else(|| "Low: any one signer".to_string())
        } else if k == n {
            "Max: all must agree".to_string()
        } else {
            "Balanced".to_string()
        })
    } else {
        None
    };

    element! {
        View(
            padding: Spacing::SM,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
        ) {
            Text(content: props.prompt.clone(), color: Theme::TEXT_MUTED)
            #(props.subtext.as_ref().map(|text| {
                element! {
                    View(margin_top: Spacing::XS) {
                        Text(content: text.clone(), color: Theme::TEXT_MUTED)
                    }
                }
            }))

            View(
                margin_top: Spacing::SM,
                margin_bottom: Spacing::SM,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
            ) {
                Text(
                    content: Icons::ARROW_UP,
                    color: if k < n { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )
                View(
                    border_style: BorderStyle::Round,
                    border_color: Theme::PRIMARY,
                    padding_left: 2,
                    padding_right: 2,
                ) {
                    Text(
                        content: format!("{} of {}", k, n),
                        color: Theme::PRIMARY,
                        weight: Weight::Bold,
                    )
                }
                Text(
                    content: Icons::ARROW_DOWN,
                    color: if k > 1 { Theme::PRIMARY } else { Theme::TEXT_MUTED },
                    weight: Weight::Bold,
                )
            }

            #(hint.map(|text| {
                element! {
                    Text(content: text, color: Theme::SECONDARY)
                }
            }))
        }
    }
}
