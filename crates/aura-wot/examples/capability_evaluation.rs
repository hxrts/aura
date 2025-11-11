//! Example: Capability Evaluation and Policy Enforcement
//!
//! This example demonstrates how to use the aura-wot capability system
//! for authorization decisions following meet-semilattice laws.

use aura_core::identifiers::DeviceId;
use aura_wot::{
    evaluate_capabilities, Capability, CapabilitySet, DelegationChain, DelegationLink,
    EvaluationContext, LocalChecks, Policy,
};
use std::collections::BTreeSet;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Aura WoT Capability Evaluation Example ===\n");

    // 1. Create base capabilities for a user
    let base_capabilities = CapabilitySet::from_capabilities(
        [
            Capability::Read {
                resource_pattern: "journal:*".to_string(),
            },
            Capability::Write {
                resource_pattern: "journal:personal/*".to_string(),
            },
            Capability::Execute {
                operation: "sync".to_string(),
            },
            Capability::Delegate { max_depth: 2 },
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
    );

    println!("Base capabilities: {:#?}\n", base_capabilities);

    // 2. Create a delegation that restricts capabilities
    let delegation = CapabilitySet::from_capabilities(
        [
            Capability::Read {
                resource_pattern: "journal:*".to_string(),
            },
            Capability::Write {
                resource_pattern: "journal:shared/*".to_string(),
            }, // More restrictive
               // No execute capability - further restriction
        ]
        .into_iter()
        .collect::<BTreeSet<_>>(),
    );

    println!("Delegation restrictions: {:#?}\n", delegation);

    // 3. Apply meet-semilattice intersection (capabilities can only shrink)
    let effective_capabilities = base_capabilities.meet(&delegation);

    println!(
        "Effective capabilities (after meet ⊓): {:#?}\n",
        effective_capabilities
    );

    // 4. Demonstrate that capabilities only shrink via meet
    println!("=== Meet-Semilattice Properties ===");

    // Idempotency: a ⊓ a = a
    let idempotent = base_capabilities.meet(&base_capabilities);
    println!(
        "Idempotency check: base ⊓ base == base? {}",
        idempotent == base_capabilities
    );

    // Commutativity: a ⊓ b = b ⊓ a
    let ab = base_capabilities.meet(&delegation);
    let ba = delegation.meet(&base_capabilities);
    println!(
        "Commutativity check: (base ⊓ delegation) == (delegation ⊓ base)? {}",
        ab == ba
    );

    // Monotonicity: result ⪯ inputs (result is subset of both)
    println!(
        "Monotonicity check: effective ⪯ base? {}",
        effective_capabilities.is_subset_of(&base_capabilities)
    );
    println!(
        "Monotonicity check: effective ⪯ delegation? {}",
        effective_capabilities.is_subset_of(&delegation)
    );

    println!("\n=== Policy Evaluation Example ===");

    // 5. Create a policy with device-specific restrictions
    let device_id = DeviceId::new();
    let mut policy = Policy::new();
    policy.set_device_capabilities(device_id, effective_capabilities.clone());

    // 6. Evaluate effective capabilities with local checks
    let context = EvaluationContext::new(device_id, "read_context".to_string());
    let local_checks = LocalChecks::empty();

    let evaluation_result = evaluate_capabilities(&policy, &[], &local_checks, &context)?;

    println!("Policy-constrained capabilities: {:#?}", evaluation_result);

    // 7. Test specific permission checks
    println!("\n=== Permission Checks ===");

    let journal_read = "journal:personal/diary";
    let journal_write = "journal:shared/notes";
    let execute_sync = "sync";

    println!(
        "Can read '{}'? {}",
        journal_read,
        check_permission(&evaluation_result, "read", journal_read)
    );
    println!(
        "Can write '{}'? {}",
        journal_write,
        check_permission(&evaluation_result, "write", journal_write)
    );
    println!(
        "Can execute '{}'? {}",
        execute_sync,
        check_execute_permission(&evaluation_result, execute_sync)
    );

    println!("\n=== Capability Delegation Chain ===");

    // 8. Demonstrate delegation chains with proper attenuation
    let mut delegation_chain = DelegationChain::new();

    // Add delegation link that further restricts capabilities
    let user1 = DeviceId::new();
    let user2 = DeviceId::new();
    let delegation_link = DelegationLink::new(
        user1,
        user2,
        CapabilitySet::from_capabilities(
            [
                Capability::Read {
                    resource_pattern: "journal:shared/*".to_string(),
                }, // Even more restrictive
            ]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        ),
        1, // Max delegation depth
    );
    delegation_chain.add_delegation(delegation_link)?;

    // Apply delegation chain (should further restrict capabilities)
    let final_capabilities = delegation_chain.effective_capabilities(&effective_capabilities);

    println!("After delegation chain: {:#?}", final_capabilities);
    println!(
        "Final capability count: {}",
        final_capabilities.capabilities().count()
    );

    // Verify that delegation only made things more restrictive
    println!(
        "Delegation chain preserved monotonicity? {}",
        final_capabilities.is_subset_of(&effective_capabilities)
    );

    Ok(())
}

/// Check if a capability set permits a specific read operation
fn check_permission(caps: &CapabilitySet, operation: &str, resource: &str) -> bool {
    for cap in caps.capabilities() {
        match (operation, cap) {
            ("read", Capability::Read { resource_pattern }) => {
                if resource_matches(resource, resource_pattern) {
                    return true;
                }
            }
            ("write", Capability::Write { resource_pattern }) => {
                if resource_matches(resource, resource_pattern) {
                    return true;
                }
            }
            _ => continue,
        }
    }
    false
}

/// Check if a capability set permits executing an operation
fn check_execute_permission(caps: &CapabilitySet, operation: &str) -> bool {
    for cap in caps.capabilities() {
        if let Capability::Execute { operation: op } = cap {
            if op == operation {
                return true;
            }
        }
    }
    false
}

/// Simple pattern matching for resource permissions
fn resource_matches(resource: &str, pattern: &str) -> bool {
    if pattern.ends_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        resource.starts_with(prefix)
    } else {
        resource == pattern
    }
}
