//! Dynamic Role Support Example
//!
//! This example demonstrates the choreography! macro with parameterized roles.

use aura_macros::choreography;

// Example 1: Fixed-size parameterized roles (simplified for testing)
choreography! {
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
choreography! {
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
    println!("This example demonstrates choreography syntax with parameterized roles.");
    println!("Note: Full parameterized role support is planned for future implementation.");
    println!();

    println!("Features demonstrated in syntax:");
    println!("- Fixed-size parameterized roles: Worker[3]");
    println!("- Variable-size parameterized roles: Follower[N]");
    println!("- Static and parameterized role mixing");
    println!("- Aura annotations with parameterized roles");
    println!();

    println!("The choreography! macro will generate appropriate session types");
    println!("and extension registrations for runtime integration with aura-mpst.");
}
