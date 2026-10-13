//! Root-verified Biscuit integrity shared by foundational capability algebra
//! and higher-layer authorization. Verification establishes signature integrity
//! under the supplied root, not that the caller selected a trusted authority.
use biscuit_auth::{Biscuit, PublicKey};

#[derive(Clone, Debug)]
pub struct VerifiedBiscuitToken {
    token: Biscuit,
}
impl VerifiedBiscuitToken {
    /// Verify serialized bytes under the independently selected root key.
    pub fn from_bytes(bytes: &[u8], root: PublicKey) -> Result<Self, biscuit_auth::error::Token> {
        let token = Biscuit::from(bytes, root)?;
        Ok(Self { token })
    }

    /// Borrow the token whose complete signature chain was verified.
    pub fn token(&self) -> &Biscuit {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use biscuit_auth::KeyPair;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn root_verified_token_rejects_wrong_root_and_malformed_bytes() {
        let mut rng = StdRng::from_seed(crate::hash::hash(
            b"aura-core.verified-biscuit.root-verification-negative.v1",
        ));
        let root = KeyPair::new_with_rng(&mut rng);
        let other = KeyPair::new_with_rng(&mut rng);
        let token = Biscuit::builder()
            .build_with_rng(&root, Default::default(), &mut rng)
            .unwrap();
        let bytes = token.to_vec().unwrap();
        assert!(VerifiedBiscuitToken::from_bytes(&bytes, root.public()).is_ok());
        assert!(VerifiedBiscuitToken::from_bytes(&bytes, other.public()).is_err());
        let mut tampered = bytes.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(VerifiedBiscuitToken::from_bytes(&tampered, root.public()).is_err());
        assert!(VerifiedBiscuitToken::from_bytes(b"not a biscuit", root.public()).is_err());
    }
}
