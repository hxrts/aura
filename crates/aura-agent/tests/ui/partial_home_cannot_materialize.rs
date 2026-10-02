use aura_agent::reactive::JoinedHomeEvidence;

fn fabricate<T>() -> T {
    panic!("partial facts cannot supply home creation evidence")
}

fn main() {
    // A member event or raw IDs do not carry the verified checkpoint and
    // participant evidence needed to create a home projection.
    let _ = JoinedHomeEvidence {
        verified: fabricate(),
        creation: fabricate(),
    };
}
