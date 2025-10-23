//! Debug test to diagnose the tokio choreography hanging issue

use crate::Simulation;
use aura_coordination::{Instruction, EventFilter};

#[tokio::test]
async fn debug_tokio_hang() {
    println!("=== Starting debug test ===");
    
    // Create a simple simulation
    let mut sim = Simulation::new(42);
    
    // Create one participant
    let (_account_id, device_info) = sim.add_account_with_devices(&["alice"]).await;
    let alice_id = device_info[0].0;
    let alice_device_id = device_info[0].1;
    
    let alice = sim.get_participant(alice_id).unwrap();
    
    // Check initial state
    {
        let scheduler = sim.scheduler();
        let guard = scheduler.read().await;
        println!("Initial: active={}, waiting={}", 
            guard.active_context_count(), 
            guard.waiting_context_count()
        );
    }
    
    // Create a protocol context
    let session_id = sim.generate_uuid();
    let mut ctx = alice.create_protocol_context(
        session_id,
        vec![alice_device_id],
        Some(1),
    );
    
    // Check after context creation
    {
        let scheduler = sim.scheduler();
        let guard = scheduler.read().await;
        println!("After context creation: active={}, waiting={}", 
            guard.active_context_count(), 
            guard.waiting_context_count()
        );
    }
    
    // Try to execute a simple instruction
    println!("Executing AwaitThreshold...");
    let future = ctx.execute(Instruction::AwaitThreshold {
        count: 0,
        filter: EventFilter {
            session_id: None,
            event_types: None,
            authors: None,
            predicate: None,
        },
        timeout_epochs: None,
    });
    
    // Poll the future once
    use std::task::Poll;
    use futures::Future;
    
    let mut pinned = Box::pin(future);
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    
    match Future::poll(pinned.as_mut(), &mut cx) {
        Poll::Ready(result) => {
            println!("Instruction completed immediately: {:?}", result);
        }
        Poll::Pending => {
            println!("Instruction is pending");
            
            // Check scheduler state
            let scheduler = sim.scheduler();
            let guard = scheduler.read().await;
            println!("After first poll: active={}, waiting={}", 
                guard.active_context_count(), 
                guard.waiting_context_count()
            );
        }
    }
    
    // Advance simulation
    println!("Advancing simulation...");
    for i in 0..5 {
        sim.tick().await.unwrap();
        
        let scheduler = sim.scheduler();
        let guard = scheduler.read().await;
        println!("After tick {}: active={}, waiting={}", 
            i,
            guard.active_context_count(), 
            guard.waiting_context_count()
        );
    }
    
    println!("=== Test complete ===");
}
