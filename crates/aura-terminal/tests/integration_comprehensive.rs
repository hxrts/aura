//! Comprehensive terminal integration tests.

#![allow(
    missing_docs,
    dead_code,
    unused,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::all
)]

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_core::types::identifiers::AuthorityId;
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::state::{
    transition, ChannelInfoModalState, ChatFocus, CreateChannelModalState, CreateChannelStep,
    DetailFocus, DispatchCommand, QueuedModal, TopicModalState, TuiCommand, TuiState,
};
use aura_terminal::tui::types::SettingsSection;
use proptest::prelude::*;

#[allow(clippy::duplicate_mod)]
mod support;

use support::TestTui;

fn screen_key_strategy() -> impl Strategy<Value = char> {
    prop_oneof![Just('1'), Just('2'), Just('3'), Just('4'), Just('5')]
}

fn terminal_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        (1u8..=5).prop_map(|n| events::char(char::from_digit(n as u32, 10).unwrap())),
        Just(events::char('h')),
        Just(events::char('j')),
        Just(events::char('k')),
        Just(events::char('l')),
        Just(events::char('i')),
        Just(events::escape()),
        Just(events::tab()),
        Just(events::enter()),
        Just(events::char('?')),
        Just(events::char('q')),
        Just(events::char('f')),
        Just(events::char('n')),
        (10u16..200, 10u16..100).prop_map(|(w, h)| events::resize(w, h)),
        any::<char>()
            .prop_filter("printable", |c| c.is_ascii_graphic())
            .prop_map(events::char),
    ]
}

#[path = "integration_comprehensive/chat_screen.rs"]
mod chat_screen;
#[path = "integration_comprehensive/contacts_screen.rs"]
mod contacts_screen;
#[path = "integration_comprehensive/global_behavior.rs"]
mod global_behavior;
#[path = "integration_comprehensive/integration_workflows.rs"]
mod integration_workflows;
#[path = "integration_comprehensive/modals.rs"]
mod modals;
#[path = "integration_comprehensive/neighborhood_screen.rs"]
mod neighborhood_screen;
#[path = "integration_comprehensive/neighborhood_screen_map.rs"]
mod neighborhood_screen_map;
#[path = "integration_comprehensive/property_tests.rs"]
mod property_tests;
#[path = "integration_comprehensive/settings_screen.rs"]
mod settings_screen;
#[path = "integration_comprehensive/stress.rs"]
mod stress;
