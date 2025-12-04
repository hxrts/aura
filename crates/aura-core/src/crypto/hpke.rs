//! HPKE (Hybrid Public Key Encryption) key types

use serde::{Deserialize, Serialize};

/// HPKE key types (X25519 serialized byte representation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkePublicKey(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkePrivateKey(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkeKeyPair {
    pub public: HpkePublicKey,
    pub private: HpkePrivateKey,
}

impl HpkeKeyPair {
    pub fn new(public: Vec<u8>, private: Vec<u8>) -> Self {
        Self {
            public: HpkePublicKey(public),
            private: HpkePrivateKey(private),
        }
    }
}
