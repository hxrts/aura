//! Telltale protocol-machine parity, contract and hardening lanes.
#![cfg(feature = "choreo-backend-telltale-machine")]
#![allow(missing_docs)]

mod telltale_machine_parity;

#[cfg(not(target_arch = "wasm32"))]
mod telltale_machine_concurrent_contracts;
#[cfg(not(target_arch = "wasm32"))]
mod telltale_machine_hardening;
#[cfg(not(target_arch = "wasm32"))]
mod telltale_machine_scenario_contracts;
