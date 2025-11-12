//! Dynamic Role Support Example
//!
//! This example demonstrates the aura_choreography! macro with parameterized roles.

use aura_macros::aura_choreography;

// Example 1: Fixed-size parameterized roles (simplified for testing)
aura_choreography! {
    #[namespace = "worker_pool"]
    protocol WorkerPool {
        roles: Master, Worker[3], Monitor;
        
        // Master distributes work to first worker  
        Master[guard_capability = "distribute_work",
               flow_cost = 100] 
        -> Worker: Task(String);
        
        // Worker reports to monitor
        Worker[guard_capability = "report_progress",
               flow_cost = 50] 
        -> Monitor: Progress(String);
        
        // Monitor reports to master
        Monitor[guard_capability = "aggregate_progress",
                flow_cost = 75,
                journal_facts = "progress_aggregated"] 
        -> Master: StatusReport(String);
    }
}

// Example 2: Variable-size parameterized roles (basic test)
aura_choreography! {
    #[namespace = "distributed_consensus"]
    protocol DistributedConsensus {
        roles: Leader, Follower[N], Observer;
        
        // Simple leader-follower communication
        Leader[guard_capability = "propose_value"] 
        -> Follower: Proposal(String);
        
        Follower[guard_capability = "cast_vote"] 
        -> Leader: Vote(String);
        
        Leader[journal_facts = "consensus_reached"] 
        -> Observer: Decision(String);
    }
}

fn main() {
    println!("=== Dynamic Role Support Examples ===");
    println!();
    
    // Test Example 1: Worker Pool
    println!("1. Worker Pool Protocol:");
    let worker_pool = worker_pool::WorkerPool::new();
    let roles = worker_pool::WorkerPool::roles();
    println!("   - Protocol ID: {}", worker_pool.protocol_id);
    println!("   - Roles: {:?}", roles.iter().map(|r| r.name()).collect::<Vec<_>>());
    
    // Test parameterized role instantiation (Worker[3])
    if let Ok(worker0) = worker_pool::Worker::new(0) {
        println!("   - Worker[0]: {}", worker0);
    }
    if let Ok(worker2) = worker_pool::Worker::new(2) {
        println!("   - Worker[2]: {}", worker2);
    }
    
    // Test boundary checking
    if let Err(error) = worker_pool::Worker::new(5) {
        println!("   - Boundary check: {}", error);
    }
    
    match worker_pool::execute_protocol() {
        Ok(result) => println!("   - Execution: {}", result),
        Err(e) => println!("   - Error: {}", e),
    }
    
    println!();
    
    // Test Example 2: Distributed Consensus  
    println!("2. Distributed Consensus Protocol:");
    let consensus = distributed_consensus::DistributedConsensus::new();
    let roles = distributed_consensus::DistributedConsensus::roles();
    println!("   - Protocol ID: {}", consensus.protocol_id);
    println!("   - Roles: {:?}", roles.iter().map(|r| r.name()).collect::<Vec<_>>());
    
    match distributed_consensus::execute_protocol() {
        Ok(result) => println!("   - Execution: {}", result),
        Err(e) => println!("   - Error: {}", e),
    }
    
    println!();
    println!("✅ Dynamic role support implemented successfully!");
    println!();
    println!("Features demonstrated:");
    println!("- Fixed-size parameterized roles: Worker[3]");
    println!("- Variable-size parameterized roles: Follower[N]");  
    println!("- Role instance creation with bounds checking");
    println!("- Static and parameterized role mixing");
    println!("- Aura annotations with parameterized roles");
}