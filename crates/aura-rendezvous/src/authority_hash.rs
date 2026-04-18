use aura_core::hash;
use aura_core::types::identifiers::AuthorityId;

/// Convert an AuthorityId to a 32-byte hash for commitment/indexing purposes.
pub(crate) fn authority_hash_bytes(authority: &AuthorityId) -> [u8; 32] {
    hash::hash(&authority.to_bytes())
}
