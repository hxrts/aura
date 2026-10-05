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
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};
use frost_ed25519::Identifier;
use rand::{CryptoRng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// The finished DKG for this participant.
#[derive(Debug, Clone)]
pub struct ContextDkgOutput {
    /// This participant's share and verifying material.
    pub key_package: KeyPackage,
    /// The group key and every participant's verifying share.
    pub public_key_package: PublicKeyPackage,
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
                self.phase = Phase::Done;
                return Ok((
                    outgoing,
                    Some(ContextDkgOutput {
                        key_package,
                        public_key_package,
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
