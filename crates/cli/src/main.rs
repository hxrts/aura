// CLI for smoke tests and operator tooling

use clap::{Parser, Subcommand};

mod commands {
    pub mod init;
    pub mod device;
    pub mod guardian;
    pub mod epoch;
    pub mod status;
    pub mod dkd;
    pub mod policy;
}

#[derive(Parser)]
#[command(name = "aura")]
#[command(about = "Aura - Threshold Identity and Storage Platform", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new account with DKG
    Init {
        /// Number of participants
        #[arg(short = 'n', long, default_value = "3")]
        participants: u16,
        
        /// Threshold (M in M-of-N)
        #[arg(short = 't', long, default_value = "2")]
        threshold: u16,
        
        /// Output directory for configuration
        #[arg(short, long, default_value = ".aura")]
        output: String,
    },
    
    /// Add a new device
    AddDevice {
        /// Device name
        #[arg(short, long)]
        name: String,
        
        /// Device type (native, guardian, browser)
        #[arg(short, long, default_value = "native")]
        device_type: String,
        
        /// Config file path
        #[arg(short, long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Remove a device
    RemoveDevice {
        /// Device ID (UUID)
        #[arg(short, long)]
        device_id: String,
        
        /// Reason for removal
        #[arg(short, long)]
        reason: String,
        
        /// Config file path
        #[arg(short, long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Add a guardian
    AddGuardian {
        /// Guardian name
        #[arg(short, long)]
        name: String,
        
        /// Contact method (signal:+1234567890, email:user@example.com)
        #[arg(short, long)]
        contact: String,
        
        /// Config file path
        #[arg(short, long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Bump session epoch (invalidate presence tickets)
    BumpEpoch {
        /// Reason for epoch bump
        #[arg(short, long)]
        reason: String,
        
        /// Config file path
        #[arg(short, long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Show account status
    Status {
        /// Config file path
        #[arg(short, long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Test key derivation
    TestDkd {
        /// App ID
        #[arg(short, long)]
        app_id: String,
        
        /// Context label
        #[arg(short, long)]
        context: String,
        
        /// Config file path
        #[arg(short = 'f', long, default_value = ".aura/config.toml")]
        config: String,
    },
    
    /// Policy management
    Policy {
        #[command(subcommand)]
        command: commands::policy::PolicySubcommand,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    // Initialize tracing
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(log_level)
        .init();
    
    match cli.command {
        Commands::Init { participants, threshold, output } => {
            commands::init::run(participants, threshold, &output).await?;
        }
        Commands::AddDevice { name, device_type, config } => {
            commands::device::add_device(&config, &name, &device_type).await?;
        }
        Commands::RemoveDevice { device_id, reason, config } => {
            commands::device::remove_device(&config, &device_id, &reason).await?;
        }
        Commands::AddGuardian { name, contact, config } => {
            commands::guardian::add_guardian(&config, &name, &contact).await?;
        }
        Commands::BumpEpoch { reason, config } => {
            commands::epoch::bump_epoch(&config, &reason).await?;
        }
        Commands::Status { config } => {
            commands::status::show_status(&config).await?;
        }
        Commands::TestDkd { app_id, context, config } => {
            commands::dkd::test_dkd(&config, &app_id, &context).await?;
        }
        Commands::Policy { command } => {
            commands::policy::handle_policy_command(&command)?;
        }
    }
    
    Ok(())
}

