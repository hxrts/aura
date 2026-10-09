//! One participant's side of a context DKG (work/8.md Tasks 55/59).
//!
//! A relational context's member authorities run a FROST DKG
//! ([`super::frost_rounds`]) to obtain shares for the context threshold PRF
//! (docs/100 §7.5). [`ContextDkgSession`] is the pure state machine for one
//! participant: it emits the messages to send, consumes the messages it
//! receives in any order, and finishes with the participant's key package and
//! the group's public key package. Round-two packages are private to their
//! recipient, so they go through a [`DkgSealer`] (the runtime seals them to the
//! recipient's device key); round-one packages are public broadcasts.

use super::frost_rounds::{participant_identifier, round_one, round_three, round_two};
use super::types::DkgConfig;
use aura_core::{AuraError, AuthorityId, Result};
use frost_ed25519::keys::dkg::{round1, round2};
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage, VerifiableSecretSharingCommitment};
use frost_ed25519::Identifier;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Seals and opens round-two packages between participants.
#[async_trait::async_trait]
pub trait DkgSealer: Send + Sync {
    /// Seal `plaintext` so only `recipient` can open it.
    async fn seal(&self, recipient: AuthorityId, plaintext: &[u8]) -> Result<Vec<u8>>;
    /// Open a package `sender` sealed to this participant.
    async fn open(&self, sender: AuthorityId, sealed: &[u8]) -> Result<Vec<u8>>;
}

/// A DKG message on the wire, bound to the DKG it belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextDkgMessage {
    /// DKG epoch from the config (binds the message to one ceremony).
    pub epoch: u64,
    /// Sender authority.
    pub from: AuthorityId,
    /// Payload.
    pub body: ContextDkgBody,
}

/// DKG message payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextDkgBody {
    /// Public round-one package, sent to every other participant.
    RoundOne(Vec<u8>),
    /// Round-two package sealed to `to`.
    RoundTwo {
        /// Recipient authority.
        to: AuthorityId,
        /// Sealed serialized package.
        sealed: Vec<u8>,
    },
}

/// A message this participant must deliver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    /// Recipients (every other participant for a broadcast).
    pub to: Vec<AuthorityId>,
    /// The message.
    pub message: ContextDkgMessage,
}

/// The finished native DKG for this exact participant and original config.
/// Completion proves native key generation, not application policy approval.
/// A caller cannot replace the retained threshold or participant mapping.
///
/// ```compile_fail,E0451
/// use aura_consensus::dkg::context_session::ContextDkgOutput;
/// fn forge() -> ContextDkgOutput {
///     ContextDkgOutput {
///         key_package: todo!(),
///         public_key_package: todo!(),
///         config: todo!(),
///         participant: todo!(),
///         vss_commitment: todo!(),
///     }
/// }
/// ```
#[derive(Debug)]
pub struct ContextDkgOutput {
    key_package: KeyPackage,
    public_key_package: PublicKeyPackage,
    config: DkgConfig,
    participant: AuthorityId,
    vss_commitment: VerifiableSecretSharingCommitment,
}

impl ContextDkgOutput {
    /// The original configuration consumed by this native session.
    pub fn config(&self) -> &DkgConfig {
        &self.config
    }

    /// The exact local participant whose native share completed.
    pub fn participant(&self) -> AuthorityId {
        self.participant
    }

    /// Borrow native private material for the sanctioned retention boundary.
    pub fn key_package(&self) -> &KeyPackage {
        &self.key_package
    }

    /// Borrow the original native public package without inferring a threshold.
    pub fn public_key_package(&self) -> &PublicKeyPackage {
        &self.public_key_package
    }

    /// Original aggregate polynomial commitment for audited share repair.
    /// Its public package was checked against the original native output.
    pub fn vss_commitment(&self) -> &VerifiableSecretSharingCommitment {
        &self.vss_commitment
    }
}

enum Phase {
    AwaitingRoundOne(round1::SecretPackage),
    AwaitingRoundTwo(round2::SecretPackage),
    Done,
}

/// One participant's DKG state machine.
pub struct ContextDkgSession {
    config: DkgConfig,
    me: AuthorityId,
    phase: Phase,
    round_one: BTreeMap<Identifier, round1::Package>,
    round_two: BTreeMap<Identifier, round2::Package>,
    pending_round_two: Vec<ContextDkgMessage>,
    own_commitment: VerifiableSecretSharingCommitment,
}

fn codec(error: impl std::fmt::Display) -> AuraError {
    AuraError::serialization(format!("context DKG package: {error}"))
}

impl ContextDkgSession {
    /// Start the DKG: returns the session and this participant's round-one
    /// broadcast.
    pub fn start<R: RngCore + CryptoRng>(
        config: DkgConfig,
        me: AuthorityId,
        rng: &mut R,
    ) -> Result<(Self, Outgoing)> {
        let (secret, package) = round_one(&config, me, rng)?;
        let own_commitment = package.commitment().clone();
        let broadcast = Outgoing {
            to: config
                .participants
                .iter()
                .copied()
                .filter(|participant| *participant != me)
                .collect(),
            message: ContextDkgMessage {
                epoch: config.epoch,
                from: me,
                body: ContextDkgBody::RoundOne(package.serialize().map_err(codec)?),
            },
        };
        Ok((
            Self {
                config,
                me,
                phase: Phase::AwaitingRoundOne(secret),
                round_one: BTreeMap::new(),
                round_two: BTreeMap::new(),
                pending_round_two: Vec::new(),
                own_commitment,
            },
            broadcast,
        ))
    }

    fn others(&self) -> usize {
        self.config.participants.len().saturating_sub(1)
    }

    /// Consume a received message. Returns messages to send and, once all
    /// rounds complete, the output. Messages for another epoch, from a
    /// non-participant, duplicated, or addressed to someone else are rejected.
    pub async fn receive(
        &mut self,
        message: ContextDkgMessage,
        sealer: &dyn DkgSealer,
    ) -> Result<(Vec<Outgoing>, Option<ContextDkgOutput>)> {
        if message.epoch != self.config.epoch {
            return Err(AuraError::invalid("DKG message for another epoch"));
        }
        if message.from == self.me {
            return Err(AuraError::invalid("DKG message from self"));
        }
        let sender = participant_identifier(&self.config, message.from)?;
        match message.body {
            ContextDkgBody::RoundOne(bytes) => {
                if self.round_one.contains_key(&sender) {
                    return Err(AuraError::invalid("duplicate round-one package"));
                }
                let package = round1::Package::deserialize(&bytes).map_err(codec)?;
                self.round_one.insert(sender, package);
                self.advance(sealer).await
            }
            ContextDkgBody::RoundTwo { to, .. } if to != self.me => Err(AuraError::invalid(
                "round-two package addressed to another participant",
            )),
            ContextDkgBody::RoundTwo { .. } => {
                if matches!(self.phase, Phase::AwaitingRoundOne(_)) {
                    // Arrived before we finished round one; replay it after.
                    self.pending_round_two.push(message);
                    return Ok((Vec::new(), None));
                }
                self.accept_round_two(sender, message, sealer).await?;
                self.advance(sealer).await
            }
        }
    }

    async fn accept_round_two(
        &mut self,
        sender: Identifier,
        message: ContextDkgMessage,
        sealer: &dyn DkgSealer,
    ) -> Result<()> {
        let ContextDkgBody::RoundTwo { sealed, .. } = message.body else {
            return Ok(());
        };
        if self.round_two.contains_key(&sender) {
            return Err(AuraError::invalid("duplicate round-two package"));
        }
        let opened = sealer.open(message.from, &sealed).await?;
        let package = round2::Package::deserialize(&opened).map_err(codec)?;
        self.round_two.insert(sender, package);
        Ok(())
    }

    async fn advance(
        &mut self,
        sealer: &dyn DkgSealer,
    ) -> Result<(Vec<Outgoing>, Option<ContextDkgOutput>)> {
        let mut outgoing = Vec::new();
        if matches!(self.phase, Phase::AwaitingRoundOne(_)) && self.round_one.len() == self.others()
        {
            let Phase::AwaitingRoundOne(secret) = std::mem::replace(&mut self.phase, Phase::Done)
            else {
                return Ok((outgoing, None));
            };
            let (secret, packages) = round_two(secret, &self.round_one)?;
            for (recipient_id, package) in packages {
                let recipient = self
                    .config
                    .participants
                    .iter()
                    .copied()
                    .find(|authority| {
                        participant_identifier(&self.config, *authority).ok() == Some(recipient_id)
                    })
                    .ok_or_else(|| {
                        AuraError::invalid("round-two recipient is not a participant")
                    })?;
                let sealed = sealer
                    .seal(recipient, &package.serialize().map_err(codec)?)
                    .await?;
                outgoing.push(Outgoing {
                    to: vec![recipient],
                    message: ContextDkgMessage {
                        epoch: self.config.epoch,
                        from: self.me,
                        body: ContextDkgBody::RoundTwo {
                            to: recipient,
                            sealed,
                        },
                    },
                });
            }
            self.phase = Phase::AwaitingRoundTwo(secret);
            for pending in std::mem::take(&mut self.pending_round_two) {
                let sender = participant_identifier(&self.config, pending.from)?;
                self.accept_round_two(sender, pending, sealer).await?;
            }
        }
        if let Phase::AwaitingRoundTwo(secret) = &self.phase {
            if self.round_two.len() == self.others() {
                let (key_package, public_key_package) =
                    round_three(secret, &self.round_one, &self.round_two)?;
                let commitments: Vec<_> = std::iter::once(&self.own_commitment)
                    .chain(self.round_one.values().map(|package| package.commitment()))
                    .collect();
                let vss_commitment =
                    frost_core::keys::sum_commitments(&commitments).map_err(|source| {
                        AuraError::Crypto {
                            message: "retain original context DKG commitment".into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
                let identifiers = public_key_package
                    .verifying_shares()
                    .keys()
                    .copied()
                    .collect();
                let reconstructed =
                    PublicKeyPackage::from_commitment(&identifiers, &vss_commitment).map_err(
                        |source| AuraError::Crypto {
                            message: "validate original context DKG commitment".into(),
                            source: Some(Arc::new(source)),
                        },
                    )?;
                if reconstructed != public_key_package {
                    return Err(AuraError::invalid(
                        "aggregate DKG commitment differs from original public package",
                    ));
                }
                self.phase = Phase::Done;
                return Ok((
                    outgoing,
                    Some(ContextDkgOutput {
                        key_package,
                        public_key_package,
                        config: self.config.clone(),
                        participant: self.me,
                        vss_commitment,
                    }),
                ));
            }
        }
        Ok((outgoing, None))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use aura_core::Hash32;
    use rand::SeedableRng;
    use std::collections::VecDeque;

    /// Test sealer: XOR with a per-pair pad, and a tag binding the pair, so a
    /// package opened by the wrong participant or from the wrong sender fails.
    struct PairSealer {
        me: AuthorityId,
    }

    fn pad(a: AuthorityId, b: AuthorityId) -> u8 {
        a.to_bytes()[0] ^ b.to_bytes()[0] ^ 0x5a
    }

    #[async_trait::async_trait]
    impl DkgSealer for PairSealer {
        async fn seal(&self, recipient: AuthorityId, plaintext: &[u8]) -> Result<Vec<u8>> {
            let mut out = recipient.to_bytes()[..4].to_vec();
            out.extend(plaintext.iter().map(|byte| byte ^ pad(self.me, recipient)));
            Ok(out)
        }
        async fn open(&self, sender: AuthorityId, sealed: &[u8]) -> Result<Vec<u8>> {
            if sealed.len() < 4 || sealed[..4] != self.me.to_bytes()[..4] {
                return Err(AuraError::crypto("sealed to another participant"));
            }
            Ok(sealed[4..]
                .iter()
                .map(|byte| byte ^ pad(sender, self.me))
                .collect())
        }
    }

    fn config(threshold: u16, total: u8) -> DkgConfig {
        DkgConfig {
            epoch: 3,
            threshold,
            max_signers: u16::from(total),
            membership_hash: Hash32::default(),
            cutoff: 0,
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            participants: (1..=total)
                .map(|seed| AuthorityId::new_from_entropy([seed; 32]))
                .collect(),
        }
    }

    /// Deliver messages through an in-memory bus, optionally reversing each
    /// delivery batch to exercise out-of-order arrival.
    async fn run(config: &DkgConfig, reverse: bool) -> Vec<ContextDkgOutput> {
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(7);
        let mut sessions = BTreeMap::new();
        let mut bus: VecDeque<(AuthorityId, ContextDkgMessage)> = VecDeque::new();
        for me in &config.participants {
            let (session, broadcast) =
                ContextDkgSession::start(config.clone(), *me, &mut rng).unwrap();
            sessions.insert(*me, session);
            for to in broadcast.to {
                bus.push_back((to, broadcast.message.clone()));
            }
        }
        let mut outputs = BTreeMap::new();
        while let Some((to, message)) = if reverse {
            bus.pop_back()
        } else {
            bus.pop_front()
        } {
            let sealer = PairSealer { me: to };
            let (outgoing, output) = sessions
                .get_mut(&to)
                .unwrap()
                .receive(message, &sealer)
                .await
                .unwrap();
            for out in outgoing {
                for recipient in out.to {
                    bus.push_back((recipient, out.message.clone()));
                }
            }
            if let Some(output) = output {
                outputs.insert(to, output);
            }
        }
        config
            .participants
            .iter()
            .map(|p| outputs.remove(p).expect("finished"))
            .collect()
    }

    #[tokio::test]
    async fn every_member_finishes_with_the_same_group_key() {
        for reverse in [false, true] {
            let outputs = run(&config(2, 3), reverse).await;
            let group = outputs[0].public_key_package.verifying_key();
            assert!(outputs
                .iter()
                .all(|output| output.public_key_package.verifying_key() == group));
        }
    }

    #[tokio::test]
    async fn original_aggregate_commitment_repairs_the_exact_native_share() {
        let original = config(2, 3);
        let outputs = run(&original, true).await;
        for (output, participant) in outputs.iter().zip(&original.participants) {
            assert_eq!(output.config(), &original);
            assert_eq!(output.participant(), *participant);
            assert_eq!(output.vss_commitment(), outputs[0].vss_commitment());
        }
        let target = *outputs[2].key_package().identifier();
        let helpers: Vec<_> = outputs[..2]
            .iter()
            .map(|output| *output.key_package().identifier())
            .collect();
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(217);
        let deltas: Vec<_> = outputs[..2]
            .iter()
            .map(|output| {
                let share = frost_ed25519::keys::SecretShare::new(
                    *output.key_package().identifier(),
                    *output.key_package().signing_share(),
                    output.vss_commitment().clone(),
                );
                frost_core::keys::repairable::repair_share_step_1(
                    &helpers, &share, &mut rng, target,
                )
                .unwrap()
            })
            .collect();
        let sigmas: Vec<_> = helpers
            .iter()
            .map(|helper| {
                let received: Vec<_> = deltas.iter().map(|row| *row.get(helper).unwrap()).collect();
                frost_core::keys::repairable::repair_share_step_2::<frost_ed25519::Ed25519Sha512>(
                    &received,
                )
            })
            .collect();
        let repair = |target, commitment| {
            frost_core::keys::repairable::repair_share_step_3::<frost_ed25519::Ed25519Sha512>(
                &sigmas, target, commitment,
            )
        };
        let repaired = KeyPackage::try_from(repair(target, outputs[0].vss_commitment())).unwrap();
        assert_eq!(
            repaired.signing_share(),
            outputs[2].key_package().signing_share()
        );
        assert_eq!(
            repaired.verifying_key(),
            outputs[2].key_package().verifying_key()
        );
        assert!(KeyPackage::try_from(repair(helpers[0], outputs[0].vss_commitment())).is_err());
        let foreign = run(&config(3, 3), false).await;
        assert!(KeyPackage::try_from(repair(target, foreign[0].vss_commitment())).is_err());
    }

    #[tokio::test]
    async fn foreign_epoch_duplicate_and_misaddressed_messages_are_rejected() {
        let config = config(2, 3);
        let mut rng = rand_chacha::ChaCha20Rng::seed_from_u64(9);
        let (a, b) = (config.participants[0], config.participants[1]);
        let (mut session, _) = ContextDkgSession::start(config.clone(), a, &mut rng).unwrap();
        let (_, from_b) = ContextDkgSession::start(config.clone(), b, &mut rng).unwrap();
        let sealer = PairSealer { me: a };

        let mut wrong_epoch = from_b.message.clone();
        wrong_epoch.epoch += 1;
        assert!(session.receive(wrong_epoch, &sealer).await.is_err());

        session
            .receive(from_b.message.clone(), &sealer)
            .await
            .unwrap();
        assert!(
            session.receive(from_b.message, &sealer).await.is_err(),
            "duplicate"
        );

        let misaddressed = ContextDkgMessage {
            epoch: config.epoch,
            from: b,
            body: ContextDkgBody::RoundTwo {
                to: config.participants[2],
                sealed: vec![0; 8],
            },
        };
        assert!(session.receive(misaddressed, &sealer).await.is_err());

        let outsider = ContextDkgMessage {
            epoch: config.epoch,
            from: AuthorityId::new_from_entropy([99; 32]),
            body: ContextDkgBody::RoundOne(vec![1, 2, 3]),
        };
        assert!(session.receive(outsider, &sealer).await.is_err());
    }
}
