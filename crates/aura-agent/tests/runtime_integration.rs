//! Aggregated aura-agent runtime integration tests (one binary, one link).
#![cfg(not(target_arch = "wasm32"))]
#![allow(missing_docs)]

mod auth_service_test;
mod beta_flow_e2e;
mod bootstrap_required;
mod delta_application_test;
mod frp_glitch_freedom_test;
mod id_generation_tests;
mod integration_tests;
mod invitation_service_test;
mod journal_integration_test;
mod production_manifest_admission;
mod reactive_scheduler_signals_e2e;
mod reconfiguration_integration;
mod recovery_service_test;
mod runtime_bridge_channel_resolution;
mod session_service_test;
mod theorem_pack_admission;
mod threshold_signing_e2e;
