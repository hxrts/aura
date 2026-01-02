use aura_core::types::facts::{try_decode_fact, try_encode_fact, FactError, FactTypeId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestFact {
    id: u64,
    payload: Vec<u8>,
}

const REGISTERED_FACTS: &[(&str, u16)] = &[("test/v1", 1), ("test/v2", 2)];

#[test]
fn round_trip_registered_fact_envelopes() {
    for (type_id, version) in REGISTERED_FACTS {
        let fact = TestFact {
            id: *version as u64,
            payload: vec![1, 2, 3, *version as u8],
        };
        let bytes = try_encode_fact(&FactTypeId::new(type_id), *version, &fact)
            .expect("encode should succeed");
        let decoded: TestFact =
            try_decode_fact(&FactTypeId::new(type_id), *version, &bytes)
                .expect("decode should succeed");
        assert_eq!(decoded, fact);
    }
}

#[test]
fn rejects_version_mismatch() {
    let fact = TestFact {
        id: 1,
        payload: vec![0, 1, 2],
    };
    let bytes = try_encode_fact(&FactTypeId::new("test/v1"), 1, &fact)
        .expect("encode should succeed");
    let err = try_decode_fact::<TestFact>(&FactTypeId::new("test/v1"), 2, &bytes)
        .expect_err("expected version mismatch");
    assert!(matches!(err, FactError::VersionMismatch { .. }));
}

#[test]
fn rejects_type_mismatch() {
    let fact = TestFact {
        id: 9,
        payload: vec![9, 9, 9],
    };
    let bytes = try_encode_fact(&FactTypeId::new("test/v1"), 1, &fact)
        .expect("encode should succeed");
    let err = try_decode_fact::<TestFact>(&FactTypeId::new("other/v1"), 1, &bytes)
        .expect_err("expected type mismatch");
    assert!(matches!(err, FactError::TypeMismatch { .. }));
}
