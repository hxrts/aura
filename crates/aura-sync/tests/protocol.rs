//! Sync protocol integration and end-to-end tests.

#[path = "support.rs"]
mod shared_support;

mod protocol {
    mod integration_example;
    mod journal_sync_e2e;
    mod protocol_integration;
}
