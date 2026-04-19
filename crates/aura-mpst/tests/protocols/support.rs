use aura_core::capability_name;
use aura_mpst::extensions::CompositeExtension;
use aura_mpst::upstream::language::compile_choreography;
use aura_mpst::{lower_aura_effects, AuraEffect, RoleId};

pub fn compile_and_lower(
    choreography: &str,
) -> Result<Vec<AuraEffect>, Box<dyn std::error::Error>> {
    let compiled = compile_choreography(choreography)?;
    Ok(lower_aura_effects(&compiled)?)
}

pub fn guard_chain_composite(
    role: &str,
    operation: &str,
    cost: u64,
    fact: &str,
) -> CompositeExtension {
    CompositeExtension::new(RoleId::new(role), operation.to_string())
        .with_capability_guard(capability_name!("chat:message:send"))
        .with_flow_cost(cost)
        .with_journal_fact(fact.to_string())
}
