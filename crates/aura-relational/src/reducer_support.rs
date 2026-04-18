use aura_core::time::PhysicalTime;
use aura_core::types::facts::FactEnvelope;
use aura_core::types::{AuthorityId, ContextId};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::DomainFact;

pub(crate) fn physical_time_ms(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

pub(crate) fn parse_typed_envelope<F: DomainFact>(
    envelope: &FactEnvelope,
    expected_type_id: &str,
) -> Option<F> {
    if envelope.type_id.as_str() != expected_type_id {
        return None;
    }
    F::from_envelope(envelope)
}

pub(crate) fn reduce_typed_envelope<F: DomainFact>(
    _context_id: ContextId,
    envelope: &FactEnvelope,
    expected_type_id: &str,
    validate: impl FnOnce(&F) -> bool,
    build_binding: impl FnOnce(F) -> RelationalBinding,
) -> Option<RelationalBinding> {
    let fact = parse_typed_envelope::<F>(envelope, expected_type_id)?;
    if !validate(&fact) {
        return None;
    }
    Some(build_binding(fact))
}

pub(crate) fn hashed_generic_binding(
    sub_type: &'static str,
    context_id: ContextId,
    payload: &[u8],
) -> RelationalBinding {
    RelationalBinding {
        binding_type: RelationalBindingType::Generic(sub_type.to_string()),
        context_id,
        data: aura_core::hash::hash(payload).to_vec(),
    }
}

pub(crate) fn stable_authority_pair_bytes(first: AuthorityId, second: AuthorityId) -> Vec<u8> {
    let mut a = first.to_bytes();
    let mut b = second.to_bytes();
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    let mut data = a.to_vec();
    data.extend_from_slice(&b);
    data
}
