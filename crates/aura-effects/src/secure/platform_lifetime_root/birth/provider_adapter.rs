//! Private selected-platform lifetime actor. This is deliberately not selected
//! by L6: the full generic capacity and actual OS-keychain durability still need
//! their separate production acceptance before secure.rs can expose it.

use super::*;
use async_trait::async_trait;
use aura_core::effects::secret_lifetime::{
    ProfileSecretLifetimeBackend, ProfileSecretLifetimeRecoveryCapability,
    SecretAllocationReference, SecretLifetimeBackend, SecretLifetimeProviderIdentity,
    SecretLifetimeState,
};
use tokio::sync::Mutex;

enum OwnerState {
    Uninitialized,
    Empty(EmptyPlatformLifetimeRootAdmission),
    One(PlatformBirthAdmission),
    Two(append::PlatformRosterAdmission),
    Three(append::third::PlatformThreeRosterAdmission),
    Four(append::third::fourth::PlatformFourRosterAdmission),
    Generic(append::third::fourth::generic::PlatformGenericRosterAdmission),
}

struct PlatformLifetimeInner {
    // Both fields are the original selected Arc handles, never another store.
    owned: ProfileOwnedSecureStorage,
    identity: SecretLifetimeProviderIdentity,
    state: Mutex<OwnerState>,
}

struct PlatformLifetimeProvider(Arc<PlatformLifetimeInner>);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Entry {
    First,
    Second,
    Third,
    Fourth,
    Tail([u8; 32]),
}

struct PlatformLifetimeAllocation {
    inner: Arc<PlatformLifetimeInner>,
    entry: Entry,
    reference: SecretAllocationReference,
}

/// Private proof-issuer seam. No production selector calls this yet.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "ProfileSecretLifetimeRecoveryCapability",
    family = "proof_issuer"
)]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
#[allow(dead_code)]
pub(super) fn selected_profile_channel(
    owned: &ProfileOwnedSecureStorage,
) -> Result<ProfileSecretLifetimeRecoveryCapability, AuraError> {
    selected_owner(owned)?;
    let inner = PlatformLifetimeInner {
        owned: ProfileOwnedSecureStorage {
            backend: owned.backend.clone(),
            _owner: owned._owner.clone(),
        },
        identity: owned._owner.lifetime_provider_identity.clone(),
        state: Mutex::new(OwnerState::Uninitialized),
    };
    Ok(
        ProfileSecretLifetimeRecoveryCapability::from_trusted_provider(Box::new(
            PlatformLifetimeProvider(Arc::new(inner)),
        )),
    )
}

impl PlatformLifetimeInner {
    async fn initialize(&self, state: &mut OwnerState) -> Result<(), AuraError> {
        if !matches!(state, OwnerState::Uninitialized) {
            return Ok(());
        }
        let (selected, _) = selected_owner(&self.owned)?;
        let root = match read_entry(selected, KEYRING_USER)? {
            Some(bytes) => Some(RootRecord::decode(selected, &bytes)?),
            None => None,
        };
        *state = match root {
            None => OwnerState::Empty(admit_empty_root(&self.owned).await?),
            Some(root) if matches!(root.phase, PENDING | COMPLETE) => {
                OwnerState::Empty(admit_empty_root(&self.owned).await?)
            }
            Some(root) if root.generic_phase != GENERIC_UNCLAIMED => OwnerState::Generic(
                append::third::fourth::generic::recover_generic(&self.owned).await?,
            ),
            Some(root) if root.tail_phase != TAIL_IDLE => {
                OwnerState::Four(append::third::fourth::recover_four_roster(&self.owned).await?)
            }
            Some(root) if root.third_phase != THIRD_UNCLAIMED => {
                OwnerState::Three(append::recover_third_owned(&self.owned).await?)
            }
            Some(root) if !root.second_scope.is_empty() || append::claim_present(selected)? => {
                OwnerState::Two(append::recover_roster(&self.owned).await?)
            }
            Some(_) => OwnerState::One(recover_one(&self.owned).await?),
        };
        Ok(())
    }

    async fn reconcile(&self, state: &mut OwnerState) -> Result<(), AuraError> {
        self.initialize(state).await?;
        match state {
            OwnerState::Empty(empty) => {
                let (selected, _) = selected_owner(&self.owned)?;
                let root = read_root(&self.owned, selected)?;
                if root.phase != COMPLETE {
                    let first = reconcile_one(&self.owned, empty).await?;
                    *state = OwnerState::One(first);
                }
            }
            OwnerState::One(first) => {
                let (selected, _) = selected_owner(&self.owned)?;
                let root = read_root(&self.owned, selected)?;
                if !root.second_scope.is_empty() || append::claim_present(selected)? {
                    // The first token stays in state across the await. An
                    // interrupted promotion can reconcile the same roster.
                    let roster = append::promote_borrowed(&self.owned, first).await?;
                    *state = OwnerState::Two(roster);
                }
            }
            OwnerState::Two(two) => {
                let (selected, _) = selected_owner(&self.owned)?;
                let root = read_root(&self.owned, selected)?;
                if root.third_phase != THIRD_UNCLAIMED {
                    let three = append::promote_third_owned(&self.owned, two).await?;
                    *state = OwnerState::Three(three);
                }
            }
            OwnerState::Three(three) => {
                let (selected, _) = selected_owner(&self.owned)?;
                let root = read_root(&self.owned, selected)?;
                if root.tail_phase != TAIL_IDLE {
                    let four = append::third::fourth::promote_borrowed(&self.owned, three).await?;
                    *state = OwnerState::Four(four);
                }
            }
            OwnerState::Four(four) => {
                let (selected, _) = selected_owner(&self.owned)?;
                let root = read_root(&self.owned, selected)?;
                if root.generic_phase != GENERIC_UNCLAIMED {
                    let generic =
                        append::third::fourth::generic::promote_borrowed(&self.owned, four).await?;
                    *state = OwnerState::Generic(generic);
                }
            }
            OwnerState::Generic(_) => {}
            OwnerState::Uninitialized => unreachable!("initialized above"),
        }
        Ok(())
    }

    async fn references(
        &self,
        state: &OwnerState,
    ) -> Result<Vec<(Entry, SecretAllocationReference)>, AuraError> {
        if let OwnerState::Three(three) = state {
            return Ok(append::third_references(&self.owned, three)
                .await?
                .into_iter()
                .zip([Entry::First, Entry::Second, Entry::Third])
                .map(|(reference, entry)| (entry, reference))
                .collect());
        }
        if let OwnerState::Four(four) = state {
            return Ok(append::third::fourth::owned_references(&self.owned, four)
                .await?
                .into_iter()
                .zip([Entry::First, Entry::Second, Entry::Third, Entry::Fourth])
                .map(|(reference, entry)| (entry, reference))
                .collect());
        }
        if let OwnerState::Generic(generic) = state {
            let (selected, _) = selected_owner(&self.owned)?;
            let root = read_root(&self.owned, selected)?;
            return Ok(
                append::third::fourth::generic::owned_references(&self.owned, generic)
                    .await?
                    .into_iter()
                    .enumerate()
                    .map(|(index, reference)| {
                        let entry = if index == 0 {
                            Entry::First
                        } else if index == 1 {
                            Entry::Second
                        } else if index == 2 {
                            Entry::Third
                        } else if reference.allocation == root.tail_next_id {
                            Entry::Fourth
                        } else {
                            Entry::Tail(reference.allocation)
                        };
                        (entry, reference)
                    })
                    .collect(),
            );
        }
        let (selected, _) = selected_owner(&self.owned)?;
        let _guard = selected.namespace_owner()?.mutation_guard().await;
        let mut root = read_root(&self.owned, selected)?;
        match state {
            OwnerState::Empty(empty) => {
                if root.id != empty._id || root.phase != COMPLETE {
                    return Err(invalid("platform empty lifetime root changed"));
                }
                Ok(Vec::new())
            }
            OwnerState::One(first) => {
                if root.id != first.root
                    || root.birth_id != first.allocation
                    || append::claim_present(selected)?
                    || verify_inventory(&self.owned, &root)?.is_none()
                {
                    return Err(invalid("platform first lifetime inventory changed"));
                }
                decision::validate_recovery(&self.owned, selected, &root)?;
                if matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE) {
                    decision::recover_erasure(&self.owned, selected, &mut root)?;
                } else {
                    let (bytes, _) = read_intent(selected, &root)?;
                    if read_entry(selected, RECORD_USER)?.as_deref() != Some(bytes.as_slice()) {
                        return Err(invalid("platform first lifetime credential changed"));
                    }
                }
                Ok(vec![(
                    Entry::First,
                    SecretAllocationReference {
                        allocation: root.birth_id,
                        scope: root.first_scope,
                    },
                )])
            }
            OwnerState::Two(roster) => {
                root = append::verify_roster_owner(&self.owned, selected, roster)?;
                if matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE) {
                    decision::recover_erasure(&self.owned, selected, &mut root)?;
                }
                let second = roster
                    .allocations
                    .iter()
                    .copied()
                    .find(|id| *id != root.birth_id)
                    .ok_or_else(|| invalid("platform second lifetime identity is missing"))?;
                Ok(vec![
                    (
                        Entry::First,
                        SecretAllocationReference {
                            allocation: root.birth_id,
                            scope: root.first_scope,
                        },
                    ),
                    (
                        Entry::Second,
                        SecretAllocationReference {
                            allocation: second,
                            scope: root.second_scope,
                        },
                    ),
                ])
            }
            OwnerState::Three(_) => unreachable!("handled before namespace lock"),
            OwnerState::Four(_) => unreachable!("handled before namespace lock"),
            OwnerState::Generic(_) => unreachable!("handled before namespace lock"),
            OwnerState::Uninitialized => Err(invalid("platform lifetime owner is uninitialized")),
        }
    }

    async fn require_reference(
        &self,
        state: &OwnerState,
        entry: Entry,
        expected: &SecretAllocationReference,
    ) -> Result<(), AuraError> {
        let references = self.references(state).await?;
        if references
            .iter()
            .any(|(observed_entry, reference)| *observed_entry == entry && reference == expected)
        {
            Ok(())
        } else {
            Err(invalid(
                "platform lifetime reference differs from original birth",
            ))
        }
    }
}

#[async_trait]
impl ProfileSecretLifetimeBackend for PlatformLifetimeProvider {
    fn provider_identity(&self) -> &SecretLifetimeProviderIdentity {
        &self.0.identity
    }

    async fn allocate(
        &self,
        scope: &[u8],
        secret: &[u8],
    ) -> Result<Arc<dyn SecretLifetimeBackend>, AuraError> {
        let mut state = self.0.state.lock().await;
        self.0.reconcile(&mut state).await?;
        let mut new_id = None;
        match &*state {
            OwnerState::Empty(empty) => {
                let first = allocate_one(&self.0.owned, empty, scope, secret).await?;
                *state = OwnerState::One(first);
            }
            OwnerState::One(first) => {
                append::append_second(&self.0.owned, first, scope, secret).await?;
                let roster = append::promote_borrowed(&self.0.owned, first).await?;
                *state = OwnerState::Two(roster);
            }
            OwnerState::Two(two) => {
                let three = append::append_third_owned(&self.0.owned, two, scope, secret).await?;
                *state = OwnerState::Three(three);
            }
            OwnerState::Three(three) => {
                append::third::fourth::append_fourth(&self.0.owned, three, scope, secret).await?;
                let four = append::third::fourth::promote_borrowed(&self.0.owned, three).await?;
                *state = OwnerState::Four(four);
            }
            OwnerState::Four(four) => {
                let generic =
                    append::third::fourth::generic::promote_borrowed(&self.0.owned, four).await?;
                *state = OwnerState::Generic(generic);
                let OwnerState::Generic(generic) = &*state else {
                    unreachable!("generic owner installed above")
                };
                new_id = Some(
                    append::third::fourth::generic::append(&self.0.owned, generic, scope, secret)
                        .await?,
                );
            }
            OwnerState::Generic(generic) => {
                new_id = Some(
                    append::third::fourth::generic::append(&self.0.owned, generic, scope, secret)
                        .await?,
                );
            }
            OwnerState::Uninitialized => unreachable!("reconciled above"),
        }
        let (entry, reference) = if let Some(id) = new_id {
            // Generic append has already authenticated every prior entry and
            // published this exact scope/id under the root. A second complete
            // roster scan here would double the cost of every birth.
            (
                Entry::Tail(id),
                SecretAllocationReference {
                    allocation: id,
                    scope: scope.to_vec(),
                },
            )
        } else {
            self.0
                .references(&state)
                .await?
                .into_iter()
                .last()
                .ok_or_else(|| invalid("new platform birth was not retained"))?
        };
        Ok(Arc::new(PlatformLifetimeAllocation {
            inner: self.0.clone(),
            entry,
            reference,
        }))
    }

    async fn recover_owned_inventory(
        &self,
    ) -> Result<Vec<Arc<dyn SecretLifetimeBackend>>, AuraError> {
        let mut state = self.0.state.lock().await;
        self.0.reconcile(&mut state).await?;
        Ok(self
            .0
            .references(&state)
            .await?
            .into_iter()
            .map(|(entry, reference)| {
                Arc::new(PlatformLifetimeAllocation {
                    inner: self.0.clone(),
                    entry,
                    reference,
                }) as Arc<dyn SecretLifetimeBackend>
            })
            .collect())
    }
}

impl PlatformLifetimeAllocation {
    async fn checked_state(&self) -> Result<tokio::sync::MutexGuard<'_, OwnerState>, AuraError> {
        let state = self.inner.state.lock().await;
        self.inner
            .require_reference(&state, self.entry, &self.reference)
            .await?;
        Ok(state)
    }
}

#[async_trait]
impl SecretLifetimeBackend for PlatformLifetimeAllocation {
    fn reference(&self) -> &SecretAllocationReference {
        &self.reference
    }

    async fn state(&self) -> Result<SecretLifetimeState, AuraError> {
        let state = self.checked_state().await?;
        if let (OwnerState::Three(three), Entry::Third) = (&*state, self.entry) {
            return append::third_state(&self.inner.owned, three).await;
        }
        if let (OwnerState::Four(four), Entry::Third) = (&*state, self.entry) {
            return append::third::owned_four_state(&self.inner.owned, four).await;
        }
        if let (OwnerState::Four(four), Entry::Fourth) = (&*state, self.entry) {
            return append::third::fourth::owned_state(&self.inner.owned, four).await;
        }
        if let (OwnerState::Generic(generic), Entry::Third) = (&*state, self.entry) {
            return append::third::owned_generic_state(&self.inner.owned, generic).await;
        }
        if let (OwnerState::Generic(generic), Entry::Fourth | Entry::Tail(_)) =
            (&*state, self.entry)
        {
            return append::third::fourth::generic::owned_state(
                &self.inner.owned,
                generic,
                &self.reference.allocation,
            )
            .await;
        }
        let (selected, _) = selected_owner(&self.inner.owned)?;
        let _guard = selected.namespace_owner()?.mutation_guard().await;
        let root = read_root(&self.inner.owned, selected)?;
        let (phase, decision) = match (&*state, self.entry) {
            (OwnerState::One(_), Entry::First) | (OwnerState::Two(_), Entry::First) => {
                let record = decision::read_record(selected, &root)?;
                (
                    root.phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Three(three), Entry::First) => {
                append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Three(three),
                )?;
                let record = decision::read_record(selected, &root)?;
                (
                    root.phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Four(four), Entry::First) => {
                let (verified, _) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Four(four),
                )?;
                let record = decision::read_record(selected, &verified)?;
                (
                    verified.phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Two(roster), Entry::Second) => {
                let claim =
                    append::lifecycle::bound_claim(&self.inner.owned, selected, &root, roster)?;
                let record = append::lifecycle::decision_record(selected, &root, &claim)?;
                (
                    root.second_phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Three(three), Entry::Second) => {
                let (_, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Three(three),
                )?;
                let record = append::lifecycle::decision_record(selected, &root, &claim)?;
                (
                    root.second_phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Four(four), Entry::Second) => {
                let (verified, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Four(four),
                )?;
                let record = append::lifecycle::decision_record(selected, &verified, &claim)?;
                (
                    verified.second_phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Generic(generic), Entry::First) => {
                let (verified, _) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Generic(generic),
                )?;
                let record = decision::read_record(selected, &verified)?;
                (
                    verified.phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            (OwnerState::Generic(generic), Entry::Second) => {
                let (verified, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Generic(generic),
                )?;
                let record = append::lifecycle::decision_record(selected, &verified, &claim)?;
                (
                    verified.second_phase,
                    record.map(|record| (record.kind, record.bytes.to_vec())),
                )
            }
            _ => return Err(invalid("platform lifetime allocation owner changed")),
        };
        if matches!(self.entry, Entry::First) && phase == DECISION_PENDING
            || matches!(self.entry, Entry::Second) && phase == SECOND_DECISION_PENDING
        {
            return Err(invalid(
                "platform lifetime first decision is not acknowledged",
            ));
        }
        match decision {
            None => Ok(SecretLifetimeState::Live),
            Some((decision::POSITIVE, bytes)) => {
                Ok(SecretLifetimeState::Positive { decision: bytes })
            }
            Some((decision::NEGATIVE, bytes))
                if matches!(self.entry, Entry::First) && phase == ERASURE_COMPLETE
                    || matches!(self.entry, Entry::Second) && phase == SECOND_ERASURE_COMPLETE =>
            {
                Ok(SecretLifetimeState::Retired { decision: bytes })
            }
            Some((decision::NEGATIVE, bytes)) => {
                Ok(SecretLifetimeState::Negative { decision: bytes })
            }
            _ => Err(invalid("platform lifetime decision kind is invalid")),
        }
    }

    async fn read_live_secret(&self) -> Result<Vec<u8>, AuraError> {
        let state = self.checked_state().await?;
        match (&*state, self.entry) {
            (OwnerState::One(first), Entry::First) => {
                Ok(read_live_secret(&self.inner.owned, first).await?.to_vec())
            }
            (OwnerState::Two(roster), Entry::First) => {
                Ok(read_roster_first_secret(&self.inner.owned, roster)
                    .await?
                    .to_vec())
            }
            (OwnerState::Two(roster), Entry::Second) => {
                Ok(append::read_roster_second_secret(&self.inner.owned, roster)
                    .await?
                    .to_vec())
            }
            (OwnerState::Three(three), Entry::First) => {
                Ok(read_three_first_secret(&self.inner.owned, three)
                    .await?
                    .to_vec())
            }
            (OwnerState::Three(three), Entry::Second) => Ok(append::lifecycle::read_three_secret(
                &self.inner.owned,
                three,
            )
            .await?
            .to_vec()),
            (OwnerState::Three(three), Entry::Third) => Ok(
                append::third::lifecycle::read_live_secret(&self.inner.owned, three)
                    .await?
                    .to_vec(),
            ),
            (OwnerState::Four(four), Entry::First) => {
                Ok(read_four_first_secret(&self.inner.owned, four)
                    .await?
                    .to_vec())
            }
            (OwnerState::Four(four), Entry::Second) => {
                Ok(append::lifecycle::read_four_secret(&self.inner.owned, four)
                    .await?
                    .to_vec())
            }
            (OwnerState::Four(four), Entry::Third) => Ok(
                append::third::lifecycle::read_four_secret(&self.inner.owned, four)
                    .await?
                    .to_vec(),
            ),
            (OwnerState::Four(four), Entry::Fourth) => Ok(
                append::third::fourth::read_fourth_secret(&self.inner.owned, four)
                    .await?
                    .to_vec(),
            ),
            (OwnerState::Generic(generic), Entry::First) => {
                Ok(read_generic_first_secret(&self.inner.owned, generic)
                    .await?
                    .to_vec())
            }
            (OwnerState::Generic(generic), Entry::Second) => Ok(
                append::lifecycle::read_generic_secret(&self.inner.owned, generic)
                    .await?
                    .to_vec(),
            ),
            (OwnerState::Generic(generic), Entry::Third) => Ok(
                append::third::lifecycle::read_generic_secret(&self.inner.owned, generic)
                    .await?
                    .to_vec(),
            ),
            (OwnerState::Generic(generic), Entry::Fourth | Entry::Tail(_)) => {
                Ok(append::third::fourth::generic::read_live(
                    &self.inner.owned,
                    generic,
                    &self.reference.allocation,
                )
                .await?
                .to_vec())
            }
            _ => Err(invalid("platform lifetime allocation owner changed")),
        }
    }

    async fn decide_positive(&self, bytes: &[u8]) -> Result<(), AuraError> {
        let state = self.checked_state().await?;
        match (&*state, self.entry) {
            (OwnerState::One(first), Entry::First) => {
                decision::decide(&self.inner.owned, first, true, bytes).await
            }
            (OwnerState::Two(roster), Entry::First) => {
                decision::decide_roster(&self.inner.owned, roster, true, bytes).await
            }
            (OwnerState::Two(roster), Entry::Second) => {
                append::lifecycle::decide(&self.inner.owned, roster, true, bytes).await
            }
            (OwnerState::Three(three), Entry::First) => {
                decision::decide_three(&self.inner.owned, three, true, bytes).await
            }
            (OwnerState::Three(three), Entry::Second) => {
                append::lifecycle::decide_three(&self.inner.owned, three, true, bytes).await
            }
            (OwnerState::Three(three), Entry::Third) => {
                append::third::lifecycle::decide(&self.inner.owned, three, true, bytes).await
            }
            (OwnerState::Four(four), Entry::First) => {
                decision::decide_four(&self.inner.owned, four, true, bytes).await
            }
            (OwnerState::Four(four), Entry::Second) => {
                append::lifecycle::decide_four(&self.inner.owned, four, true, bytes).await
            }
            (OwnerState::Four(four), Entry::Third) => {
                append::third::lifecycle::decide_four(&self.inner.owned, four, true, bytes).await
            }
            (OwnerState::Four(four), Entry::Fourth) => {
                append::third::fourth::lifecycle::decide(&self.inner.owned, four, true, bytes).await
            }
            (OwnerState::Generic(generic), Entry::First) => {
                decision::decide_generic(&self.inner.owned, generic, true, bytes).await
            }
            (OwnerState::Generic(generic), Entry::Second) => {
                append::lifecycle::decide_generic(&self.inner.owned, generic, true, bytes).await
            }
            (OwnerState::Generic(generic), Entry::Third) => {
                append::third::lifecycle::decide_generic(&self.inner.owned, generic, true, bytes)
                    .await
            }
            (OwnerState::Generic(generic), Entry::Fourth | Entry::Tail(_)) => {
                append::third::fourth::generic::decide(
                    &self.inner.owned,
                    generic,
                    &self.reference.allocation,
                    true,
                    bytes,
                )
                .await
            }
            _ => Err(invalid("platform lifetime allocation owner changed")),
        }
    }

    async fn decide_negative(&self, bytes: &[u8]) -> Result<(), AuraError> {
        let state = self.checked_state().await?;
        match (&*state, self.entry) {
            (OwnerState::One(first), Entry::First) => {
                decision::decide(&self.inner.owned, first, false, bytes).await
            }
            (OwnerState::Two(roster), Entry::First) => {
                decision::decide_roster(&self.inner.owned, roster, false, bytes).await
            }
            (OwnerState::Two(roster), Entry::Second) => {
                append::lifecycle::decide(&self.inner.owned, roster, false, bytes).await
            }
            (OwnerState::Three(three), Entry::First) => {
                decision::decide_three(&self.inner.owned, three, false, bytes).await
            }
            (OwnerState::Three(three), Entry::Second) => {
                append::lifecycle::decide_three(&self.inner.owned, three, false, bytes).await
            }
            (OwnerState::Three(three), Entry::Third) => {
                append::third::lifecycle::decide(&self.inner.owned, three, false, bytes).await
            }
            (OwnerState::Four(four), Entry::First) => {
                decision::decide_four(&self.inner.owned, four, false, bytes).await
            }
            (OwnerState::Four(four), Entry::Second) => {
                append::lifecycle::decide_four(&self.inner.owned, four, false, bytes).await
            }
            (OwnerState::Four(four), Entry::Third) => {
                append::third::lifecycle::decide_four(&self.inner.owned, four, false, bytes).await
            }
            (OwnerState::Four(four), Entry::Fourth) => {
                append::third::fourth::lifecycle::decide(&self.inner.owned, four, false, bytes)
                    .await
            }
            (OwnerState::Generic(generic), Entry::First) => {
                decision::decide_generic(&self.inner.owned, generic, false, bytes).await
            }
            (OwnerState::Generic(generic), Entry::Second) => {
                append::lifecycle::decide_generic(&self.inner.owned, generic, false, bytes).await
            }
            (OwnerState::Generic(generic), Entry::Third) => {
                append::third::lifecycle::decide_generic(&self.inner.owned, generic, false, bytes)
                    .await
            }
            (OwnerState::Generic(generic), Entry::Fourth | Entry::Tail(_)) => {
                append::third::fourth::generic::decide(
                    &self.inner.owned,
                    generic,
                    &self.reference.allocation,
                    false,
                    bytes,
                )
                .await
            }
            _ => Err(invalid("platform lifetime allocation owner changed")),
        }
    }

    async fn acknowledge_retirement(&self, bytes: &[u8]) -> Result<(), AuraError> {
        let state = self.checked_state().await?;
        if let (OwnerState::Three(three), Entry::Third) = (&*state, self.entry) {
            match append::third_state(&self.inner.owned, three).await? {
                SecretLifetimeState::Negative { decision } if decision == bytes => {
                    append::third::lifecycle::acknowledge_negative(&self.inner.owned, three, bytes)
                        .await?;
                }
                SecretLifetimeState::Retired { decision } if decision == bytes => {}
                _ => {
                    return Err(invalid(
                        "platform third retirement differs from original negative decision",
                    ));
                }
            }
            return append::third::lifecycle::erase(&self.inner.owned, three).await;
        }
        if let (OwnerState::Four(four), Entry::Third) = (&*state, self.entry) {
            match append::third::owned_four_state(&self.inner.owned, four).await? {
                SecretLifetimeState::Negative { decision } if decision == bytes => {
                    append::third::lifecycle::acknowledge_four(&self.inner.owned, four, bytes)
                        .await?;
                }
                SecretLifetimeState::Retired { decision } if decision == bytes => {}
                _ => {
                    return Err(invalid(
                        "platform third retirement differs from original negative decision",
                    ))
                }
            }
            return append::third::lifecycle::erase_four(&self.inner.owned, four).await;
        }
        if let (OwnerState::Four(four), Entry::Fourth) = (&*state, self.entry) {
            match append::third::fourth::owned_state(&self.inner.owned, four).await? {
                SecretLifetimeState::Negative { decision } if decision == bytes => {
                    append::third::fourth::lifecycle::acknowledge_negative(
                        &self.inner.owned,
                        four,
                        bytes,
                    )
                    .await?;
                }
                SecretLifetimeState::Retired { decision } if decision == bytes => {}
                _ => {
                    return Err(invalid(
                        "platform fourth retirement differs from original negative decision",
                    ))
                }
            }
            return append::third::fourth::lifecycle::erase(&self.inner.owned, four).await;
        }
        if let (OwnerState::Generic(generic), Entry::Third) = (&*state, self.entry) {
            match append::third::owned_generic_state(&self.inner.owned, generic).await? {
                SecretLifetimeState::Negative { decision } if decision == bytes => {
                    append::third::lifecycle::acknowledge_generic(
                        &self.inner.owned,
                        generic,
                        bytes,
                    )
                    .await?;
                }
                SecretLifetimeState::Retired { decision } if decision == bytes => {}
                _ => return Err(invalid("platform third retirement differs from original")),
            }
            return append::third::lifecycle::erase_generic(&self.inner.owned, generic).await;
        }
        if let (OwnerState::Generic(generic), Entry::Fourth | Entry::Tail(_)) =
            (&*state, self.entry)
        {
            return append::third::fourth::generic::retire_negative_exact(
                &self.inner.owned,
                generic,
                &self.reference.allocation,
                bytes,
            )
            .await;
        }
        let (selected, _) = selected_owner(&self.inner.owned)?;
        let guard = selected.namespace_owner()?.mutation_guard().await;
        let root = read_root(&self.inner.owned, selected)?;
        let (record, erasure_started) = match (&*state, self.entry) {
            (OwnerState::One(_), Entry::First) | (OwnerState::Two(_), Entry::First) => (
                decision::read_record(selected, &root)?,
                matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE),
            ),
            (OwnerState::Two(roster), Entry::Second) => {
                let claim =
                    append::lifecycle::bound_claim(&self.inner.owned, selected, &root, roster)?;
                (
                    append::lifecycle::decision_record(selected, &root, &claim)?,
                    matches!(
                        root.second_phase,
                        SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
                    ),
                )
            }
            (OwnerState::Three(three), Entry::First) => {
                append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Three(three),
                )?;
                (
                    decision::read_record(selected, &root)?,
                    matches!(root.phase, ERASURE_PENDING | ERASURE_COMPLETE),
                )
            }
            (OwnerState::Three(three), Entry::Second) => {
                let (_, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Three(three),
                )?;
                (
                    append::lifecycle::decision_record(selected, &root, &claim)?,
                    matches!(
                        root.second_phase,
                        SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
                    ),
                )
            }
            (OwnerState::Four(four), Entry::First) => {
                let (verified, _) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Four(four),
                )?;
                (
                    decision::read_record(selected, &verified)?,
                    matches!(verified.phase, ERASURE_PENDING | ERASURE_COMPLETE),
                )
            }
            (OwnerState::Four(four), Entry::Second) => {
                let (verified, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Four(four),
                )?;
                (
                    append::lifecycle::decision_record(selected, &verified, &claim)?,
                    matches!(
                        verified.second_phase,
                        SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
                    ),
                )
            }
            (OwnerState::Generic(generic), Entry::First) => {
                let (verified, _) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Generic(generic),
                )?;
                (
                    decision::read_record(selected, &verified)?,
                    matches!(verified.phase, ERASURE_PENDING | ERASURE_COMPLETE),
                )
            }
            (OwnerState::Generic(generic), Entry::Second) => {
                let (verified, claim) = append::verify_original_roster(
                    &self.inner.owned,
                    selected,
                    append::OriginalRosterOwner::Generic(generic),
                )?;
                (
                    append::lifecycle::decision_record(selected, &verified, &claim)?,
                    matches!(
                        verified.second_phase,
                        SECOND_ERASURE_PENDING | SECOND_ERASURE_COMPLETE
                    ),
                )
            }
            _ => return Err(invalid("platform lifetime allocation owner changed")),
        };
        if !matches!(record, Some(record) if record.kind == decision::NEGATIVE && record.bytes.as_slice() == bytes)
        {
            return Err(invalid(
                "platform retirement differs from original negative decision",
            ));
        }
        drop(guard);
        match (&*state, self.entry) {
            (OwnerState::One(first), Entry::First) => {
                if !erasure_started {
                    decision::negative_denial::acknowledge_negative_denial(
                        &self.inner.owned,
                        first,
                        bytes,
                    )
                    .await?;
                }
                decision::negative_denial::erasure::erase_original(&self.inner.owned, first).await
            }
            (OwnerState::Two(roster), Entry::First) => {
                if !erasure_started {
                    decision::negative_denial::acknowledge_roster(&self.inner.owned, roster, bytes)
                        .await?;
                }
                decision::negative_denial::erasure::erase_roster(&self.inner.owned, roster).await
            }
            (OwnerState::Two(roster), Entry::Second) => {
                if !erasure_started {
                    append::lifecycle::acknowledge_negative(&self.inner.owned, roster, bytes)
                        .await?;
                }
                append::lifecycle::erase(&self.inner.owned, roster).await
            }
            (OwnerState::Three(three), Entry::First) => {
                if !erasure_started {
                    decision::negative_denial::acknowledge_three(&self.inner.owned, three, bytes)
                        .await?;
                }
                decision::negative_denial::erasure::erase_three(&self.inner.owned, three).await
            }
            (OwnerState::Three(three), Entry::Second) => {
                if !erasure_started {
                    append::lifecycle::acknowledge_three(&self.inner.owned, three, bytes).await?;
                }
                append::lifecycle::erase_three(&self.inner.owned, three).await
            }
            (OwnerState::Four(four), Entry::First) => {
                if !erasure_started {
                    decision::negative_denial::acknowledge_four(&self.inner.owned, four, bytes)
                        .await?;
                }
                decision::negative_denial::erasure::erase_four(&self.inner.owned, four).await
            }
            (OwnerState::Four(four), Entry::Second) => {
                if !erasure_started {
                    append::lifecycle::acknowledge_four(&self.inner.owned, four, bytes).await?;
                }
                append::lifecycle::erase_four(&self.inner.owned, four).await
            }
            (OwnerState::Generic(generic), Entry::First) => {
                if !erasure_started {
                    decision::negative_denial::acknowledge_generic(
                        &self.inner.owned,
                        generic,
                        bytes,
                    )
                    .await?;
                }
                decision::negative_denial::erasure::erase_generic(&self.inner.owned, generic).await
            }
            (OwnerState::Generic(generic), Entry::Second) => {
                if !erasure_started {
                    append::lifecycle::acknowledge_generic(&self.inner.owned, generic, bytes)
                        .await?;
                }
                append::lifecycle::erase_generic(&self.inner.owned, generic).await
            }
            _ => Err(invalid("platform lifetime allocation owner changed")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secure::platform_lifetime_root::tests::Fixture;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    #[tokio::test]
    async fn selected_adapter_recovers_empty_through_fifth_under_original_lease() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        assert!(channel.belongs_to_provider(&owned._owner.lifetime_provider_identity));
        assert!(channel.recover_owned_inventory().await?.is_empty());
        let mut duplicate = selected_profile_channel(owned)?;
        assert!(duplicate.recover_owned_inventory().await.is_err());
        let first = channel.allocate(b"first scope", b"first secret").await?;
        assert_eq!(first.reference().scope, b"first scope");
        assert_eq!(first.read_live_secret().await?, b"first secret");
        let second = channel.allocate(b"second scope", b"second secret").await?;
        assert_eq!(second.reference().scope, b"second scope");
        assert_eq!(first.read_live_secret().await?, b"first secret");
        assert_eq!(second.read_live_secret().await?, b"second secret");
        assert!(channel.reconcile_unhanded_births().await?.is_empty());
        let third = channel.allocate(b"third scope", b"third secret").await?;
        assert_eq!(third.reference().scope, b"third scope");
        assert_eq!(first.read_live_secret().await?, b"first secret");
        assert_eq!(second.read_live_secret().await?, b"second secret");
        assert_eq!(third.read_live_secret().await?, b"third secret");
        let fourth = channel.allocate(b"fourth scope", b"fourth secret").await?;
        assert_eq!(fourth.reference().scope, b"fourth scope");
        for (allocation, expected) in [
            (&first, b"first secret".as_slice()),
            (&second, b"second secret".as_slice()),
            (&third, b"third secret".as_slice()),
            (&fourth, b"fourth secret".as_slice()),
        ] {
            assert_eq!(allocation.read_live_secret().await?, expected);
        }
        let fifth = channel.allocate(b"fifth scope", b"fifth secret").await?;
        assert_eq!(fifth.reference().scope, b"fifth scope");
        assert_eq!(fifth.read_live_secret().await?, b"fifth secret");
        assert_eq!(first.read_live_secret().await?, b"first secret");
        drop((
            first, second, third, fourth, fifth, channel, duplicate, selected,
        ));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 5);
        for (scope, secret) in [
            (b"first scope".as_slice(), b"first secret".as_slice()),
            (b"second scope".as_slice(), b"second secret".as_slice()),
            (b"third scope".as_slice(), b"third secret".as_slice()),
            (b"fourth scope".as_slice(), b"fourth secret".as_slice()),
            (b"fifth scope".as_slice(), b"fifth secret".as_slice()),
        ] {
            let allocation = recovered
                .iter()
                .find(|allocation| allocation.reference().scope == scope)
                .ok_or("original scope missing after generic reopen")?;
            assert_eq!(allocation.read_live_secret().await?, secret);
        }
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_third_preserves_prior_retirement_and_reopens() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let second = channel.allocate(b"second scope", b"second secret").await?;
        let third = channel.allocate(b"third scope", b"third secret").await?;
        for (allocation, decision) in [
            (&first, b"first refusal".as_slice()),
            (&second, b"second refusal".as_slice()),
            (&third, b"third refusal".as_slice()),
        ] {
            let negative = allocation.decide_negative(decision).await?;
            negative.retire().await?;
            negative.retire().await?;
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision: exact } if exact == decision)
            );
            assert!(allocation.read_live_secret().await.is_err());
        }
        drop((first, second, third, channel, selected));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 3);
        for (allocation, (scope, decision)) in recovered.iter().zip([
            (b"first scope".as_slice(), b"first refusal".as_slice()),
            (b"second scope".as_slice(), b"second refusal".as_slice()),
            (b"third scope".as_slice(), b"third refusal".as_slice()),
        ]) {
            assert_eq!(allocation.reference().scope, scope);
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision: exact } if exact == decision)
            );
            assert!(allocation.read_live_secret().await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_fourth_preserves_all_retirement_and_reopens() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let second = channel.allocate(b"second scope", b"second secret").await?;
        let third = channel.allocate(b"third scope", b"third secret").await?;
        let fourth = channel.allocate(b"fourth scope", b"fourth secret").await?;
        for (allocation, decision) in [
            (&first, b"first refusal".as_slice()),
            (&second, b"second refusal".as_slice()),
            (&third, b"third refusal".as_slice()),
            (&fourth, b"fourth refusal".as_slice()),
        ] {
            let negative = allocation.decide_negative(decision).await?;
            negative.retire().await?;
            negative.retire().await?;
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision: exact } if exact == decision)
            );
            assert!(allocation.read_live_secret().await.is_err());
        }
        drop((first, second, third, fourth, channel, selected));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 4);
        for (allocation, (scope, decision)) in recovered.iter().zip([
            (b"first scope".as_slice(), b"first refusal".as_slice()),
            (b"second scope".as_slice(), b"second refusal".as_slice()),
            (b"third scope".as_slice(), b"third refusal".as_slice()),
            (b"fourth scope".as_slice(), b"fourth refusal".as_slice()),
        ]) {
            assert_eq!(allocation.reference().scope, scope);
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision: exact } if exact == decision)
            );
            assert!(allocation.read_live_secret().await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_generic_preserves_prior_lifecycle_after_fourth_erasure_and_reopen(
    ) -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let second = channel.allocate(b"second scope", b"second secret").await?;
        let third = channel.allocate(b"third scope", b"third secret").await?;
        let fourth = channel.allocate(b"fourth scope", b"fourth secret").await?;
        let fifth = channel.allocate(b"fifth scope", b"fifth secret").await?;
        fourth
            .decide_negative(b"fourth refusal")
            .await?
            .retire()
            .await?;
        assert!(fourth.read_live_secret().await.is_err());
        assert_eq!(fifth.read_live_secret().await?, b"fifth secret");
        drop((first, second, third, fourth, fifth, channel, selected));

        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 5);
        for (scope, secret, refusal) in [
            (
                b"first scope".as_slice(),
                b"first secret".as_slice(),
                b"first refusal".as_slice(),
            ),
            (
                b"second scope".as_slice(),
                b"second secret".as_slice(),
                b"second refusal".as_slice(),
            ),
            (
                b"third scope".as_slice(),
                b"third secret".as_slice(),
                b"third refusal".as_slice(),
            ),
        ] {
            let allocation = recovered
                .iter()
                .find(|allocation| allocation.reference().scope == scope)
                .ok_or("prior allocation missing after generic reopen")?;
            assert_eq!(allocation.read_live_secret().await?, secret);
            allocation.decide_negative(refusal).await?.retire().await?;
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision } if decision == refusal)
            );
        }
        drop((recovered, channel, reopened));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 5);
        for (scope, refusal) in [
            (b"first scope".as_slice(), b"first refusal".as_slice()),
            (b"second scope".as_slice(), b"second refusal".as_slice()),
            (b"third scope".as_slice(), b"third refusal".as_slice()),
            (b"fourth scope".as_slice(), b"fourth refusal".as_slice()),
        ] {
            let allocation = recovered
                .iter()
                .find(|allocation| allocation.reference().scope == scope)
                .ok_or("original allocation missing after generic retirement")?;
            assert!(
                matches!(allocation.state().await?, SecretLifetimeState::Retired { decision } if decision == refusal)
            );
            assert!(allocation.read_live_secret().await.is_err());
        }
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_generic_transfer_replays_original_root_interrupt() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let inner = Arc::new(PlatformLifetimeInner {
            owned: ProfileOwnedSecureStorage {
                backend: owned.backend.clone(),
                _owner: owned._owner.clone(),
            },
            identity: owned._owner.lifetime_provider_identity.clone(),
            state: Mutex::new(OwnerState::Uninitialized),
        });
        let provider = PlatformLifetimeProvider(inner.clone());
        assert!(provider.recover_owned_inventory().await?.is_empty());
        for index in 0..4 {
            let scope = format!("scope-{index}");
            provider
                .allocate(scope.as_bytes(), b"original secret")
                .await?;
        }
        let state = inner.state.lock().await;
        let OwnerState::Four(four) = &*state else {
            return Err("original four owner was not retained".into());
        };
        let interrupted =
            append::third::fourth::generic::interrupt_transfer_after_root(&inner.owned, four).await;
        assert!(matches!(
            interrupted,
            Err(AuraError::Invalid { message, .. })
                if message == "platform birth interrupted at fixture boundary"
        ));
        drop(state);
        let fifth = provider.allocate(b"fifth scope", b"fifth secret").await?;
        assert_eq!(fifth.read_live_secret().await?, b"fifth secret");
        assert_eq!(provider.recover_owned_inventory().await?.len(), 5);
        drop((fifth, provider, inner, selected));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        assert_eq!(channel.recover_owned_inventory().await?.len(), 5);
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_generic_refuses_changed_prior_credential_before_sixth_birth(
    ) -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        for index in 0..5 {
            let scope = format!("scope-{index}");
            channel
                .allocate(scope.as_bytes(), b"original secret")
                .await?;
        }
        let platform = selected_owner(owned)?.0;
        let prior = platform.entry_for_user(RECORD_USER)?;
        let exact = prior.get_secret()?;
        let root_before = platform.entry_for_user(KEYRING_USER)?.get_secret()?;
        prior.set_secret(b"changed prior credential")?;
        assert!(channel
            .allocate(b"sixth scope", b"sixth secret")
            .await
            .is_err());
        assert_eq!(
            platform.entry_for_user(KEYRING_USER)?.get_secret()?,
            root_before
        );
        prior.set_secret(&exact)?;
        let sixth = channel.allocate(b"sixth scope", b"sixth secret").await?;
        assert_eq!(sixth.read_live_secret().await?, b"sixth secret");
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_generic_fifth_retirement_and_sixth_positive_reopen() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        for index in 0..4 {
            let scope = format!("scope-{index}");
            channel
                .allocate(scope.as_bytes(), b"original secret")
                .await?;
        }
        let fifth = channel.allocate(b"fifth scope", b"fifth secret").await?;
        let sixth = channel.allocate(b"sixth scope", b"sixth secret").await?;
        fifth
            .decide_negative(b"fifth refusal")
            .await?
            .retire()
            .await?;
        sixth.decide_positive(b"sixth approval").await?;
        assert!(
            matches!(fifth.state().await?, SecretLifetimeState::Retired { decision } if decision == b"fifth refusal")
        );
        assert!(fifth.read_live_secret().await.is_err());
        assert!(
            matches!(sixth.state().await?, SecretLifetimeState::Positive { decision } if decision == b"sixth approval")
        );
        assert_eq!(sixth.read_live_secret().await?, b"sixth secret");
        drop((fifth, sixth, channel, selected));

        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 6);
        let fifth = recovered
            .iter()
            .find(|allocation| allocation.reference().scope == b"fifth scope")
            .ok_or("fifth selected allocation missing")?;
        let sixth = recovered
            .iter()
            .find(|allocation| allocation.reference().scope == b"sixth scope")
            .ok_or("sixth selected allocation missing")?;
        assert!(
            matches!(fifth.state().await?, SecretLifetimeState::Retired { decision } if decision == b"fifth refusal")
        );
        assert!(
            matches!(sixth.state().await?, SecretLifetimeState::Positive { decision } if decision == b"sixth approval")
        );
        assert_eq!(sixth.read_live_secret().await?, b"sixth secret");
        Ok(())
    }

    async fn selected_adapter_generic_workload(count: usize) -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        append::third::fourth::generic::profile::reset();
        let birth_started = crate::time::monotonic_now();
        for index in 0..count {
            let scope = format!("scope-{index}");
            let allocation = channel
                .allocate(scope.as_bytes(), b"measured secret")
                .await?;
            assert_eq!(allocation.reference().scope, scope.as_bytes());
        }
        let birth_elapsed = birth_started.elapsed();
        if count == aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT {
            assert!(matches!(
                channel.allocate(b"over capacity", b"secret").await,
                Err(AuraError::Invalid { message, .. })
                    if message == "platform generic roster capacity exhausted"
            ));
        }
        let [verify_ns, pre_commit_ns, commit_ns, post_commit_ns] =
            append::third::fourth::generic::profile::snapshot();
        drop((channel, selected));
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovery_started = crate::time::monotonic_now();
        let recovered = channel.recover_owned_inventory().await?;
        let recovery_elapsed = recovery_started.elapsed();
        assert_eq!(recovered.len(), count);
        for index in [0, 3, 4, 63, 64, 255, count - 1]
            .into_iter()
            .filter(|index| *index < count)
        {
            let scope = format!("scope-{index}");
            let allocation = recovered
                .iter()
                .find(|allocation| allocation.reference().scope == scope.as_bytes())
                .ok_or("selected adapter lost an original scope")?;
            assert_eq!(allocation.read_live_secret().await?, b"measured secret");
        }
        drop((recovered, channel, reopened));
        let cleanup_started = crate::time::monotonic_now();
        drop(fixture);
        let cleanup_elapsed = cleanup_started.elapsed();
        println!(
            "selected adapter {count}: birth_ms={} verify_ms={} pre_commit_ms={} commit_ms={} post_commit_ms={} recovery_ms={} cleanup_ms={}",
            birth_elapsed.as_millis(),
            verify_ns / 1_000_000,
            pre_commit_ns / 1_000_000,
            commit_ns / 1_000_000,
            post_commit_ns / 1_000_000,
            recovery_elapsed.as_millis(),
            cleanup_elapsed.as_millis(),
        );
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_generic_65_births_cross_first_page() -> TestResult {
        selected_adapter_generic_workload(65).await
    }

    #[tokio::test]
    async fn selected_adapter_generic_256_births_recover_original_scope() -> TestResult {
        selected_adapter_generic_workload(256).await
    }

    #[tokio::test]
    async fn selected_adapter_generic_4096_births_recover_and_refuse_4097() -> TestResult {
        selected_adapter_generic_workload(
            aura_core::effects::secret_lifetime::MAX_PROFILE_ALLOCATION_COUNT,
        )
        .await
    }

    #[tokio::test]
    async fn selected_adapter_fourth_refuses_changed_prior_credential() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        channel.allocate(b"second scope", b"second secret").await?;
        channel.allocate(b"third scope", b"third secret").await?;
        channel.allocate(b"fourth scope", b"fourth secret").await?;
        let platform = selected_owner(owned)?.0;
        let record = platform.entry_for_user(RECORD_USER)?;
        let original = record.get_secret()?;
        record.set_secret(b"changed prior credential")?;
        let root_before = platform.entry_for_user(KEYRING_USER)?.get_secret()?;
        assert!(first.read_live_secret().await.is_err());
        assert!(first.decide_positive(b"cannot decide").await.is_err());
        assert_eq!(
            platform.entry_for_user(KEYRING_USER)?.get_secret()?,
            root_before
        );
        record.set_secret(&original)?;
        assert_eq!(first.read_live_secret().await?, b"first secret");
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_third_refuses_changed_prior_credential() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        channel.allocate(b"second scope", b"second secret").await?;
        channel.allocate(b"third scope", b"third secret").await?;
        let platform = selected_owner(owned)?.0;
        let record = platform.entry_for_user(RECORD_USER)?;
        let original = record.get_secret()?;
        record.set_secret(b"changed prior credential")?;
        let root_before = platform.entry_for_user(KEYRING_USER)?.get_secret()?;
        assert!(first.read_live_secret().await.is_err());
        assert!(first.decide_positive(b"cannot decide").await.is_err());
        assert_eq!(
            platform.entry_for_user(KEYRING_USER)?.get_secret()?,
            root_before
        );
        record.set_secret(&original)?;
        assert_eq!(first.read_live_secret().await?, b"first secret");
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_reopens_retired_first_with_original_scope() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let negative = first.decide_negative(b"original refusal").await?;
        negative.retire().await?;
        negative.retire().await?;
        assert!(
            matches!(first.state().await?, SecretLifetimeState::Retired { decision } if decision == b"original refusal")
        );
        drop(negative);
        drop(first);
        drop(channel);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let mut recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 1);
        let first = recovered.pop().expect("first");
        assert_eq!(first.reference().scope, b"first scope");
        assert!(
            matches!(first.state().await?, SecretLifetimeState::Retired { decision } if decision == b"original refusal")
        );
        assert!(first.read_live_secret().await.is_err());
        assert!(channel.reconcile_unhanded_births().await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_reopens_both_original_scopes_and_reconciles_once() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let second = channel.allocate(b"second scope", b"second secret").await?;
        let first_negative = first.decide_negative(b"first refusal").await?;
        let second_negative = second.decide_negative(b"second refusal").await?;
        first_negative.retire().await?;
        first_negative.retire().await?;
        second_negative.retire().await?;
        second_negative.retire().await?;
        drop(first_negative);
        drop(second_negative);
        drop(first);
        drop(second);
        drop(channel);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let owners = channel.recover_owned_inventory().await?;
        assert_eq!(owners.len(), 2);
        let mut scopes: Vec<_> = owners
            .iter()
            .map(|owner| owner.reference().scope.clone())
            .collect();
        scopes.sort();
        assert_eq!(
            scopes,
            vec![b"first scope".to_vec(), b"second scope".to_vec()]
        );
        for owner in owners {
            assert!(matches!(
                owner.state().await?,
                SecretLifetimeState::Retired { .. }
            ));
            assert!(owner.read_live_secret().await.is_err());
        }
        assert!(channel.reconcile_unhanded_births().await?.is_empty());
        assert!(channel.reconcile_unhanded_births().await?.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_refuses_foreign_profile_and_missing_root_evidence() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        channel.recover_owned_inventory().await?;
        let first = channel.allocate(b"first scope", b"first secret").await?;
        let foreign = Fixture::new()?;
        let foreign_selected = foreign.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(foreign_owned) = &foreign_selected else {
            unreachable!()
        };
        let mut foreign_channel = selected_profile_channel(foreign_owned)?;
        assert!(!foreign_channel.belongs_to_provider(&owned._owner.lifetime_provider_identity));
        assert!(foreign_channel.recover_owned_inventory().await?.is_empty());
        drop(foreign_channel);
        drop(foreign_selected);
        let platform = selected_owner(owned)?.0;
        platform.entry_for_user(KEYRING_USER)?.delete_credential()?;
        drop(first);
        drop(channel);
        drop(selected);
        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        assert!(channel.recover_owned_inventory().await.is_err());
        assert!(!owned._owner.lifetime_root_claimed.load(Ordering::Acquire));
        Ok(())
    }

    #[tokio::test]
    async fn selected_adapter_recovers_original_interrupted_first_birth() -> TestResult {
        let fixture = Fixture::new()?;
        let selected = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &selected else {
            unreachable!()
        };
        let empty = admit_empty_root(owned).await?;
        assert!(allocate_one_inner(
            owned,
            &empty,
            b"interrupted scope",
            b"interrupted secret",
            StopAfter::Inventory,
        )
        .await
        .is_err());
        drop(empty);
        drop(selected);

        let reopened = fixture.selected()?;
        let ProductionSecureStorageHandler::ProfileOwned(owned) = &reopened else {
            unreachable!()
        };
        let mut channel = selected_profile_channel(owned)?;
        let recovered = channel.recover_owned_inventory().await?;
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].reference().scope, b"interrupted scope");
        assert_eq!(
            recovered[0].read_live_secret().await?,
            b"interrupted secret"
        );
        assert!(channel.reconcile_unhanded_births().await?.is_empty());
        Ok(())
    }
}
