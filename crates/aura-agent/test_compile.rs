//! Minimal test to check if agent compiles

use aura_types::identifiers::DeviceId;

mod agent;

fn main() {
    let device_id = DeviceId::new();
    let agent = agent::AuraAgent::for_testing(device_id);
    println!("Agent created successfully with device ID: {}", agent.device_id());
}