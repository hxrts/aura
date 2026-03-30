use aura_agent::VerifiedServiceWitness;
use aura_core::ServiceFamily;

fn main() {
    let _ = VerifiedServiceWitness {
        family: ServiceFamily::Hold,
        providers: Vec::new(),
        observed_at_ms: 0,
        success: true,
        outstanding_hold_delta: 0,
        _sealed: (),
    };
}
