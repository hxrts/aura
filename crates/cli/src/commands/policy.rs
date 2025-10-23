// Policy management commands

use anyhow::Result;
use aura_policy::{Operation, PolicyContext, PolicyTemplate, OperationType, RiskTier};
use aura_policy::types::DevicePosture;
use tracing::info;

pub fn handle_policy_command(subcommand: &PolicySubcommand) -> Result<()> {
    match subcommand {
        PolicySubcommand::List => list_templates(),
        PolicySubcommand::Docs { template } => show_docs(template),
        PolicySubcommand::Evaluate { template, operation, risk } => {
            evaluate_policy(template, operation, risk)
        }
    }
}

#[derive(clap::Subcommand)]
pub enum PolicySubcommand {
    /// List available policy templates
    List,
    /// Show documentation for a template
    Docs {
        /// Template name
        #[arg(short, long)]
        template: String,
    },
    /// Evaluate a policy decision
    Evaluate {
        /// Policy template to use
        #[arg(short, long, default_value = "balanced")]
        template: String,
        /// Operation to evaluate
        #[arg(short, long)]
        operation: String,
        /// Risk tier
        #[arg(short, long, default_value = "medium")]
        risk: String,
    },
}

fn list_templates() -> Result<()> {
    println!("Available Policy Templates:");
    println!();
    
    for template in aura_policy::templates::docs::list_templates() {
        println!("  - {}", template);
    }
    
    println!();
    println!("Use 'aura policy docs --template <name>' to see details");
    
    Ok(())
}

fn show_docs(template: &str) -> Result<()> {
    match aura_policy::templates::docs::get_template_docs(template) {
        Some(docs) => {
            println!("{}", docs);
            Ok(())
        }
        None => {
            eprintln!("Unknown template: {}", template);
            eprintln!("Use 'aura policy list' to see available templates");
            Err(anyhow::anyhow!("Unknown template"))
        }
    }
}

fn evaluate_policy(template: &str, operation: &str, risk: &str) -> Result<()> {
    info!("Evaluating policy with template: {}", template);
    
    // Load policy engine
    let engine = match template {
        "conservative" => PolicyTemplate::conservative(),
        "balanced" => PolicyTemplate::balanced(),
        "solo_user" => PolicyTemplate::solo_user(),
        "enterprise" => PolicyTemplate::enterprise(),
        "permissive" => PolicyTemplate::permissive(),
        _ => {
            eprintln!("Unknown template: {}", template);
            return Err(anyhow::anyhow!("Unknown template"));
        }
    };
    
    // Parse risk tier
    let risk_tier = match risk.to_lowercase().as_str() {
        "low" => RiskTier::Low,
        "medium" => RiskTier::Medium,
        "high" => RiskTier::High,
        "critical" => RiskTier::Critical,
        _ => {
            eprintln!("Unknown risk tier: {}", risk);
            return Err(anyhow::anyhow!("Unknown risk tier"));
        }
    };
    
    // Parse operation type
    let operation_type = match operation.to_lowercase().as_str() {
        "add_device" => OperationType::AddDevice,
        "remove_device" => OperationType::RemoveDevice,
        "add_guardian" => OperationType::AddGuardian,
        "remove_guardian" => OperationType::RemoveGuardian,
        "store_object" => OperationType::StoreObject,
        "fetch_object" => OperationType::FetchObject,
        _ => {
            eprintln!("Unknown operation: {}", operation);
            return Err(anyhow::anyhow!("Unknown operation"));
        }
    };
    
    // Create mock context
    let ctx = PolicyContext {
        account_id: aura_journal::AccountId::new(),
        requester: aura_journal::DeviceId::new(),
        device_posture: DevicePosture {
            device_id: aura_journal::DeviceId::new(),
            device_type: aura_policy::types::DeviceType::Native,
            is_hardware_backed: true,
            has_secure_boot: true,
            is_jailbroken: false,
            last_attestation: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
        },
        operation: Operation {
            operation_type,
            risk_tier,
            resource: None,
        },
        guardians_count: 2,
        active_devices_count: 3,
        session_epoch: 1,
    };
    
    // Create effects for the evaluation
    let effects = aura_crypto::Effects::default();
    
    // Evaluate
    match engine.evaluate(&ctx, &effects) {
        Ok(decision) => {
            println!("Policy Decision: {:?}", decision);
            Ok(())
        }
        Err(e) => {
            eprintln!("Policy evaluation failed: {}", e);
            Err(e.into())
        }
    }
}

