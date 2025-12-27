#[cfg(not(feature = "simulation"))]
#[test]
fn default_features_are_minimal() {
    assert!(
        !cfg!(feature = "simulation"),
        "simulation feature must be opt-in for deterministic production behavior"
    );
}
