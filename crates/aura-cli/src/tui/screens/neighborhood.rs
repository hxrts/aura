//! # Neighborhood Screen
//!
//! Display and navigate neighborhoods with block traversal.
//! Implements the urban social topology traversal from `work/neighbor.md`.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use super::{Screen, ScreenType};
use crate::tui::input::InputAction;
use crate::tui::styles::Styles;

/// Traversal depth in a block
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraversalDepth {
    /// Can see frontage, no interior access
    #[default]
    Street,
    /// Can see public block info, limited interaction
    Frontage,
    /// Full resident-level access
    Interior,
}

impl TraversalDepth {
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Street => "Street",
            Self::Frontage => "Frontage",
            Self::Interior => "Interior",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Street => "Passing by - can see block frontage only",
            Self::Frontage => "At the door - can view public info",
            Self::Interior => "Inside - full access as resident or guest",
        }
    }
}

/// Block summary for display in neighborhood view
#[derive(Debug, Clone)]
pub struct BlockSummary {
    /// Block ID
    pub id: String,
    /// Block name
    pub name: Option<String>,
    /// Number of residents
    pub resident_count: u8,
    /// Maximum residents (8 in v1)
    pub max_residents: u8,
    /// Whether this is the user's home block
    pub is_home: bool,
    /// Whether the user can enter (has capability)
    pub can_enter: bool,
    /// Whether currently at this block
    pub is_current: bool,
}

/// Adjacency relationship between blocks
#[derive(Debug, Clone)]
pub struct BlockAdjacency {
    /// First block ID
    pub block_a: String,
    /// Second block ID
    pub block_b: String,
}

/// Current traversal position
#[derive(Debug, Clone, Default)]
pub struct TraversalPosition {
    /// Current neighborhood (None = not in any neighborhood)
    pub neighborhood_id: Option<String>,
    /// Current block (None = on the street)
    pub block_id: Option<String>,
    /// Depth of access
    pub depth: TraversalDepth,
    /// When this position was entered
    pub entered_at: u64,
}

/// Neighborhood screen state
pub struct NeighborhoodScreen {
    /// Neighborhood ID being viewed
    neighborhood_id: Option<String>,
    /// Neighborhood name
    neighborhood_name: Option<String>,
    /// Blocks in this neighborhood
    blocks: Vec<BlockSummary>,
    /// Selected block index
    selected_block: Option<usize>,
    /// Block list state
    list_state: ListState,
    /// Adjacency graph (block connections)
    adjacencies: Vec<BlockAdjacency>,
    /// Current traversal position
    position: TraversalPosition,
    /// Whether showing adjacencies panel
    show_adjacencies: bool,
    /// Focus: 0=blocks, 1=adjacencies, 2=position
    focused_panel: usize,
    /// Flag for redraw
    needs_redraw: bool,
}

impl NeighborhoodScreen {
    /// Create a new neighborhood screen
    pub fn new() -> Self {
        Self {
            neighborhood_id: None,
            neighborhood_name: None,
            blocks: Vec::new(),
            selected_block: None,
            list_state: ListState::default(),
            adjacencies: Vec::new(),
            position: TraversalPosition::default(),
            show_adjacencies: false,
            focused_panel: 0,
            needs_redraw: true,
        }
    }

    /// Set the neighborhood being viewed
    pub fn set_neighborhood(&mut self, id: String, name: Option<String>) {
        self.neighborhood_id = Some(id);
        self.neighborhood_name = name;
        self.needs_redraw = true;
    }

    /// Set the blocks in this neighborhood
    pub fn set_blocks(&mut self, blocks: Vec<BlockSummary>) {
        self.blocks = blocks;
        if self.selected_block.is_none() && !self.blocks.is_empty() {
            self.selected_block = Some(0);
            self.list_state.select(Some(0));
        }
        self.needs_redraw = true;
    }

    /// Set adjacency relationships
    pub fn set_adjacencies(&mut self, adjacencies: Vec<BlockAdjacency>) {
        self.adjacencies = adjacencies;
        self.needs_redraw = true;
    }

    /// Set current position
    pub fn set_position(&mut self, position: TraversalPosition) {
        self.position = position;
        self.needs_redraw = true;
    }

    /// Get selected block
    pub fn selected_block(&self) -> Option<&BlockSummary> {
        self.selected_block.and_then(|idx| self.blocks.get(idx))
    }

    /// Get adjacent blocks to the given block
    pub fn get_adjacent_blocks(&self, block_id: &str) -> Vec<&BlockSummary> {
        let adjacent_ids: Vec<&str> = self
            .adjacencies
            .iter()
            .filter_map(|adj| {
                if adj.block_a == block_id {
                    Some(adj.block_b.as_str())
                } else if adj.block_b == block_id {
                    Some(adj.block_a.as_str())
                } else {
                    None
                }
            })
            .collect();

        self.blocks
            .iter()
            .filter(|b| adjacent_ids.contains(&b.id.as_str()))
            .collect()
    }

    /// Move selection up
    fn select_prev(&mut self) {
        if let Some(selected) = self.selected_block {
            if selected > 0 {
                self.selected_block = Some(selected - 1);
                self.list_state.select(Some(selected - 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Move selection down
    fn select_next(&mut self) {
        if let Some(selected) = self.selected_block {
            if selected + 1 < self.blocks.len() {
                self.selected_block = Some(selected + 1);
                self.list_state.select(Some(selected + 1));
                self.needs_redraw = true;
            }
        }
    }

    /// Next panel
    fn next_panel(&mut self) {
        self.focused_panel = (self.focused_panel + 1) % 3;
        self.needs_redraw = true;
    }

    /// Toggle adjacencies panel
    fn toggle_adjacencies(&mut self) {
        self.show_adjacencies = !self.show_adjacencies;
        self.needs_redraw = true;
    }

    /// Render the neighborhood header
    fn render_header(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let title = format!(
            " {} ",
            self.neighborhood_name.as_deref().unwrap_or("Neighborhood")
        );

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let lines = vec![
            Line::from(vec![
                Span::styled("Blocks: ", styles.text_muted()),
                Span::styled(format!("{}", self.blocks.len()), styles.text()),
            ]),
            Line::from(vec![
                Span::styled("Connections: ", styles.text_muted()),
                Span::styled(format!("{}", self.adjacencies.len()), styles.text()),
            ]),
        ];

        let header = Paragraph::new(lines).alignment(Alignment::Center);
        f.render_widget(header, inner);
    }

    /// Render the blocks list
    fn render_blocks(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Blocks ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 0 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let items: Vec<ListItem> = self
            .blocks
            .iter()
            .map(|blk| {
                let (icon, icon_style) = if blk.is_current {
                    ("*", styles.text_highlight())
                } else if blk.is_home {
                    ("H", styles.text_success())
                } else if blk.can_enter {
                    (">", styles.text())
                } else {
                    ("-", styles.text_muted())
                };

                let name = blk.name.as_deref().unwrap_or(&blk.id);
                let count_str = format!(" ({}/{})", blk.resident_count, blk.max_residents);

                let line = Line::from(vec![
                    Span::styled(format!("{} ", icon), icon_style),
                    Span::styled(name, styles.text()),
                    Span::styled(count_str, styles.text_muted()),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(block).highlight_style(
            Style::default()
                .add_modifier(Modifier::BOLD)
                .bg(styles.palette.surface),
        );

        let mut state = self.list_state.clone();
        f.render_stateful_widget(list, area, &mut state);
    }

    /// Render current position panel
    fn render_position(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Current Position ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 2 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let depth_style = match self.position.depth {
            TraversalDepth::Street => styles.text_muted(),
            TraversalDepth::Frontage => styles.text_warning(),
            TraversalDepth::Interior => styles.text_success(),
        };

        let current_block = self
            .position
            .block_id
            .as_deref()
            .unwrap_or("None (on the street)");

        let lines = vec![
            Line::from(vec![
                Span::styled("Block: ", styles.text_muted()),
                Span::styled(current_block, styles.text()),
            ]),
            Line::from(vec![
                Span::styled("Depth: ", styles.text_muted()),
                Span::styled(self.position.depth.name(), depth_style),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                self.position.depth.description(),
                styles.text_muted(),
            )]),
        ];

        let pos_para = Paragraph::new(lines);
        f.render_widget(pos_para, inner);
    }

    /// Render adjacencies panel
    fn render_adjacencies(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Adjacent Blocks ")
            .borders(Borders::ALL)
            .border_style(if self.focused_panel == 1 {
                styles.border_focused()
            } else {
                styles.border()
            });

        let inner = block.inner(area);
        f.render_widget(block, area);

        let selected = self.selected_block();

        if let Some(blk) = selected {
            let adjacent = self.get_adjacent_blocks(&blk.id);

            if adjacent.is_empty() {
                let empty = Paragraph::new("No adjacent blocks").style(styles.text_muted());
                f.render_widget(empty, inner);
            } else {
                let lines: Vec<Line> = adjacent
                    .iter()
                    .map(|adj| {
                        let name = adj.name.as_deref().unwrap_or(&adj.id);
                        let can_enter = if adj.can_enter { " (can enter)" } else { "" };
                        Line::from(vec![
                            Span::styled(" -> ", styles.text_muted()),
                            Span::styled(name, styles.text()),
                            Span::styled(can_enter, styles.text_success()),
                        ])
                    })
                    .collect();

                let adj_para = Paragraph::new(lines);
                f.render_widget(adj_para, inner);
            }
        } else {
            let empty = Paragraph::new("Select a block").style(styles.text_muted());
            f.render_widget(empty, inner);
        }
    }

    /// Render actions
    fn render_actions(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let block = Block::default()
            .title(" Actions ")
            .borders(Borders::ALL)
            .border_style(styles.border());

        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut actions = Vec::new();

        if let Some(blk) = self.selected_block() {
            if blk.can_enter && !blk.is_current {
                actions.push(Span::styled("[Enter] ", styles.text_highlight()));
                actions.push(Span::styled("Visit Block  ", styles.text()));
            }
        }

        actions.push(Span::styled("[A] ", styles.text_muted()));
        actions.push(Span::styled("Toggle Adjacencies  ", styles.text()));

        if self.position.block_id.is_some() {
            actions.push(Span::styled("[L] ", styles.text_warning()));
            actions.push(Span::styled("Leave Block  ", styles.text()));
        }

        let action_line = Line::from(actions);
        let actions_para = Paragraph::new(action_line).wrap(Wrap { trim: true });
        f.render_widget(actions_para, inner);
    }
}

impl Default for NeighborhoodScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen for NeighborhoodScreen {
    fn screen_type(&self) -> ScreenType {
        ScreenType::Neighborhood
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_prev();
                None
            }
            KeyCode::Tab => {
                self.next_panel();
                None
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.toggle_adjacencies();
                None
            }
            KeyCode::Enter => {
                if let Some(blk) = self.selected_block() {
                    if blk.can_enter && !blk.is_current {
                        return Some(InputAction::Submit(format!(
                            "action:enter_block:{}",
                            blk.id
                        )));
                    }
                }
                None
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                if self.position.block_id.is_some() {
                    return Some(InputAction::Submit(
                        "action:leave_current_block".to_string(),
                    ));
                }
                None
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                // View selected block details
                if let Some(blk) = self.selected_block() {
                    return Some(InputAction::Submit(format!("action:view_block:{}", blk.id)));
                }
                None
            }
            _ => None,
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        // Layout: header, main content, actions
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Header
                Constraint::Min(10),   // Main content
                Constraint::Length(3), // Actions
            ])
            .split(area);

        self.render_header(f, main_chunks[0], styles);

        // Main content layout depends on whether adjacencies are shown
        if self.show_adjacencies {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(40), // Blocks
                    Constraint::Percentage(30), // Adjacencies
                    Constraint::Percentage(30), // Position
                ])
                .split(main_chunks[1]);

            self.render_blocks(f, content_chunks[0], styles);
            self.render_adjacencies(f, content_chunks[1], styles);
            self.render_position(f, content_chunks[2], styles);
        } else {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(main_chunks[1]);

            self.render_blocks(f, content_chunks[0], styles);
            self.render_position(f, content_chunks[1], styles);
        }

        self.render_actions(f, main_chunks[2], styles);
    }

    fn on_enter(&mut self) {
        self.needs_redraw = true;
    }

    fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    fn update(&mut self) {
        self.needs_redraw = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_neighborhood_screen_new() {
        let screen = NeighborhoodScreen::new();
        assert_eq!(screen.screen_type(), ScreenType::Neighborhood);
        assert!(screen.blocks.is_empty());
    }

    #[test]
    fn test_set_blocks() {
        let mut screen = NeighborhoodScreen::new();
        let blocks = vec![
            BlockSummary {
                id: "block1".to_string(),
                name: Some("Alpha Block".to_string()),
                resident_count: 5,
                max_residents: 8,
                is_home: true,
                can_enter: true,
                is_current: false,
            },
            BlockSummary {
                id: "block2".to_string(),
                name: Some("Beta Block".to_string()),
                resident_count: 8,
                max_residents: 8,
                is_home: false,
                can_enter: false,
                is_current: false,
            },
        ];
        screen.set_blocks(blocks);
        assert_eq!(screen.blocks.len(), 2);
        assert_eq!(screen.selected_block, Some(0));
    }

    #[test]
    fn test_adjacencies() {
        let mut screen = NeighborhoodScreen::new();
        let blocks = vec![
            BlockSummary {
                id: "a".to_string(),
                name: None,
                resident_count: 4,
                max_residents: 8,
                is_home: false,
                can_enter: true,
                is_current: false,
            },
            BlockSummary {
                id: "b".to_string(),
                name: None,
                resident_count: 4,
                max_residents: 8,
                is_home: false,
                can_enter: true,
                is_current: false,
            },
            BlockSummary {
                id: "c".to_string(),
                name: None,
                resident_count: 4,
                max_residents: 8,
                is_home: false,
                can_enter: true,
                is_current: false,
            },
        ];
        screen.set_blocks(blocks);

        let adjacencies = vec![
            BlockAdjacency {
                block_a: "a".to_string(),
                block_b: "b".to_string(),
            },
            BlockAdjacency {
                block_a: "b".to_string(),
                block_b: "c".to_string(),
            },
        ];
        screen.set_adjacencies(adjacencies);

        let adj_to_b = screen.get_adjacent_blocks("b");
        assert_eq!(adj_to_b.len(), 2); // a and c
    }

    #[test]
    fn test_traversal_depth() {
        assert_eq!(TraversalDepth::Street.name(), "Street");
        assert_eq!(TraversalDepth::Frontage.name(), "Frontage");
        assert_eq!(TraversalDepth::Interior.name(), "Interior");
    }

    #[test]
    fn test_navigation() {
        let mut screen = NeighborhoodScreen::new();
        let blocks = vec![
            BlockSummary {
                id: "a".to_string(),
                name: None,
                resident_count: 4,
                max_residents: 8,
                is_home: false,
                can_enter: true,
                is_current: false,
            },
            BlockSummary {
                id: "b".to_string(),
                name: None,
                resident_count: 4,
                max_residents: 8,
                is_home: false,
                can_enter: true,
                is_current: false,
            },
        ];
        screen.set_blocks(blocks);

        assert_eq!(screen.selected_block, Some(0));
        screen.select_next();
        assert_eq!(screen.selected_block, Some(1));
        screen.select_prev();
        assert_eq!(screen.selected_block, Some(0));
    }

    #[test]
    fn test_panel_focus() {
        let mut screen = NeighborhoodScreen::new();
        assert_eq!(screen.focused_panel, 0);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 1);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 2);
        screen.next_panel();
        assert_eq!(screen.focused_panel, 0);
    }
}
