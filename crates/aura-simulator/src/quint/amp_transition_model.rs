//! Observational transition-policy replay. This module never publishes runtime
//! certificate/finalization facts and does not prove witness or consensus custody.
use super::super::action_registry::{ActionBuilder, ActionRegistry};
use super::{channel_id_from_input, param_string, success_result, AmpChannelHarness};
use aura_core::{AuraError, AuthorityId, ChannelId, ContextId, Hash32, Result};
use aura_journal::fact::{AmpTransitionIdentity, AmpTransitionPolicy};
use aura_journal::reduction::AmpTransitionReductionStatus as Status;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Clone)]
struct Observation {
    identity: AmpTransitionIdentity,
    status: Status,
    suspect: Option<AuthorityId>,
    readable_state_destroyed: bool,
}
struct NativeScopeObservation {
    context: ContextId,
    channel: ChannelId,
    epoch: u64,
    actor: AuthorityId,
    members: BTreeSet<AuthorityId>,
}
#[derive(Default)]
struct TransitionModel {
    epoch: Option<u64>,
    native_anchor: Option<(ContextId, ChannelId, u64)>,
    records: BTreeMap<String, Observation>,
    conflicted: BTreeSet<Hash32>,
}
fn reject(detail: &str) -> AuraError {
    AuraError::invalid(format!("observational AMP transition model: {detail}"))
}
fn same_parent(left: &Observation, right: &Observation) -> bool {
    left.identity.context == right.identity.context
        && left.identity.channel == right.identity.channel
        && left.identity.parent_epoch == right.identity.parent_epoch
        && left.identity.parent_commitment == right.identity.parent_commitment
}
impl TransitionModel {
    fn propose(
        &mut self,
        label: String,
        observed: &NativeScopeObservation,
        policy: AmpTransitionPolicy,
        suspect: Option<AuthorityId>,
    ) -> Result<()> {
        let (context, channel, native_epoch, actor, members) = (
            observed.context,
            observed.channel,
            observed.epoch,
            observed.actor,
            &observed.members,
        );
        if !members.contains(&actor) {
            return Err(reject(
                "coordinator not in actual observed channel membership",
            ));
        }
        if label.is_empty() || self.records.contains_key(&label) {
            return Err(reject("duplicate or missing transition label"));
        }
        let emergency = matches!(
            policy,
            AmpTransitionPolicy::EmergencyQuarantineTransition
                | AmpTransitionPolicy::EmergencyCryptoshredTransition
        );
        if emergency != suspect.is_some() || suspect == Some(actor) {
            return Err(reject("invalid exact emergency suspect"));
        }
        if let Some(anchor) = self.native_anchor {
            if anchor != (context, channel, native_epoch) {
                return Err(reject("foreign or changed native base scope"));
            }
        }
        let parent_epoch = self.epoch.unwrap_or(native_epoch);
        let successor_epoch = parent_epoch
            .checked_add(1)
            .ok_or_else(|| reject("epoch overflow"))?;
        let encoded =
            aura_core::util::serialization::to_vec(&(context, channel, parent_epoch, members))
                .map_err(|source| AuraError::Serialization {
                    message: "encode exact observational transition scope".into(),
                    source: Some(Arc::new(source)),
                })?;
        let identity = AmpTransitionIdentity {
            context,
            channel,
            parent_epoch,
            successor_epoch,
            parent_commitment: Hash32::from_bytes(&encoded),
            successor_commitment: Hash32::from_bytes(
                format!("aura.simulator.observational-transition:{label}").as_bytes(),
            ),
            membership_commitment: Hash32::from_bytes(
                &aura_core::util::serialization::to_vec(members).map_err(|source| {
                    AuraError::Serialization {
                        message: "encode observed model membership".into(),
                        source: Some(Arc::new(source)),
                    }
                })?,
            ),
            transition_policy: policy,
        };
        self.native_anchor = Some((context, channel, native_epoch));
        self.epoch = Some(parent_epoch);
        self.records.insert(
            label,
            Observation {
                identity,
                status: Status::Observed,
                suspect,
                readable_state_destroyed: matches!(
                    policy,
                    AmpTransitionPolicy::EmergencyCryptoshredTransition
                ),
            },
        );
        Ok(())
    }
    fn certify(&mut self, label: &str) -> Result<()> {
        let selected = self
            .records
            .get(label)
            .ok_or_else(|| reject("certificate phase has no original proposal"))?;
        if selected.status != Status::Observed {
            return Err(reject(
                "certificate phase requires original observed proposal",
            ));
        }
        if self.records.iter().any(|(other, record)| {
            other != label
                && same_parent(selected, record)
                && matches!(record.status, Status::A2Live | Status::A3Finalized)
        }) {
            return Err(reject("another live successor exists"));
        }
        self.records
            .get_mut(label)
            .ok_or_else(|| reject("original proposal disappeared"))?
            .status = Status::A2Live;
        Ok(())
    }
    fn finalize(&mut self, label: &str) -> Result<()> {
        let selected = self
            .records
            .get(label)
            .ok_or_else(|| reject("finalization phase has no original proposal"))?;
        if selected.status != Status::A2Live {
            return Err(reject("finalization requires selected model A2 phase"));
        }
        if self.records.iter().any(|(other, record)| {
            other != label && same_parent(selected, record) && record.status == Status::A3Finalized
        }) {
            return Err(reject("another finalized successor exists"));
        }
        self.epoch = Some(selected.identity.successor_epoch);
        self.records
            .get_mut(label)
            .ok_or_else(|| reject("original proposal disappeared"))?
            .status = Status::A3Finalized;
        Ok(())
    }
    fn conflict(&mut self, left: &str, right: &str) -> Result<()> {
        if left == right {
            return Err(reject("conflict requires distinct original proposals"));
        }
        let a = self
            .records
            .get(left)
            .ok_or_else(|| reject("missing left original proposal"))?;
        let b = self
            .records
            .get(right)
            .ok_or_else(|| reject("missing right original proposal"))?;
        if !same_parent(a, b) {
            return Err(reject("conflict has foreign parent scope"));
        }
        for label in [left, right] {
            let selected = self
                .records
                .get_mut(label)
                .ok_or_else(|| reject("original proposal disappeared"))?;
            self.conflicted.insert(selected.identity.transition_id());
            selected.status = if selected.status == Status::A3Finalized {
                Status::A3Conflict
            } else {
                Status::A2Conflict
            };
        }
        Ok(())
    }
    fn check_invariants(&self) -> Result<()> {
        if self.records.is_empty() {
            return Err(reject("no transition observations"));
        }
        for record in self.records.values() {
            let live = self
                .records
                .values()
                .filter(|other| {
                    same_parent(record, other)
                        && matches!(other.status, Status::A2Live | Status::A3Finalized)
                })
                .count();
            if live > 1 {
                return Err(reject("multiple live successors"));
            }
            if record.identity.successor_epoch != record.identity.parent_epoch + 1 {
                return Err(reject("invalid bound successor epoch"));
            }
            if self.conflicted.contains(&record.identity.transition_id())
                && matches!(record.status, Status::A2Live | Status::A3Finalized)
            {
                return Err(reject("conflict remains live"));
            }
            if matches!(
                record.identity.transition_policy,
                AmpTransitionPolicy::EmergencyCryptoshredTransition
            ) && !record.readable_state_destroyed
            {
                return Err(reject(
                    "cryptoshred observation lacks explicit destruction policy",
                ));
            }
            if matches!(
                record.identity.transition_policy,
                AmpTransitionPolicy::EmergencyQuarantineTransition
                    | AmpTransitionPolicy::EmergencyCryptoshredTransition
            ) && record.suspect.is_none()
            {
                return Err(reject("emergency observation lacks exact suspect"));
            }
        }
        Ok(())
    }
}

pub(super) fn register(registry: &mut ActionRegistry, harness: Arc<AmpChannelHarness>) {
    let model = Arc::new(Mutex::new(TransitionModel::default()));
    for name in [
        "proposeTransition",
        "certifyTransition",
        "finalizeTransition",
        "reportTransitionConflict",
        "emergencyQuarantine",
        "emergencyCryptoshred",
        "assertTransitionInvariant",
    ] {
        let harness = harness.clone();
        let model = model.clone();
        registry.register(ActionBuilder::new(name).description("Observational AMP transition policy; no native certificate or consensus issuance")
            .execute_fn(move |params, _, state| {
                let params = params.clone(); let state = state.clone(); let harness = harness.clone(); let model = model.clone();
                Box::pin(async move {
                    let label = param_string(&params, &["tid", "transition", "message"]);
                    match name {
                        "proposeTransition" | "emergencyQuarantine" | "emergencyCryptoshred" => {
                            let actor = param_string(&params, &["coordinator", "actor"]).ok_or_else(|| reject("missing original actor"))?;
                            let cid = param_string(&params, &["cid", "channel"]).ok_or_else(|| reject("missing original channel"))?;
                            let label = label.ok_or_else(|| reject("missing transition label"))?;
                            let agent = harness.agent_for(&actor)?;
                            let effects = agent.runtime().effects();
                            let channel = channel_id_from_input(&cid);
                            let native = aura_amp::get_channel_state(effects.as_ref(), harness.context_id(), channel).await?;
                            let members = aura_amp::list_channel_participants(effects.as_ref(), harness.context_id(), channel).await?.into_iter().collect::<BTreeSet<_>>();
                            let policy = match name { "emergencyQuarantine" => AmpTransitionPolicy::EmergencyQuarantineTransition,
                                "emergencyCryptoshred" => AmpTransitionPolicy::EmergencyCryptoshredTransition, _ => AmpTransitionPolicy::NormalTransition };
                            let suspect = if name == "proposeTransition" { None } else {
                                let name = param_string(&params, &["suspect", "member"]).ok_or_else(|| reject("missing exact suspect"))?;
                                let _actual_suspect = harness.agent_for(&name)?;
                                Some(harness.authority_for(&name)?)
                            };
                            let observed = NativeScopeObservation { context: harness.context_id(), channel,
                                epoch: native.chan_epoch, actor: harness.authority_for(&actor)?, members };
                            model.lock().await.propose(label, &observed, policy, suspect)?;
                        }
                        "certifyTransition" => model.lock().await.certify(&label.ok_or_else(|| reject("missing transition label"))?)?,
                        "finalizeTransition" => model.lock().await.finalize(&label.ok_or_else(|| reject("missing transition label"))?)?,
                        "reportTransitionConflict" => {
                            let left = param_string(&params, &["leftId", "left", "message"]).ok_or_else(|| reject("missing left transition"))?;
                            let mut owner = model.lock().await;
                            let right = if let Some(right) = param_string(&params, &["rightId", "right"]) { right } else {
                                let selected = owner.records.get(&left).ok_or_else(|| reject("missing original left transition"))?;
                                let matches = owner.records.iter().filter(|(name, record)| name.as_str() != left && same_parent(selected, record)).map(|(name, _)| name.clone()).collect::<Vec<_>>();
                                if matches.len() != 1 { return Err(reject("conflict does not identify one exact original peer proposal")); }
                                matches[0].clone()
                            };
                            owner.conflict(&left, &right)?;
                        }
                        "assertTransitionInvariant" => {
                            let owner = model.lock().await;
                            owner.check_invariants()?;
                            let (context, channel, original_epoch) = owner.native_anchor.ok_or_else(|| reject("missing original native base"))?;
                            drop(owner);
                            for name in ["bob", "alice"] {
                                let agent = harness.agent_for(name)?;
                                let actual = aura_amp::get_channel_state(agent.runtime().effects().as_ref(), context, channel).await?;
                                if actual.chan_epoch != original_epoch { return Err(reject("observational model changed native channel epoch")); }
                            }
                        }
                        _ => return Err(reject("unsupported transition model action")),
                    }
                    Ok(success_result(state, vec![]))
                })
            }).build());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> NativeScopeObservation {
        let actor = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"aura-simulator.transition-model-negative.actor",
        ));
        let member = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"aura-simulator.transition-model-negative.member",
        ));
        NativeScopeObservation {
            context: ContextId::new_from_entropy(aura_core::hash::hash(
                b"aura-simulator.transition-model-negative.context",
            )),
            channel: ChannelId::from_bytes(aura_core::hash::hash(
                b"aura-simulator.transition-model-negative.channel",
            )),
            epoch: 1,
            actor,
            members: BTreeSet::from([actor, member]),
        }
    }
    fn propose(model: &mut TransitionModel, label: &str, observed: &NativeScopeObservation) {
        if let Err(source) = model.propose(
            label.into(),
            observed,
            AmpTransitionPolicy::NormalTransition,
            None,
        ) {
            panic!("valid scoped observational proposal: {source}");
        }
    }
    #[test]
    fn model_phase_requires_original_proposal_and_selected_live_phase() {
        let observed = scope();
        let mut model = TransitionModel::default();
        assert!(model.certify("missing").is_err());
        assert!(model.finalize("missing").is_err());
        propose(&mut model, "normal", &observed);
        assert!(model.finalize("normal").is_err());
        assert!(model.certify("normal").is_ok());
        assert!(model.certify("normal").is_err());
        assert!(model.finalize("normal").is_ok());
        assert_eq!(model.epoch, Some(2));
        assert_eq!(
            model.native_anchor,
            Some((observed.context, observed.channel, 1))
        );
        assert!(model.check_invariants().is_ok());
    }
    #[test]
    fn model_conflict_requires_exact_parent_and_suppresses_live_successors() {
        let observed = scope();
        let mut model = TransitionModel::default();
        propose(&mut model, "left", &observed);
        propose(&mut model, "right", &observed);
        assert!(model.conflict("left", "left").is_err());
        assert!(model.certify("left").is_ok());
        assert!(model.certify("right").is_err());
        let Some(right) = model.records.get_mut("right") else {
            panic!("actual original model proposal");
        };
        let original = right.identity.parent_commitment;
        right.identity.parent_commitment = Hash32::from_bytes(b"foreign observational parent");
        assert!(model.conflict("left", "right").is_err());
        let Some(right) = model.records.get_mut("right") else {
            panic!("original proposal retained");
        };
        right.identity.parent_commitment = original;
        assert!(model.conflict("left", "right").is_ok());
        assert!(model.check_invariants().is_ok());
        assert!(model.certify("right").is_err());
        let Some(left) = model.records.get_mut("left") else {
            panic!("original conflicting proposal retained");
        };
        left.status = Status::A2Live;
        assert!(
            model.check_invariants().is_err(),
            "fault injection cannot revive a conflicted model successor"
        );
    }
    #[test]
    fn model_emergency_requires_exact_suspect_and_explicit_destruction_policy() {
        let observed = scope();
        let mut model = TransitionModel::default();
        assert!(model
            .propose(
                "missing".into(),
                &observed,
                AmpTransitionPolicy::EmergencyQuarantineTransition,
                None
            )
            .is_err());
        assert!(model
            .propose(
                "self".into(),
                &observed,
                AmpTransitionPolicy::EmergencyCryptoshredTransition,
                Some(observed.actor)
            )
            .is_err());
        let suspect = AuthorityId::new_from_entropy(aura_core::hash::hash(
            b"aura-simulator.transition-model-negative.suspect",
        ));
        assert!(model
            .propose(
                "cryptoshred".into(),
                &observed,
                AmpTransitionPolicy::EmergencyCryptoshredTransition,
                Some(suspect)
            )
            .is_ok());
        assert!(model.check_invariants().is_ok());
        let Some(record) = model.records.get_mut("cryptoshred") else {
            panic!("original emergency observation retained");
        };
        record.readable_state_destroyed = false;
        assert!(model.check_invariants().is_err());
        let mut foreign = scope();
        foreign.actor = suspect;
        assert!(model
            .propose(
                "foreign".into(),
                &foreign,
                AmpTransitionPolicy::NormalTransition,
                None
            )
            .is_err());
    }
}
