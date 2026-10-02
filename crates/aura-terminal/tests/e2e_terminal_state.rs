//! End-to-end terminal state lifecycle tests.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::disallowed_methods,
    clippy::needless_borrows_for_generic_args,
    clippy::manual_range_contains,
    clippy::clone_on_copy,
    clippy::if_same_then_else
)]

#[allow(clippy::duplicate_mod)]
mod support;

#[path = "e2e_terminal_state/account_recovery.rs"]
mod account_recovery;
#[path = "e2e_terminal_state/backup_and_persistence.rs"]
mod backup_and_persistence;
#[path = "e2e_terminal_state/command_dispatch.rs"]
mod command_dispatch;
#[path = "e2e_terminal_state/component_states.rs"]
mod component_states;
#[path = "e2e_terminal_state/messaging_and_help.rs"]
mod messaging_and_help;

fn add_fixture_home(
    homes: &mut aura_app::views::home::HomesState,
    home: aura_app::views::home::HomeState,
) -> aura_app::views::home::AddHomeResult {
    let home_id = home.id;
    let was_first = homes.is_empty();
    let mut detached = serde_json::to_value(&*homes).unwrap();
    detached["homes"]
        .as_object_mut()
        .unwrap()
        .insert(home_id.to_string(), serde_json::to_value(home).unwrap());
    *homes = serde_json::from_value(detached).unwrap();
    aura_app::views::home::AddHomeResult { home_id, was_first }
}
