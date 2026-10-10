//! Exact finite input domains shared by Kani refinement and native domain checks.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Transition {
    ApplyShare,
    TriggerFallback,
    FailConsensus,
}

pub(super) fn transition(choice: u8) -> Option<Transition> {
    match choice {
        0 => Some(Transition::ApplyShare),
        1 => Some(Transition::TriggerFallback),
        2 => Some(Transition::FailConsensus),
        _ => None,
    }
}

pub(super) fn bindings(identity: bool, operation: bool, prestate: bool) -> (u8, u8, u8) {
    (
        if identity { 22 } else { 21 },
        if operation { 32 } else { 31 },
        if prestate { 42 } else { 41 },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinement_binding_choices_cover_exact_original_cartesian_domain() {
        let mut observed = std::collections::BTreeSet::new();
        for identity in [false, true] {
            for operation in [false, true] {
                for prestate in [false, true] {
                    observed.insert(bindings(identity, operation, prestate));
                }
            }
        }
        let expected = (21..=22)
            .flat_map(|identity| {
                (31..=32).flat_map(move |operation| {
                    (41..=42).map(move |prestate| (identity, operation, prestate))
                })
            })
            .collect();
        assert_eq!(observed, expected);
    }

    #[test]
    fn refinement_transition_choices_cover_all_and_refuse_invalid_selectors() {
        assert_eq!(transition(0), Some(Transition::ApplyShare));
        assert_eq!(transition(1), Some(Transition::TriggerFallback));
        assert_eq!(transition(2), Some(Transition::FailConsensus));
        for invalid in 3..=u8::MAX {
            assert_eq!(transition(invalid), None);
        }
    }
}
