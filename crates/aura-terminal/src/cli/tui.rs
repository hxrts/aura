//! TUI (Terminal User Interface) commands
//!
//! This module defines the command-line interface for launching the
//! production TUI for interactive Aura management.

use bpaf::{construct, long, short, Parser};

/// TUI launch arguments for interactive terminal interface
#[derive(Debug, Clone)]
pub struct TuiArgs {
    /// Storage directory for Aura data.
    /// Falls back to $AURA_PATH if set, otherwise defaults to ./aura-data
    pub data_dir: Option<String>,

    /// Device ID to use for this session
    pub device_id: Option<String>,

    /// Run in demo mode with simulated Alice and Carol peer agents.
    /// Uses a real agent runtime with deterministic simulation.
    pub demo: bool,
}

pub fn tui_parser() -> impl Parser<TuiArgs> {
    let data_dir = short('d')
        .long("data-dir")
        .help("Storage directory for Aura data")
        .argument::<String>("DIR")
        .optional();
    let device_id = short('i')
        .long("device-id")
        .help("Device ID to use for this session")
        .argument::<String>("DEVICE")
        .optional();
    let demo = long("demo")
        .help("Run with simulated Alice/Carol peers for recovery demo")
        .switch();
    construct!(TuiArgs {
        data_dir,
        device_id,
        demo
    })
}
