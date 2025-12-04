//! TUI (Terminal User Interface) commands
//!
//! This module defines the command-line interface for launching the
//! production TUI for interactive Aura management.

use clap::Args;

/// TUI launch arguments for interactive terminal interface
#[derive(Debug, Clone, Args)]
pub struct TuiArgs {
    /// Storage directory for Aura data.
    /// Falls back to $AURA_PATH if set, otherwise defaults to ./aura-data
    #[arg(short, long)]
    pub data_dir: Option<String>,

    /// Device ID to use for this session
    #[arg(short = 'i', long)]
    pub device_id: Option<String>,

    /// Use demo mode with sample data for testing navigation
    #[arg(long)]
    pub demo: bool,
}
