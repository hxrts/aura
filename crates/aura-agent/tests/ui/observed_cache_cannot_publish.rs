use aura_agent::AuraEffectSystem;

fn publish_observed_cache(effects: &AuraEffectSystem) {
    if let Some(observed) = effects.biscuit_cache() {
        effects.set_biscuit_cache(observed);
    }
}

fn main() {}
