//! Owner-minted sequences, nonces and commit keys (docs/122 "Replay
//! Protection", type layer).
//!
//! A sequence value is minted only by its owner and consumed once:
//!
//! - [`SequenceOwner<D>`] is the in-process, `ActorOwned` source of
//!   [`Admission<D>`] for domain `D`. It is not `Clone`.
//! - [`Admission<D>`] is `MoveOwned`: not `Clone` or `Copy`, private fields,
//!   `#[must_use]`. It names exactly one sequence value, and consuming it
//!   yields that value once.
//! - [`DurableSequenceOwner<D>`] reserves the next value and persists the
//!   advanced counter to its [`SequenceStore`] under its own lock before
//!   handing out the admission. Concurrent callers never share a value, and a
//!   restart never reissues one.
//! - [`NonceOwner<K>`] builds every 96-bit AEAD nonce for key `K` itself.
//! - [`CommitKey`] is the owner-minted idempotency key for one fact commit.
//!
//! Forging, reusing or cloning an admission does not compile:
//!
//! ```compile_fail,E0451
//! use aura_core::ownership::sequence::Admission;
//! struct Frames;
//! let _forged = Admission::<Frames> { value: 7, domain: std::marker::PhantomData };
//! ```
//!
//! ```compile_fail,E0599
//! use aura_core::ownership::sequence::Admission;
//! struct Frames;
//! fn copy(admission: Admission<Frames>) -> (Admission<Frames>, Admission<Frames>) {
//!     (admission.clone(), admission)
//! }
//! ```
//!
//! ```compile_fail,E0382
//! use aura_core::ownership::sequence::SequenceOwner;
//! struct Frames;
//! let mut owner = SequenceOwner::<Frames>::starting_at(0);
//! let admission = owner.admit().unwrap();
//! let _first = admission.consume();
//! let _again = admission.consume();
//! ```
//!
//! ```compile_fail,E0599
//! use aura_core::ownership::sequence::SequenceOwner;
//! struct Frames;
//! fn share(owner: SequenceOwner<Frames>) -> (SequenceOwner<Frames>, SequenceOwner<Frames>) {
//!     (owner.clone(), owner)
//! }
//! ```

use crate::{AuraError, Hash32};
use std::fmt;
use std::marker::PhantomData;

/// One owner-minted sequence value for domain `D`, consumed once.
#[must_use = "an admission names a reserved sequence value; consume it"]
pub struct Admission<D> {
    value: u64,
    domain: PhantomData<fn() -> D>,
}

impl<D> Admission<D> {
    fn mint(value: u64) -> Self {
        Self {
            value,
            domain: PhantomData,
        }
    }

    /// The admitted sequence value, without consuming the admission.
    pub fn value(&self) -> u64 {
        self.value
    }

    /// Consume the admission, yielding its sequence value.
    pub fn consume(self) -> u64 {
        self.value
    }
}

impl<D> fmt::Debug for Admission<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Admission")
            .field("domain", &std::any::type_name::<D>())
            .field("value", &self.value)
            .finish()
    }
}

fn exhausted<D>() -> AuraError {
    AuraError::invalid(format!(
        "sequence domain {} is exhausted",
        std::any::type_name::<D>()
    ))
}

/// The in-process owner and only source of [`Admission<D>`] for domain `D`.
pub struct SequenceOwner<D> {
    next: u64,
    domain: PhantomData<fn() -> D>,
}

impl<D> SequenceOwner<D> {
    /// An owner whose first admission is `next`.
    pub fn starting_at(next: u64) -> Self {
        Self {
            next,
            domain: PhantomData,
        }
    }

    /// The value the next admission will carry.
    pub fn peek_next(&self) -> u64 {
        self.next
    }

    /// Mint the admission for exactly the next sequence value.
    pub fn admit(&mut self) -> Result<Admission<D>, AuraError> {
        let value = self.next;
        self.next = value.checked_add(1).ok_or_else(exhausted::<D>)?;
        Ok(Admission::mint(value))
    }
}

impl<D> fmt::Debug for SequenceOwner<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SequenceOwner")
            .field("domain", &std::any::type_name::<D>())
            .field("next", &self.next)
            .finish()
    }
}

/// Persistent counter behind a [`DurableSequenceOwner`]. It stores the next
/// unused value.
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait SequenceStore: Send + Sync {
    /// Stable identity of the persisted counter (for example its storage
    /// location). An owner serves exactly one identity.
    fn identity(&self) -> Hash32;

    /// The persisted next unused value, if one was ever persisted.
    async fn load(&self) -> Result<Option<u64>, AuraError>;

    /// Persist `next` as the next unused value.
    async fn persist(&self, next: u64) -> Result<(), AuraError>;
}

struct DurableState {
    identity: Option<Hash32>,
    next: Option<u64>,
}

/// Reserves and persists sequence values for one [`SequenceStore`] counter.
///
/// `reserve` holds the owner lock across load, persist and mint, so
/// concurrent reservations never observe the same value. The advanced
/// counter is persisted before the admission exists, so a restart (or a
/// failed use of the admission) never reissues a value.
pub struct DurableSequenceOwner<D> {
    state: futures::lock::Mutex<DurableState>,
    domain: PhantomData<fn() -> D>,
}

impl<D> Default for DurableSequenceOwner<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D> DurableSequenceOwner<D> {
    /// An owner that has not yet loaded its counter.
    pub fn new() -> Self {
        Self {
            state: futures::lock::Mutex::new(DurableState {
                identity: None,
                next: None,
            }),
            domain: PhantomData,
        }
    }

    /// Reserve the next value, never below `floor`, persisting the advanced
    /// counter to `store` before returning its admission.
    pub async fn reserve<S: SequenceStore + ?Sized>(
        &self,
        store: &S,
        floor: u64,
    ) -> Result<Admission<D>, AuraError> {
        let mut state = self.state.lock().await;
        let identity = store.identity();
        match state.identity {
            Some(bound) if bound != identity => {
                return Err(AuraError::invalid(
                    "durable sequence owner is bound to a different counter",
                ));
            }
            _ => state.identity = Some(identity),
        }
        let current = match state.next {
            Some(next) => next,
            None => store.load().await?.unwrap_or(0),
        };
        let value = current.max(floor);
        let next = value.checked_add(1).ok_or_else(exhausted::<D>)?;
        store.persist(next).await?;
        state.next = Some(next);
        Ok(Admission::mint(value))
    }
}

impl<D> fmt::Debug for DurableSequenceOwner<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DurableSequenceOwner")
            .field("domain", &std::any::type_name::<D>())
            .finish_non_exhaustive()
    }
}

/// Builds every 96-bit AEAD nonce for key `K`: a fixed 32-bit prefix and the
/// owner's 64-bit counter, so no two nonces under one key repeat.
pub struct NonceOwner<K> {
    prefix: [u8; 4],
    counter: SequenceOwner<K>,
}

impl<K> NonceOwner<K> {
    /// A nonce owner for a fresh key, with a per-key `prefix`.
    pub fn for_fresh_key(prefix: [u8; 4]) -> Self {
        Self {
            prefix,
            counter: SequenceOwner::starting_at(0),
        }
    }

    /// The next nonce under this key.
    pub fn next_nonce(&mut self) -> Result<[u8; 12], AuraError> {
        let counter = self.counter.admit()?.consume();
        let mut nonce = [0u8; 12];
        nonce[..4].copy_from_slice(&self.prefix);
        nonce[4..].copy_from_slice(&counter.to_be_bytes());
        Ok(nonce)
    }
}

impl<K> fmt::Debug for NonceOwner<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NonceOwner")
            .field("key", &std::any::type_name::<K>())
            .finish_non_exhaustive()
    }
}

/// Owner-minted idempotency key for one journal fact commit: the commit's
/// domain admission bound to its content digest.
#[must_use = "a commit key identifies one fact commit; hand it to the commit"]
pub struct CommitKey {
    digest: Hash32,
}

impl CommitKey {
    /// Mint the key for the commit admitted by `admission` over `content`.
    pub fn mint<D>(admission: Admission<D>, content: &[u8]) -> Self {
        let mut hasher = crate::hash::hasher();
        hasher.update(b"aura.commit-key.v1");
        hasher.update(std::any::type_name::<D>().as_bytes());
        hasher.update(&admission.consume().to_le_bytes());
        hasher.update(content);
        Self {
            digest: Hash32(hasher.finalize()),
        }
    }

    /// The key's digest, for comparison and indexing.
    pub fn digest(&self) -> Hash32 {
        self.digest
    }
}

impl fmt::Debug for CommitKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CommitKey").field(&self.digest).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct Frames;

    #[test]
    fn sequence_owner_admits_each_value_once() {
        let mut owner = SequenceOwner::<Frames>::starting_at(5);
        let values: Vec<u64> = (0..3).map(|_| owner.admit().unwrap().consume()).collect();
        assert_eq!(values, vec![5, 6, 7]);
        assert!(SequenceOwner::<Frames>::starting_at(u64::MAX)
            .admit()
            .is_err());
    }

    #[derive(Default)]
    struct MemoryStore {
        next: AtomicU64,
        persisted: AtomicU64,
    }

    #[async_trait::async_trait]
    impl SequenceStore for MemoryStore {
        fn identity(&self) -> Hash32 {
            Hash32([1; 32])
        }
        async fn load(&self) -> Result<Option<u64>, AuraError> {
            Ok(match self.next.load(Ordering::SeqCst) {
                0 => None,
                next => Some(next),
            })
        }
        async fn persist(&self, next: u64) -> Result<(), AuraError> {
            // Yield between load and persist so racing reservations interleave
            // unless the owner lock serializes them.
            futures::future::ready(()).await;
            self.next.store(next, Ordering::SeqCst);
            self.persisted.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn durable_owner_persists_before_admitting_and_respects_floor() {
        let store = MemoryStore::default();
        let owner = DurableSequenceOwner::<Frames>::new();
        assert_eq!(owner.reserve(&store, 3).await.unwrap().consume(), 3);
        assert_eq!(store.next.load(Ordering::SeqCst), 4);
        assert_eq!(owner.reserve(&store, 0).await.unwrap().consume(), 4);
        assert_eq!(owner.reserve(&store, 9).await.unwrap().consume(), 9);

        // A restarted owner resumes from the persisted counter.
        let restarted = DurableSequenceOwner::<Frames>::new();
        assert_eq!(restarted.reserve(&store, 0).await.unwrap().consume(), 10);
    }

    #[tokio::test]
    async fn concurrent_reservations_never_share_a_value() {
        let store = Arc::new(MemoryStore::default());
        let owner = Arc::new(DurableSequenceOwner::<Frames>::new());
        let reservations = (0..32).map(|_| {
            let store = store.clone();
            let owner = owner.clone();
            async move { owner.reserve(store.as_ref(), 1).await.unwrap().consume() }
        });
        let mut values = futures::future::join_all(reservations).await;
        values.sort_unstable();
        assert_eq!(values, (1..=32).collect::<Vec<_>>());
        assert_eq!(store.persisted.load(Ordering::SeqCst), 32);
    }

    struct OtherStore;

    #[async_trait::async_trait]
    impl SequenceStore for OtherStore {
        fn identity(&self) -> Hash32 {
            Hash32([2; 32])
        }
        async fn load(&self) -> Result<Option<u64>, AuraError> {
            Ok(None)
        }
        async fn persist(&self, _next: u64) -> Result<(), AuraError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn durable_owner_serves_one_counter() {
        let owner = DurableSequenceOwner::<Frames>::new();
        let _ = owner
            .reserve(&MemoryStore::default(), 0)
            .await
            .unwrap()
            .consume();
        assert!(owner.reserve(&OtherStore, 0).await.is_err());
    }

    #[test]
    fn nonce_owner_never_repeats_under_one_key() {
        let mut owner = NonceOwner::<Frames>::for_fresh_key([9, 9, 9, 9]);
        let first = owner.next_nonce().unwrap();
        let second = owner.next_nonce().unwrap();
        assert_ne!(first, second);
        assert_eq!(&first[..4], &[9, 9, 9, 9]);
    }

    #[test]
    fn commit_keys_bind_admission_and_content() {
        let mut owner = SequenceOwner::<Frames>::starting_at(0);
        let a = CommitKey::mint(owner.admit().unwrap(), b"fact");
        let b = CommitKey::mint(owner.admit().unwrap(), b"fact");
        assert_ne!(a.digest(), b.digest());
    }
}
