//! Canonical encoding for domain facts.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Encoding used inside a fact envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactEncoding {
    /// DAG-CBOR encoding (canonical, deterministic).
    DagCbor,
    /// JSON encoding (primarily for debugging).
    Json,
}

/// Canonical envelope for domain fact payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactEnvelope {
    /// Domain fact type identifier (e.g., "chat", "invitation").
    pub type_id: String,
    /// Schema version for the encoded payload.
    pub schema_version: u16,
    /// Payload encoding format.
    pub encoding: FactEncoding,
    /// Encoded payload bytes.
    pub payload: Vec<u8>,
}

/// Delta produced by applying domain facts during reduction.
pub trait FactDelta: Default + Clone {
    /// Merge another delta into this one.
    fn merge(&mut self, other: &Self);
}

/// Reducer that maps domain facts to typed deltas.
pub trait FactDeltaReducer<F, D: FactDelta> {
    /// Apply a single fact and return its delta.
    fn apply(&self, fact: &F) -> D;

    /// Apply a fact into an existing delta.
    fn apply_into(&self, fact: &F, delta: &mut D) {
        let update = self.apply(fact);
        delta.merge(&update);
    }

    /// Reduce a batch of facts into a single delta.
    fn reduce_batch(&self, facts: &[F]) -> D {
        let mut delta = D::default();
        for fact in facts {
            self.apply_into(fact, &mut delta);
        }
        delta
    }
}

/// Encode a domain fact payload with a canonical envelope.
pub fn encode_domain_fact<T: Serialize>(type_id: &str, schema_version: u16, value: &T) -> Vec<u8> {
    let payload = crate::util::serialization::to_vec(value)
        .expect("DomainFact payload must serialize with DAG-CBOR");
    let envelope = FactEnvelope {
        type_id: type_id.to_string(),
        schema_version,
        encoding: FactEncoding::DagCbor,
        payload,
    };
    crate::util::serialization::to_vec(&envelope)
        .expect("DomainFact envelope must serialize with DAG-CBOR")
}

/// Decode a domain fact payload from a canonical envelope.
pub fn decode_domain_fact<T: DeserializeOwned>(
    expected_type_id: &str,
    expected_schema_version: u16,
    bytes: &[u8],
) -> Option<T> {
    let envelope: FactEnvelope = crate::util::serialization::from_slice(bytes).ok()?;
    if envelope.type_id != expected_type_id {
        return None;
    }
    if envelope.schema_version != expected_schema_version {
        return None;
    }
    match envelope.encoding {
        FactEncoding::DagCbor => crate::util::serialization::from_slice(&envelope.payload).ok(),
        FactEncoding::Json => serde_json::from_slice(&envelope.payload).ok(),
    }
}
