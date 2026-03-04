use crate::model::{ModalState, UiModel, UiScreen};

const PANEL_WIDTH: usize = 38;
const CONTENT_ROWS: usize = 20;
const SETTINGS_ROWS: [&str; 5] = [
    "Profile",
    "Guardian Threshold",
    "Request Recovery",
    "Devices",
    "Authority",
];

pub fn render_canonical_snapshot(model: &UiModel) -> String {
    let mut lines = Vec::with_capacity(CONTENT_ROWS + 4);

    lines.push("Neighborhood Chat Contacts Notifications Settings".to_string());
    lines.push("┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐".to_string());

    for row_idx in 0..CONTENT_ROWS {
        let (left, center, mut right) = panel_row(model, row_idx);
        if row_idx == 0 {
            let authority = format!("Authority: {} (local)", model.authority_id);
            right = if right.is_empty() {
                authority
            } else {
                format!("{right} {authority}")
            };
        }
        lines.push(format_panel_row(&left, &center, &right));
    }

    if let Some(toast) = &model.toast {
        lines.push(format!(
            "│ {:<114} │",
            format!("{} {} [Esc] dismiss", toast.icon, toast.message)
        ));
    }

    lines.join("\n")
}

fn panel_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    match model.screen {
        UiScreen::Neighborhood => neighborhood_row(model, row_idx),
        UiScreen::Chat => chat_row(model, row_idx),
        UiScreen::Contacts => contacts_row(model, row_idx),
        UiScreen::Notifications => notifications_row(model, row_idx),
        UiScreen::Settings => settings_row(model, row_idx),
    }
}

fn neighborhood_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            "Neighborhood".to_string(),
            String::new(),
            "Welcome to Aura".to_string(),
        );
    }
    if row_idx == 1 {
        return (
            "➤ Homes".to_string(),
            String::new(),
            model
                .selected_home
                .as_ref()
                .map(|home| format!("Selected home: {home}"))
                .unwrap_or_else(|| "Selected home: none".to_string()),
        );
    }
    if row_idx == 2 && matches!(model.modal, Some(ModalState::CreateHome)) {
        return (
            "".to_string(),
            "Create New Home".to_string(),
            model.modal_buffer.clone(),
        );
    }
    if row_idx == 2 {
        return (
            "Can enter: Neighborhood".to_string(),
            String::new(),
            String::new(),
        );
    }
    if row_idx == 3 {
        return (
            "Members & Participants".to_string(),
            String::new(),
            String::new(),
        );
    }
    if row_idx == 4 {
        return ("Member".to_string(), String::new(), String::new());
    }
    (String::new(), String::new(), String::new())
}

fn chat_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return ("Channels".to_string(), String::new(), "Chat".to_string());
    }

    if row_idx > 0 && row_idx <= model.channels.len() {
        let channel = &model.channels[row_idx - 1];
        let prefix = if channel.selected { "➤ " } else { "" };
        return (format!("{prefix}# {}", channel.name), String::new(), String::new());
    }

    let message_offset = row_idx.saturating_sub(4);
    if let Some(message) = model.messages.get(message_offset) {
        return (String::new(), String::new(), message.clone());
    }

    if row_idx == CONTENT_ROWS - 1 {
        let mode = if model.input_mode { "insert" } else { "normal" };
        let value = if model.input_mode {
            model.input_buffer.clone()
        } else {
            String::new()
        };
        return (format!("mode: {mode}"), value, String::new());
    }

    (String::new(), String::new(), String::new())
}

fn contacts_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            format!("Contacts ({})", model.contacts.len()),
            String::new(),
            "Contacts".to_string(),
        );
    }

    if row_idx > 0 && row_idx <= model.contacts.len() {
        let contact = &model.contacts[row_idx - 1];
        let prefix = if contact.selected { "➤ " } else { "" };
        return (
            format!("{prefix}○ {}", contact.name),
            String::new(),
            String::new(),
        );
    }

    if row_idx == 2 && matches!(model.modal, Some(ModalState::CreateInvitation)) {
        return (
            String::new(),
            "Invitation Created".to_string(),
            model
                .last_invite_code
                .clone()
                .unwrap_or_else(|| "pending".to_string()),
        );
    }

    if row_idx == 2 && matches!(model.modal, Some(ModalState::AcceptInvitation)) {
        return (
            String::new(),
            "Paste Invitation".to_string(),
            model.modal_buffer.clone(),
        );
    }

    (String::new(), String::new(), String::new())
}

fn notifications_row(_model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return (
            "Notifications".to_string(),
            String::new(),
            "No pending notifications".to_string(),
        );
    }
    (String::new(), String::new(), String::new())
}

fn settings_row(model: &UiModel, row_idx: usize) -> (String, String, String) {
    if row_idx == 0 {
        return ("Settings".to_string(), String::new(), String::new());
    }

    if row_idx > 0 && row_idx <= SETTINGS_ROWS.len() {
        let idx = row_idx - 1;
        let prefix = if idx == model.settings_index { "➤ " } else { "" };
        let right = if SETTINGS_ROWS[idx] == "Authority" {
            format!("Authority: {} (local)", model.authority_id)
        } else {
            String::new()
        };
        return (format!("{prefix}{}", SETTINGS_ROWS[idx]), String::new(), right);
    }

    (String::new(), String::new(), String::new())
}

fn format_panel_row(left: &str, center: &str, right: &str) -> String {
    format!(
        "│ {:<left_width$} │ {:<center_width$} │ {:<right_width$} │",
        left,
        center,
        right,
        left_width = PANEL_WIDTH,
        center_width = PANEL_WIDTH,
        right_width = PANEL_WIDTH,
    )
}
