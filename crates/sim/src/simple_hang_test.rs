//! Minimal test to reproduce the hanging issue

use crate::Simulation;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_until_idle_with_protocols() {
    println!("=== Testing run_until_idle with active protocols ===");
    
    let mut sim = Simulation::new(42);
    
    // Create a shared account with three devices
    let (_account_id, device_info) = sim
        .add_account_with_devices(&["alice", "bob", "carol"])
        .await;
    
    let alice = device_info[0].0;
    let bob = device_info[1].0;
    let carol = device_info[2].0;
    
    let alice_device_id = device_info[0].1;
    let bob_device_id = device_info[1].1;
    let carol_device_id = device_info[2].1;
    
    let participants = vec![alice_device_id, bob_device_id, carol_device_id];
    
    // Get participants
    let alice_participant = sim.get_participant(alice).unwrap();
    let bob_participant = sim.get_participant(bob).unwrap();
    let carol_participant = sim.get_participant(carol).unwrap();
    
    let session_id = sim.generate_uuid();
    
    println!("Starting DKD protocols for all participants...");
    
    // Check scheduler state before starting
    {
        let scheduler_ref = sim.scheduler();
        let scheduler = scheduler_ref.read().await;
        println!("Before starting - Active contexts: {}, Waiting contexts: {}", 
                 scheduler.active_context_count(), 
                 scheduler.waiting_context_count());
    }
    
    // Start DKD protocols for all participants
    let alice_dkd = alice_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
    let bob_dkd = bob_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
    let carol_dkd = carol_participant.initiate_dkd_with_session(session_id, participants.clone(), 2);
    
    // Give async tasks a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    
    // Check scheduler state after starting
    {
        let scheduler_ref = sim.scheduler();
        let scheduler = scheduler_ref.read().await;
        println!("After starting - Active contexts: {}, Waiting contexts: {}", 
                 scheduler.active_context_count(), 
                 scheduler.waiting_context_count());
    }
    
    println!("Checking if simulation is idle...");
    println!("Is idle: {}", sim.is_idle().await);
    
    // Use select! to race between simulation and protocols
    tokio::select! {
        // Try to run simulation
        result = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            sim.run_until_idle()
        ) => {
            match result {
                Ok(Ok(ticks)) => println!("Simulation completed after {} ticks", ticks),
                Ok(Err(e)) => println!("Simulation error: {:?}", e),
                Err(_) => println!("Simulation run_until_idle timed out!"),
            }
        }
        
        // Or wait for any protocol to complete/timeout
        _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            alice_dkd
        ) => {
            println!("Alice DKD finished or timed out");
        }
        
        _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            bob_dkd
        ) => {
            println!("Bob DKD finished or timed out");
        }
        
        _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            carol_dkd
        ) => {
            println!("Carol DKD finished or timed out");
        }
    }
    
    println!("Test completed - all tasks finished or timed out");
}