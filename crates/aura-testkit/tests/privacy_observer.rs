//! Tests for privacy observer functionality

use aura_testkit::privacy::{simulate_rendezvous_trace, traces_equivalent, PrivacyObserver};

#[test]
fn observer_traces_are_indistinguishable() {
    let trace_a = simulate_rendezvous_trace(5);
    let trace_b = simulate_rendezvous_trace(5);

    assert!(
        traces_equivalent(&trace_a, &trace_b),
        "external observer should not distinguish swapped contexts"
    );
}

#[test]
fn observer_records_envelopes() {
    let events = simulate_rendezvous_trace(3);
    let mut observer = PrivacyObserver::new();
    for event in events.clone() {
        observer.observe(event);
    }
    assert_eq!(observer.trace(), events.as_slice());
}
