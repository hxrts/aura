//! # TUI Components
//!
//! Reusable UI components for the demo interface.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Reusable progress bar component
pub fn render_progress_bar(
    f: &mut Frame<'_>,
    area: Rect,
    title: &str,
    progress: u16,
    color: Color,
) {
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title(title))
        .gauge_style(Style::default().fg(color))
        .percent(progress);

    f.render_widget(gauge, area);
}

/// Reusable message list component
pub fn render_message_list(
    f: &mut Frame<'_>,
    area: Rect,
    title: &str,
    messages: &[String],
    max_items: usize,
) {
    let items: Vec<ListItem> = messages
        .iter()
        .rev()
        .take(max_items)
        .map(|msg| ListItem::new(msg.as_str()))
        .collect();

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    f.render_widget(list, area);
}

/// Status panel with multiple sections
pub fn render_status_panel(f: &mut Frame<'_>, area: Rect, sections: &[(&str, Vec<Line>)]) {
    let constraints = vec![Constraint::Length(sections.len() as u16 * 4); sections.len()];
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, (title, content)) in sections.iter().enumerate() {
        let paragraph = Paragraph::new(content.clone())
            .block(Block::default().borders(Borders::ALL).title(*title))
            .wrap(Wrap { trim: true });

        f.render_widget(paragraph, chunks[i]);
    }
}

/// Guardian status component
pub fn render_guardian_status(
    f: &mut Frame<'_>,
    area: Rect,
    guardian_name: &str,
    approved: bool,
    online: bool,
) {
    let status_text = if approved {
        "[APPROVED]"
    } else if online {
        "[PENDING]"
    } else {
        "[OFFLINE]"
    };

    let status_color = if approved {
        Color::Green
    } else if online {
        Color::Yellow
    } else {
        Color::Red
    };

    let text = vec![Line::from(vec![
        Span::raw(format!("{}: ", guardian_name)),
        Span::styled(status_text, Style::default().fg(status_color)),
    ])];

    let paragraph = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: true });

    f.render_widget(paragraph, area);
}
