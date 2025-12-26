//! Shared multi-select list for contacts/peers.

use iocraft::prelude::*;

use crate::tui::theme::{Spacing, Theme};

#[derive(Clone, Debug)]
pub struct ContactMultiSelectItem {
    pub name: String,
    pub badge: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ContactMultiSelectProps {
    pub prompt: String,
    pub items: Vec<ContactMultiSelectItem>,
    pub selected: Vec<usize>,
    pub focused: usize,
    pub min_selected: Option<usize>,
    pub footer_hint: Option<String>,
}

impl ContactMultiSelectProps {
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            items: Vec::new(),
            selected: Vec::new(),
            focused: 0,
            min_selected: None,
            footer_hint: None,
        }
    }
}

pub fn contact_multi_select(props: &ContactMultiSelectProps) -> impl Into<AnyElement<'static>> {
    let selected_count = props.selected.len();
    let min_required = props.min_selected.unwrap_or(0);
    let count_line = if min_required > 0 {
        format!("{selected_count} selected (min {min_required})")
    } else {
        format!("{selected_count} selected")
    };
    let count_color = if min_required == 0 || selected_count >= min_required {
        Theme::SUCCESS
    } else {
        Theme::WARNING
    };

    element! {
        View(
            padding_left: Spacing::SM,
            padding_right: Spacing::SM,
            padding_top: Spacing::XS,
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
        ) {
            Text(
                content: props.prompt.clone(),
                color: Theme::TEXT_MUTED,
            )

            View(
                margin_top: Spacing::XS,
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
                max_height: 10,
                overflow: Overflow::Hidden,
            ) {
                #(props.items.iter().enumerate().map(|(i, item)| {
                    let is_selected = props.selected.contains(&i);
                    let is_focused = i == props.focused;
                    let pointer = if is_focused { "▸" } else { " " };
                    let checkbox = if is_selected { "[x]" } else { "[ ]" };
                    let bg = if is_focused { Theme::BG_SELECTED } else { Color::Reset };
                    let fg = if is_focused { Theme::TEXT } else { Theme::TEXT_MUTED };
                    let pointer_color = if is_focused { Theme::PRIMARY } else { Color::Reset };

                    element! {
                        View(
                            flex_direction: FlexDirection::Row,
                            gap: 1,
                            padding_left: Spacing::XS,
                            background_color: bg,
                        ) {
                            Text(content: pointer.to_string(), color: pointer_color)
                            Text(content: checkbox.to_string(), color: if is_selected { Theme::SUCCESS } else { fg })
                            Text(content: item.name.clone(), color: fg)
                            #(item.badge.as_ref().map(|badge| {
                                element! {
                                    Text(content: badge.clone(), color: Theme::WARNING)
                                }
                            }))
                        }
                    }
                }))
            }

            Text(content: count_line, color: count_color)

            #(props.footer_hint.as_ref().map(|hint| {
                element! {
                    View(margin_top: Spacing::XS) {
                        Text(content: hint.clone(), color: Theme::TEXT_MUTED)
                    }
                }
            }))
        }
    }
}
