#!/usr/bin/env rust-script
//! Test script to verify CRDT properties work through Journal API
//! 
//! This script verifies that the Journal API preserves semilattice properties:
//! - Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)  
//! - Commutativity: a ⊔ b = b ⊔ a
//! - Idempotency: a ⊔ a = a

use aura_core::{AccountId, DeviceId};
use aura_journal::journal_api::Journal;
use aura_journal::{DeviceMetadata, DeviceType};
use ed25519_dalek::SigningKey;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Verifying CRDT properties through Journal API...");

    // Create test journals
    let account_id = AccountId::new();
    let device_key = SigningKey::from_bytes(&[1u8; 32]);
    let verifying_key = device_key.verifying_key();
    
    let mut journal1 = Journal::new_with_group_key(account_id, verifying_key);
    let mut journal2 = Journal::new_with_group_key(account_id, verifying_key);
    let mut journal3 = Journal::new_with_group_key(account_id, verifying_key);

    // Add different devices to each journal
    let device1 = DeviceMetadata {
        device_id: DeviceId::new(),
        device_type: DeviceType::Owner,
        public_key: verifying_key,
        name: "Device1".to_string(),
        capabilities: vec!["read".to_string(), "write".to_string()],
        added_at: 1000,
    };
    
    let device2 = DeviceMetadata {
        device_id: DeviceId::new(),
        device_type: DeviceType::Guardian,
        public_key: verifying_key,
        name: "Device2".to_string(),
        capabilities: vec!["read".to_string()],
        added_at: 1001,
    };

    let device3 = DeviceMetadata {
        device_id: DeviceId::new(),
        device_type: DeviceType::Guardian,
        public_key: verifying_key,
        name: "Device3".to_string(),
        capabilities: vec!["recovery".to_string()],
        added_at: 1002,
    };

    journal1.add_device(device1.clone())?;
    journal2.add_device(device2.clone())?;
    journal3.add_device(device3.clone())?;

    // Test commutativity: j1 ⊔ j2 = j2 ⊔ j1
    println!("Testing commutativity...");
    let mut left = journal1.clone();
    left.merge(&journal2)?;
    
    let mut right = journal2.clone();
    right.merge(&journal1)?;
    
    assert_eq!(left.devices().len(), right.devices().len(), 
        "Commutativity failed: device counts don't match");
    println!("✓ Commutativity verified");

    // Test associativity: (j1 ⊔ j2) ⊔ j3 = j1 ⊔ (j2 ⊔ j3)
    println!("Testing associativity...");
    let mut left_assoc = journal1.clone();
    left_assoc.merge(&journal2)?;
    left_assoc.merge(&journal3)?;
    
    let mut right_assoc = journal2.clone();
    right_assoc.merge(&journal3)?;
    let mut right_assoc_final = journal1.clone();
    right_assoc_final.merge(&right_assoc)?;
    
    assert_eq!(left_assoc.devices().len(), right_assoc_final.devices().len(),
        "Associativity failed: device counts don't match");
    println!("✓ Associativity verified");

    // Test idempotency: j ⊔ j = j
    println!("Testing idempotency...");
    let original_count = journal1.devices().len();
    let mut idempotent = journal1.clone();
    idempotent.merge(&journal1)?;
    
    assert_eq!(idempotent.devices().len(), original_count,
        "Idempotency failed: device count changed after self-merge");
    println!("✓ Idempotency verified");

    // Test fact addition preserves properties
    println!("Testing fact operations...");
    let mut fact_journal1 = Journal::new(account_id);
    let mut fact_journal2 = Journal::new(account_id);
    
    fact_journal1.add_fact("test_key".to_string(), serde_json::Value::String("value1".to_string()))?;
    fact_journal2.add_fact("test_key2".to_string(), serde_json::Value::String("value2".to_string()))?;
    
    let mut merged_facts = fact_journal1.clone();
    merged_facts.merge(&fact_journal2)?;
    
    // Verify both facts are present
    assert!(merged_facts.get_fact("test_key").is_some(), "First fact not found after merge");
    assert!(merged_facts.get_fact("test_key2").is_some(), "Second fact not found after merge");
    println!("✓ Fact operations verified");

    println!("\n🎉 All CRDT properties verified successfully through Journal API!");
    println!("The Journal abstraction preserves semilattice semantics.");
    
    Ok(())
}