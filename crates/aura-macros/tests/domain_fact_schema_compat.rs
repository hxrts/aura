use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
use aura_core::types::identifiers::ContextId;
use aura_journal::DomainFact as _;
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "tests/domain-fact-schema-compat",
    schema_version = 2,
    min_supported_schema_version = 1,
    context = "context_id"
)]
struct CompatFact {
    context_id: ContextId,
    value: u64,
}

#[test]
fn domain_fact_from_envelope_accepts_supported_older_schema_versions() {
    let fact = CompatFact {
        context_id: ContextId::new_from_entropy([7u8; 32]),
        value: 42,
    };
    let envelope = FactEnvelope {
        type_id: FactTypeId::from("tests/domain-fact-schema-compat"),
        schema_version: 1,
        encoding: FactEncoding::DagCbor,
        payload: aura_core::util::serialization::to_vec(&fact)
            .unwrap_or_else(|error| panic!("serialize fact: {error}")),
    };

    let decoded = CompatFact::from_envelope(&envelope)
        .unwrap_or_else(|| panic!("decode supported older schema"));
    assert_eq!(decoded, fact);
}
