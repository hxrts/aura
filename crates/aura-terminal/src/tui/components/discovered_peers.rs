//! # Discovered Peers Panel
//!
//! Panel showing LAN-discovered peers that can be invited as contacts.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::theme::Theme;

/// Callback type for inviting a discovered peer
pub type InvitePeerCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// A discovered peer from LAN broadcast
#[derive(Clone, Debug, Default)]
pub struct DiscoveredPeerInfo {
    /// Authority ID (hex string)
    pub authority_id: String,
    /// Display name if available
    pub display_name: Option<String>,
    /// IP address and port
    pub address: String,
    /// Discovery method (LAN, relay, etc.)
    pub discovery_method: String,
    /// Time since discovery in seconds
    pub age_secs: u64,
}

impl DiscoveredPeerInfo {
    /// Create a new discovered peer info
    pub fn new(authority_id: impl Into<String>, address: impl Into<String>) -> Self {
        Self {
            authority_id: authority_id.into(),
            address: address.into(),
            discovery_method: "LAN".to_string(),
            display_name: None,
            age_secs: 0,
        }
    }

    /// Set the display name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set the discovery method
    pub fn with_method(mut self, method: impl Into<String>) -> Self {
        self.discovery_method = method.into();
        self
    }

    /// Set the age in seconds
    pub fn with_age(mut self, age_secs: u64) -> Self {
        self.age_secs = age_secs;
        self
    }

    /// Get display label (name or truncated authority ID)
    pub fn display_label(&self) -> String {
        if let Some(name) = &self.display_name {
            name.clone()
        } else {
            // Show first 8 and last 4 chars of authority ID
            let id = &self.authority_id;
            if id.len() > 16 {
                format!("{}...{}", &id[..8], &id[id.len() - 4..])
            } else {
                id.clone()
            }
        }
    }

    /// Format age for display
    pub fn age_display(&self) -> String {
        if self.age_secs < 60 {
            format!("{}s ago", self.age_secs)
        } else if self.age_secs < 3600 {
            format!("{}m ago", self.age_secs / 60)
        } else {
            format!("{}h ago", self.age_secs / 3600)
        }
    }
}

/// Props for DiscoveredPeersPanel
#[derive(Default, Props)]
pub struct DiscoveredPeersPanelProps {
    /// List of discovered peers
    pub peers: Vec<DiscoveredPeerInfo>,
    /// Currently selected index
    pub selected_index: usize,
    /// Whether this panel is focused
    pub focused: bool,
    /// Callback when a peer is invited
    pub on_invite: Option<InvitePeerCallback>,
}

/// Panel showing discovered peers on the local network
#[component]
pub fn DiscoveredPeersPanel(props: &DiscoveredPeersPanelProps) -> impl Into<AnyElement<'static>> {
    let peers = props.peers.clone();
    let selected_index = props.selected_index;
    let focused = props.focused;

    let border_color = if focused {
        Theme::PRIMARY
    } else {
        Theme::BORDER
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
        ) {
            // Header
            View(
                padding_left: 2,
                padding_right: 2,
                padding_top: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                View(flex_direction: FlexDirection::Row, gap: 2) {
                    Text(content: "📡", color: Theme::PRIMARY)
                    Text(content: "Nearby Peers", weight: Weight::Bold, color: Theme::TEXT)
                    Text(
                        content: format!("({})", peers.len()),
                        color: Theme::TEXT_MUTED,
                    )
                }
            }

            // Peer list
            View(
                flex_direction: FlexDirection::Column,
                padding: 1,
                overflow: Overflow::Scroll,
            ) {
                #(if peers.is_empty() {
                    vec![element! {
                        View(padding: 1) {
                            Text(
                                content: "No peers discovered on local network",
                                color: Theme::TEXT_MUTED,
                            )
                        }
                    }]
                } else {
                    peers.iter().enumerate().map(|(idx, peer)| {
                        let is_selected = idx == selected_index && focused;
                        let bg = if is_selected {
                            Theme::BG_SELECTED
                        } else {
                            Theme::BG_DARK
                        };
                        let pointer = if is_selected { "▸ " } else { "  " }.to_string();
                        let label = peer.display_label();
                        let method = format!("[{}]", peer.discovery_method);
                        let address = peer.address.clone();
                        let age = peer.age_display();
                        let key = peer.authority_id.clone();

                        element! {
                            View(
                                key: key,
                                flex_direction: FlexDirection::Row,
                                background_color: bg,
                                padding_left: 1,
                                padding_right: 1,
                                gap: 1,
                            ) {
                                Text(content: pointer, color: Theme::PRIMARY)
                                Text(content: label, color: Theme::TEXT)
                                Text(content: method, color: Theme::SECONDARY)
                                Text(content: address, color: Theme::TEXT_MUTED)
                                View(flex_grow: 1.0) {}
                                Text(content: age, color: Theme::TEXT_MUTED)
                            }
                        }
                    }).collect()
                })
            }

            // Footer with key hint (only when focused and peers exist)
            #(if focused && !peers.is_empty() {
                Some(element! {
                    View(
                        padding_left: 2,
                        padding_right: 2,
                        border_style: BorderStyle::Single,
                        border_edges: Edges::Top,
                        border_color: Theme::BORDER,
                    ) {
                        View(flex_direction: FlexDirection::Row, gap: 2) {
                            Text(content: "Enter", color: Theme::SECONDARY)
                            Text(content: "Invite", color: Theme::TEXT_MUTED)
                        }
                    }
                })
            } else {
                None
            })
        }
    }
}

/// State for discovered peers panel
#[derive(Clone, Debug, Default)]
pub struct DiscoveredPeersState {
    /// List of discovered peers
    pub peers: Vec<DiscoveredPeerInfo>,
    /// Currently selected index
    pub selected_index: usize,
    /// Whether the panel is focused
    pub focused: bool,
}

impl DiscoveredPeersState {
    /// Create new state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the peers list
    pub fn set_peers(&mut self, peers: Vec<DiscoveredPeerInfo>) {
        self.peers = peers;
        // Ensure selected index is valid
        if self.selected_index >= self.peers.len() && !self.peers.is_empty() {
            self.selected_index = self.peers.len() - 1;
        }
    }

    /// Move selection up
    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if self.selected_index + 1 < self.peers.len() {
            self.selected_index += 1;
        }
    }

    /// Get the selected peer
    pub fn get_selected(&self) -> Option<&DiscoveredPeerInfo> {
        self.peers.get(self.selected_index)
    }

    /// Set focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Check if there are any peers
    pub fn has_peers(&self) -> bool {
        !self.peers.is_empty()
    }

    /// Check if can invite (has peers and one is selected)
    pub fn can_invite(&self) -> bool {
        self.focused && self.get_selected().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovered_peer_info() {
        let peer = DiscoveredPeerInfo::new("abc123def456", "192.168.1.100:8080")
            .with_name("Alice")
            .with_method("LAN")
            .with_age(120);

        assert_eq!(peer.display_label(), "Alice");
        assert_eq!(peer.age_display(), "2m ago");
    }

    #[test]
    fn test_peer_truncated_id() {
        let peer = DiscoveredPeerInfo::new(
            "0123456789abcdef0123456789abcdef",
            "192.168.1.100:8080",
        );

        // Should show first 8 and last 4 chars
        assert_eq!(peer.display_label(), "01234567...cdef");
    }

    #[test]
    fn test_discovered_peers_state() {
        let mut state = DiscoveredPeersState::new();
        assert!(!state.has_peers());
        assert!(!state.can_invite());

        let peers = vec![
            DiscoveredPeerInfo::new("peer1", "192.168.1.1:8080"),
            DiscoveredPeerInfo::new("peer2", "192.168.1.2:8080"),
            DiscoveredPeerInfo::new("peer3", "192.168.1.3:8080"),
        ];

        state.set_peers(peers);
        assert!(state.has_peers());
        assert_eq!(state.peers.len(), 3);
        assert_eq!(state.selected_index, 0);

        // Still can't invite until focused
        assert!(!state.can_invite());

        state.set_focused(true);
        assert!(state.can_invite());

        // Navigate
        state.select_next();
        assert_eq!(state.selected_index, 1);
        assert_eq!(state.get_selected().unwrap().authority_id, "peer2");

        state.select_prev();
        assert_eq!(state.selected_index, 0);
    }
}
