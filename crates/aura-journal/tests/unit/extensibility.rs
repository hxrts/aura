use aura_core::types::identifiers::ContextId;
use aura_journal::extensibility::{FactEncoding, FactEnvelope};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{parse_envelope, DomainFact, FactReducer, FactRegistry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum TestFact {
    Created { id: String },
    Updated { id: String, value: u32 },
}

impl DomainFact for TestFact {
    fn type_id(&self) -> &'static str {
        "test"
    }

    fn context_id(&self) -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn to_envelope(&self) -> FactEnvelope {
        #[allow(clippy::expect_used)]
        let payload = aura_core::util::serialization::to_vec(self).expect("TestFact serialization");
        FactEnvelope {
            type_id: aura_core::types::facts::FactTypeId::from(self.type_id()),
            schema_version: self.schema_version(),
            encoding: FactEncoding::DagCbor,
            payload,
        }
    }

    fn from_envelope(envelope: &FactEnvelope) -> Option<Self> {
        if envelope.type_id.as_str() != "test" {
            return None;
        }
        aura_core::util::serialization::from_slice(&envelope.payload).ok()
    }
}

struct TestFactReducer;

impl FactReducer for TestFactReducer {
    fn handles_type(&self) -> &'static str {
        "test"
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != "test" {
            return None;
        }

        let fact: TestFact = TestFact::from_envelope(envelope)?;
        let id = match &fact {
            TestFact::Created { id } => id.clone(),
            TestFact::Updated { id, .. } => id.clone(),
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic("test".to_string()),
            context_id,
            data: id.into_bytes(),
        })
    }
}

#[test]
fn domain_fact_envelope_roundtrip() {
    let fact = TestFact::Created {
        id: "abc".to_string(),
    };
    let envelope = fact.to_envelope();
    let restored = TestFact::from_envelope(&envelope);
    assert_eq!(restored, Some(fact));
}

#[test]
fn to_generic() {
    let fact = TestFact::Created {
        id: "abc".to_string(),
    };
    let generic = fact.to_generic();

    if let aura_journal::RelationalFact::Generic { envelope, .. } = generic {
        assert_eq!(envelope.type_id.as_str(), "test");
        let restored = TestFact::from_envelope(&envelope);
        assert!(restored.is_some());
    } else {
        panic!("Expected Generic variant");
    }
}

#[test]
fn registry() {
    let mut registry = FactRegistry::new();
    registry.register::<TestFact>("test", Box::new(TestFactReducer));

    assert!(registry.is_registered("test"));
    assert!(!registry.is_registered("unknown"));
}

#[test]
fn registry_reduce_envelope() {
    let mut registry = FactRegistry::new();
    registry.register::<TestFact>("test", Box::new(TestFactReducer));

    let fact = TestFact::Created {
        id: "xyz".to_string(),
    };
    let context_id = fact.context_id();
    let envelope = fact.to_envelope();

    let binding = registry.reduce_envelope(context_id, &envelope);
    assert_eq!(binding.data, b"xyz".to_vec());
}

#[test]
fn parse_typed_envelope() {
    let fact = TestFact::Updated {
        id: "foo".to_string(),
        value: 42,
    };
    let envelope = fact.to_envelope();

    let restored: Option<TestFact> = parse_envelope(&envelope, "test");
    assert!(restored.is_some());

    let mut wrong_envelope = envelope;
    wrong_envelope.type_id = aura_core::types::facts::FactTypeId::from("wrong");
    let wrong_type: Option<TestFact> = parse_envelope(&wrong_envelope, "test");
    assert!(wrong_type.is_none());
}
