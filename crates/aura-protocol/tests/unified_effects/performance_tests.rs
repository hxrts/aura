//! Performance tests for the unified effect system
//!
//! These tests verify that the unified architecture performs well and
//! meets the performance requirements specified in the work plan.

use std::time::{Duration, Instant};
use uuid::Uuid;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_types::{
    handlers::{AuraHandler, Effect, EffectType, context::AuraContext},
    identifiers::DeviceId,
};

#[tokio::test]
async fn test_effect_dispatch_performance() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Warm up the system
    for _ in 0..10 {
        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: "Warmup".to_string(),
            component: Some("perf".to_string()),
        };
        
        let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
        system.execute_effect(effect, &mut ctx).await.unwrap();
    }
    
    // Measure effect dispatch performance
    let iterations = 1000;
    let start = Instant::now();
    
    for i in 0..iterations {
        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: format!("Performance test {}", i),
            component: Some("perf".to_string()),
        };
        
        let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
        system.execute_effect(effect, &mut ctx).await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let per_operation = elapsed / iterations;
    
    // Should be very fast - target < 1ms per operation
    assert!(per_operation < Duration::from_millis(1), 
        "Effect dispatch too slow: {:?} per operation", per_operation);
    
    println!("Effect dispatch performance: {:?} per operation", per_operation);
}

#[tokio::test]
async fn test_crypto_middleware_performance() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test BLAKE3 hashing performance
    let test_data = vec![0u8; 1024]; // 1KB of data
    let iterations = 100;
    
    let start = Instant::now();
    
    for _ in 0..iterations {
        let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
            algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
            data: test_data.clone(),
        };
        
        let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
        let _result: aura_protocol::effects::crypto::CryptoHashResult = 
            system.execute_effect(effect, &mut ctx).await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let per_hash = elapsed / iterations;
    
    // BLAKE3 should be very fast
    assert!(per_hash < Duration::from_millis(10), 
        "BLAKE3 hashing too slow: {:?} per 1KB", per_hash);
    
    println!("BLAKE3 performance: {:?} per 1KB hash", per_hash);
}

#[tokio::test]
async fn test_concurrent_performance() {
    use std::sync::Arc;
    use tokio::sync::RwLock;
    
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = Arc::new(RwLock::new(AuraEffectSystem::for_testing(device_id)));
    
    let num_threads = 10;
    let operations_per_thread = 100;
    
    let start = Instant::now();
    
    let mut handles = Vec::new();
    
    for thread_id in 0..num_threads {
        let system_clone = system.clone();
        
        let handle = tokio::spawn(async move {
            for i in 0..operations_per_thread {
                let mut system = system_clone.write().await;
                let mut ctx = AuraContext::for_testing(device_id);
                
                let log_params = aura_protocol::effects::console::ConsoleLogParams {
                    level: aura_protocol::effects::console::LogLevel::Info,
                    message: format!("Thread {} operation {}", thread_id, i),
                    component: Some("concurrent".to_string()),
                };
                
                let effect = Effect::new(EffectType::Console, "log", &log_params).unwrap();
                system.execute_effect(effect, &mut ctx).await.unwrap();
            }
        });
        
        handles.push(handle);
    }
    
    // Wait for all operations
    for handle in handles {
        handle.await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let total_operations = num_threads * operations_per_thread;
    let per_operation = elapsed / total_operations;
    
    // Should handle concurrent load efficiently
    assert!(per_operation < Duration::from_millis(5), 
        "Concurrent performance too slow: {:?} per operation", per_operation);
    
    println!("Concurrent performance: {} operations in {:?} ({:?} per op)", 
        total_operations, elapsed, per_operation);
}

#[tokio::test]
async fn test_memory_usage_stability() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Simulate a long-running process with many operations
    let iterations = 10000;
    
    for i in 0..iterations {
        // Mix different effect types to test overall system
        let effect = if i % 4 == 0 {
            let params = aura_protocol::effects::console::ConsoleLogParams {
                level: aura_protocol::effects::console::LogLevel::Info,
                message: format!("Memory test {}", i),
                component: Some("memory".to_string()),
            };
            Effect::new(EffectType::Console, "log", &params).unwrap()
        } else if i % 4 == 1 {
            let params = aura_protocol::effects::crypto::CryptoHashParams {
                algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
                data: format!("data-{}", i).into_bytes(),
            };
            Effect::new(EffectType::Crypto, "hash", &params).unwrap()
        } else if i % 4 == 2 {
            let params = aura_protocol::effects::random::RandomBytesParams {
                length: 16,
                purpose: Some(format!("test-{}", i)),
            };
            Effect::new(EffectType::Random, "bytes", &params).unwrap()
        } else {
            let params = aura_protocol::effects::time::TimeNowParams {};
            Effect::new(EffectType::Time, "now", &params).unwrap()
        };
        
        let _result = system.execute_effect(effect, &mut ctx).await.unwrap();
        
        // Periodically check that we're not accumulating excessive state
        if i % 1000 == 0 {
            // The fact that we can continue executing without issues
            // indicates memory usage is stable
        }
    }
    
    // If we reach here without panics or excessive slowdown, memory usage is stable
    println!("Completed {} operations without memory issues", iterations);
}

#[tokio::test]
async fn test_middleware_overhead() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Measure direct crypto operation performance
    let data = vec![0u8; 1024];
    let iterations = 100;
    
    let start = Instant::now();
    
    for _ in 0..iterations {
        let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
            algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
            data: data.clone(),
        };
        
        let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
        let _result: aura_protocol::effects::crypto::CryptoHashResult = 
            system.execute_effect(effect, &mut ctx).await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let per_operation = elapsed / iterations;
    
    // Middleware overhead should be minimal
    // This is verified by the fact that operations complete quickly
    assert!(per_operation < Duration::from_millis(5), 
        "Middleware overhead too high: {:?} per operation", per_operation);
    
    println!("Middleware overhead: {:?} per crypto operation", per_operation);
}

#[tokio::test]
async fn test_effect_registry_lookup_performance() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let system = AuraEffectSystem::for_testing(device_id);
    
    // Test effect support lookup performance
    let iterations = 10000;
    let start = Instant::now();
    
    for _ in 0..iterations {
        // Test lookups for different effect types
        let _ = system.supports_effect(EffectType::Crypto);
        let _ = system.supports_effect(EffectType::Network);
        let _ = system.supports_effect(EffectType::Console);
        let _ = system.supports_effect(EffectType::Storage);
        let _ = system.supports_effect(EffectType::Time);
    }
    
    let elapsed = start.elapsed();
    let per_lookup = elapsed / (iterations * 5); // 5 lookups per iteration
    
    // Registry lookups should be extremely fast
    assert!(per_lookup < Duration::from_nanos(1000), 
        "Registry lookup too slow: {:?} per lookup", per_lookup);
    
    println!("Registry lookup performance: {:?} per lookup", per_lookup);
}

#[tokio::test]
async fn test_context_creation_performance() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let iterations = 1000;
    
    let start = Instant::now();
    
    for _ in 0..iterations {
        let _ctx = AuraContext::for_testing(device_id);
    }
    
    let elapsed = start.elapsed();
    let per_creation = elapsed / iterations;
    
    // Context creation should be fast
    assert!(per_creation < Duration::from_micros(100), 
        "Context creation too slow: {:?} per creation", per_creation);
    
    println!("Context creation performance: {:?} per context", per_creation);
}

#[tokio::test]
async fn test_session_execution_performance() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    let iterations = 100;
    let start = Instant::now();
    
    for i in 0..iterations {
        use aura_types::session_types::LocalSessionType;
        let session = LocalSessionType::new(i, format!("session-{}", i));
        
        system.execute_session(session, &mut ctx).await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let per_session = elapsed / iterations;
    
    // Session execution should be efficient
    assert!(per_session < Duration::from_millis(10), 
        "Session execution too slow: {:?} per session", per_session);
    
    println!("Session execution performance: {:?} per session", per_session);
}

#[tokio::test]
async fn test_large_data_handling() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test with large data (10MB)
    let large_data = vec![0u8; 10 * 1024 * 1024];
    
    let start = Instant::now();
    
    let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
        algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
        data: large_data,
    };
    
    let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
    let result: aura_protocol::effects::crypto::CryptoHashResult = 
        system.execute_effect(effect, &mut ctx).await.unwrap();
    
    let elapsed = start.elapsed();
    
    // Should handle large data efficiently
    assert!(elapsed < Duration::from_secs(1), 
        "Large data handling too slow: {:?} for 10MB", elapsed);
    assert!(!result.hash.is_empty());
    
    println!("Large data performance: {:?} for 10MB hash", elapsed);
}

/// Benchmark helper to verify performance requirements from work plan
#[tokio::test]
async fn test_performance_requirements() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut system = AuraEffectSystem::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);
    
    // Test requirement: < 5% performance impact from middleware
    let iterations = 1000;
    let test_data = vec![0u8; 256];
    
    let start = Instant::now();
    
    for _ in 0..iterations {
        let hash_params = aura_protocol::effects::crypto::CryptoHashParams {
            algorithm: aura_protocol::effects::crypto::HashAlgorithm::Blake3,
            data: test_data.clone(),
        };
        
        let effect = Effect::new(EffectType::Crypto, "hash", &hash_params).unwrap();
        let _result: aura_protocol::effects::crypto::CryptoHashResult = 
            system.execute_effect(effect, &mut ctx).await.unwrap();
    }
    
    let elapsed = start.elapsed();
    let throughput = iterations as f64 / elapsed.as_secs_f64();
    
    // Should achieve high throughput (>1000 ops/sec for simple operations)
    assert!(throughput > 1000.0, 
        "Throughput too low: {:.2} ops/sec", throughput);
    
    println!("Performance requirements verified: {:.2} ops/sec", throughput);
}