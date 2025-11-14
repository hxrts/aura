//! Multi-Protocol Choreography Example  
//!
//! This example demonstrates the intended usage of aura_macros::choreography! macro
//! with multiple protocols using namespaces to avoid conflicts.

use serde::{Serialize, Deserialize};

// Message type definitions for worker pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub work_id: String,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub work_id: String,
    pub percentage: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusReport {
    pub worker_status: String,
    pub overall_progress: u8,
}

// Message types for consensus protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub proposal_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub proposal_id: String,
    pub accept: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub proposal_id: String,
    pub chosen_value: String,
}

// Example choreographies demonstrating multi-protocol usage
//
// Note: The actual macro usage requires proper rumpsteak-aura type imports
// and our extension system needs further development to support the full
// integration. This shows the intended syntax.

/*
Use case 1: Worker pool choreography with namespace

use aura_macros::choreography;

choreography! {
    #[namespace = "worker_pool"]
    choreography WorkerPool {
        roles: Master, Worker, Monitor;

        // Basic communication flow
        Master -> Worker: Task;
        Worker -> Monitor: Progress;
        Monitor -> Master: StatusReport;
    }
}

Use case 2: Consensus protocol with different namespace

choreography! {
    #[namespace = "consensus"]
    choreography DistributedConsensus {
        roles: Leader, Follower, Observer;

        // Simple leader-follower communication
        Leader -> Follower: Proposal;
        Follower -> Leader: Vote;
        Leader -> Observer: Decision;
    }
}

With Aura extensions (planned):

choreography! {
    #[namespace = "secure_worker_pool"]
    choreography SecureWorkerPool {
        roles: Master, Worker, Monitor;

        [@guard_capability = "distribute_work", @flow_cost = 100]
        Master -> Worker: Task;

        [@guard_capability = "report_progress", @flow_cost = 50]
        Worker -> Monitor: Progress;

        [@journal_facts = "status_reported", @flow_cost = 30]
        Monitor -> Master: StatusReport;
    }
}
*/

fn main() {
    println!("=== Multi-Protocol Choreography Example ===\n");

    println!("This example demonstrates the intended syntax for aura_macros::choreography!");
    println!("with multiple protocols using namespaces to avoid type conflicts.\n");

    println!("=== Intended Features ===");
    println!("- Multiple choreographies in a single module using namespaces");
    println!("- Multi-role protocols: Master-Worker-Monitor pattern");
    println!("- Leader-follower consensus protocols");
    println!("- Namespace declarations: #[namespace = \"protocol_name\"]");
    println!("- Aura-specific annotations for capabilities and flow control");
    
    println!("\n=== Example Protocol Structures ===");
    println!("WorkerPool (namespace: worker_pool):");
    println!("  - Master distributes tasks to Worker");
    println!("  - Worker reports progress to Monitor");
    println!("  - Monitor sends status back to Master");
    println!();
    println!("DistributedConsensus (namespace: consensus):");
    println!("  - Leader proposes value to Follower");
    println!("  - Follower votes back to Leader");
    println!("  - Leader announces decision to Observer");

    println!("\n=== Aura Extension Syntax ===");
    println!("Statement-level annotations:");
    println!("  [@guard_capability = \"action_name\", @flow_cost = 100]");
    println!("  Sender -> Receiver: Message;");
    println!();
    println!("Role-specific annotations:");
    println!("  Sender[@flow_cost = 50] -> Receiver: Message;");
    println!();
    println!("Supported annotations:");
    println!("  * @guard_capability - Required capability for operations");
    println!("  * @flow_cost - Flow cost for communication");  
    println!("  * @journal_facts - Journal facts to record");
    println!("  * @journal_merge - Enable journal merge operations");
    
    println!("\n=== Current Implementation Status ===");
    println!("✓ Core macro infrastructure (rumpsteak-aura integration)");
    println!("✓ Extension registry and statement parser");
    println!("✓ Basic choreography syntax parsing");
    println!("⚠  Full annotation support (needs rumpsteak-aura type imports)");
    println!("⚠  Namespace collision handling");
    println!("⚠  Generated code compilation (missing type imports)");
    
    println!("\n=== Next Development Steps ===");
    println!("1. Fix annotation syntax parsing to match rumpsteak-aura grammar");
    println!("2. Implement proper ExtensionEffect traits for Aura features");
    println!("3. Add required type imports to generated code");
    println!("4. Test namespace support with multiple protocols");
    
    println!("\n=== Example Complete ===");
}
