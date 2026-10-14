//! Bounded temporary holding for original enrollment VM envelopes. Registration
//! is untrusted broker data until the issuer compares its signature with the
//! independently retained setup verifier. Every taken frame still enters the
//! ordinary guarded transport and VM verification path.

use super::PendingMailboxDelivery;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::hash::hash;
use aura_core::{AuthorityId, DeviceId};
use aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxRegistration;
use aura_invitation::enrollment_mailbox_session::is_original_enrollment_mailbox_session;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

const MAX_REGISTRATIONS: usize = 256;
const MAX_PENDING_ENVELOPES: usize = 32;
const MAX_ENVELOPES_PER_REGISTRATION: usize = 4;
const MAX_ACKNOWLEDGED_PER_REGISTRATION: usize = 128;

/// Broker-side transport custody only. An ACK is recorded after the original
/// receiver has verified and immutably retained the exact envelope. It says
/// nothing about later choreography or enrollment success.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(super) struct PendingMailboxAckWitness {
    pub manifest_digest: String,
    pub subject: String,
    pub device: String,
    pub message_id: String,
    pub envelope_digest: String,
    pub stage: PendingMailboxWitnessStage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum PendingMailboxWitnessStage {
    ReceiverAcknowledgedRetainedFrame,
}
#[derive(Debug)]
struct PendingEnvelope {
    delivery: PendingMailboxDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PendingMailboxKey {
    subject: AuthorityId,
    device: DeviceId,
    manifest_digest: [u8; 32],
}

impl PendingMailboxKey {
    pub(super) fn new(subject: AuthorityId, device: DeviceId, manifest_digest: [u8; 32]) -> Self {
        Self {
            subject,
            device,
            manifest_digest,
        }
    }
}

#[derive(Debug)]
struct PendingMailboxEntry {
    registration: PendingEnrollmentMailboxRegistration,
    credential_commitment: [u8; 32],
    retention_ends_at_ms: u64,
    envelopes: Vec<PendingEnvelope>,
    acknowledged: HashMap<String, [u8; 32]>,
}

/// Broker-local availability bound. Signed expiry is checked by each real
/// runtime's selected physical provider; monotonic age here only attenuates
/// storage retention and cannot establish distributed ordering.
#[derive(Debug)]
pub(super) struct PendingEnrollmentMailboxes {
    entries: HashMap<PendingMailboxKey, PendingMailboxEntry>,
    retention: Duration,
    #[cfg(test)]
    fail_next_ack: bool,
}

impl PendingEnrollmentMailboxes {
    pub(super) fn new(retention: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            retention,
            #[cfg(test)]
            fail_next_ack: false,
        }
    }

    #[cfg(test)]
    pub(super) fn fail_next_ack_for_test(&mut self) {
        self.fail_next_ack = true;
    }

    fn prune_expired(&mut self, now_ms: u64) {
        self.entries
            .retain(|_, entry| now_ms < entry.retention_ends_at_ms);
    }

    /// The broker retains bytes and validates the credential commitment. It
    /// makes no issuer-trust claim from the registration's embedded signature.
    pub(super) fn register(
        &mut self,
        registration: PendingEnrollmentMailboxRegistration,
        retrieval_credential: &str,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.prune_expired(now_ms);
        if retrieval_credential.len() < 32 || retrieval_credential.len() > 256 {
            return Err("pending mailbox retrieval credential length is invalid");
        }
        let statement = &registration.statement;
        if now_ms >= statement.expires_at_ms() {
            return Err("pending mailbox registration expired on selected physical provider");
        }
        let candidate = hash(retrieval_credential.as_bytes());
        if statement.version()
            != aura_invitation::enrollment_mailbox::PendingEnrollmentMailboxStatement::VERSION
            || !constant_time_eq(&statement.retrieval_credential_commitment(), &candidate)
        {
            return Err("pending mailbox registration credential binding is invalid");
        }
        let key = PendingMailboxKey::new(
            statement.subject(),
            statement.invitee_device(),
            statement.manifest_digest(),
        );
        if let Some(existing) = self.entries.get(&key) {
            if !constant_time_eq(&existing.credential_commitment, &candidate)
                || existing.registration != registration
            {
                return Err("pending mailbox registration conflicts with original scope");
            }
            return Ok(());
        }
        if self.entries.len() >= MAX_REGISTRATIONS {
            return Err("pending mailbox registration limit reached");
        }
        let retention_ends_at_ms = statement
            .expires_at_ms()
            .min(now_ms.saturating_add(self.retention.as_millis() as u64));
        self.entries.insert(
            key,
            PendingMailboxEntry {
                credential_commitment: statement.retrieval_credential_commitment(),
                registration,
                retention_ends_at_ms,
                envelopes: Vec::new(),
                acknowledged: HashMap::new(),
            },
        );
        Ok(())
    }

    pub(super) fn registration(
        &mut self,
        key: PendingMailboxKey,
        now_ms: u64,
    ) -> Option<PendingEnrollmentMailboxRegistration> {
        self.prune_expired(now_ms);
        self.entries
            .get(&key)
            .map(|entry| entry.registration.clone())
    }

    /// Accept only a frame with exact signed scope, session and device routing
    /// metadata. The recipient VM verifies the actual cryptographic content.
    pub(super) fn enqueue(
        &mut self,
        key: PendingMailboxKey,
        envelope: TransportEnvelope,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.prune_expired(now_ms);
        let pending_total: usize = self
            .entries
            .values()
            .map(|entry| entry.envelopes.len())
            .sum();
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or("pending mailbox registration is absent")?;
        let statement = &entry.registration.statement;
        let destination_device = envelope
            .metadata
            .get("aura-destination-device-id")
            .and_then(|raw| raw.parse::<DeviceId>().ok());
        let session = envelope
            .metadata
            .get("session-id")
            .and_then(|raw| Uuid::parse_str(raw).ok());
        if envelope.source != statement.subject()
            || envelope.destination != statement.subject()
            || destination_device != Some(statement.invitee_device())
            || envelope
                .metadata
                .get("content-type")
                .is_none_or(|value| value != "application/aura-choreography")
            || !session.is_some_and(|id| {
                is_original_enrollment_mailbox_session(
                    statement.invitation(),
                    &statement.manifest_digest(),
                    id,
                )
            })
            || envelope.receipt.as_ref().is_none_or(|receipt| {
                receipt.src != envelope.source
                    || receipt.dst != envelope.destination
                    || receipt.context != envelope.context
            })
        {
            return Err("pending mailbox envelope differs from original signed route");
        }
        let message_id = envelope
            .metadata
            .get("aura-message-id")
            .filter(|id| Uuid::parse_str(id).is_ok())
            .ok_or("pending mailbox envelope lacks guarded message identity")?
            .clone();
        let envelope_bytes = serde_json::to_vec(&envelope)
            .map_err(|_| "pending mailbox envelope encoding failed")?;
        let envelope_digest = hash(&envelope_bytes);
        if let Some(acknowledged) = entry.acknowledged.get(&message_id) {
            return if *acknowledged == envelope_digest {
                Ok(())
            } else {
                Err("pending mailbox acknowledged message identity changed")
            };
        }
        if let Some(pending) = entry
            .envelopes
            .iter()
            .find(|pending| pending.delivery.message_id == message_id)
        {
            let original = serde_json::to_vec(&pending.delivery.envelope)
                .map_err(|_| "pending mailbox original envelope encoding failed")?;
            return if original == envelope_bytes {
                Ok(())
            } else {
                Err("pending mailbox message identity changed on retry")
            };
        }
        if entry.acknowledged.len() >= MAX_ACKNOWLEDGED_PER_REGISTRATION
            || pending_total >= MAX_PENDING_ENVELOPES
            || entry.envelopes.len() >= MAX_ENVELOPES_PER_REGISTRATION
        {
            return Err("pending mailbox envelope limit reached");
        }
        entry.envelopes.push(PendingEnvelope {
            delivery: PendingMailboxDelivery {
                message_id,
                envelope,
            },
        });
        Ok(())
    }

    pub(super) fn take(
        &mut self,
        key: PendingMailboxKey,
        retrieval_credential: &str,
        now_ms: u64,
    ) -> Result<Vec<PendingMailboxDelivery>, &'static str> {
        self.prune_expired(now_ms);
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or("pending mailbox registration is absent")?;
        let candidate = hash(retrieval_credential.as_bytes());
        if !constant_time_eq(&candidate, &entry.credential_commitment) {
            return Err("pending mailbox retrieval credential rejected");
        }
        Ok(entry
            .envelopes
            .iter()
            .map(|pending| pending.delivery.clone())
            .collect())
    }

    pub(super) fn acknowledged_witnesses(
        &mut self,
        key: PendingMailboxKey,
        now_ms: u64,
    ) -> Vec<PendingMailboxAckWitness> {
        self.prune_expired(now_ms);
        let Some(entry) = self.entries.get(&key) else {
            return Vec::new();
        };
        let mut witnesses: Vec<_> = entry
            .acknowledged
            .iter()
            .map(|(message_id, digest)| ack_witness(key, message_id, digest))
            .collect();
        witnesses.sort_by(|a, b| a.message_id.cmp(&b.message_id));
        witnesses
    }

    /// Only the credential holder can retire an exact delivered message after
    /// its runtime has admitted the original envelope. An interrupted take
    /// leaves the bounded retained frame available for immediate redelivery.
    pub(super) fn acknowledge(
        &mut self,
        key: PendingMailboxKey,
        retrieval_credential: &str,
        message_id: &str,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.prune_expired(now_ms);
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or("pending mailbox registration is absent")?;
        let candidate = hash(retrieval_credential.as_bytes());
        if !constant_time_eq(&candidate, &entry.credential_commitment) {
            return Err("pending mailbox retrieval credential rejected");
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_ack) {
            return Err("injected pending mailbox ACK interruption");
        }
        if entry.acknowledged.contains_key(message_id) {
            return Ok(());
        }
        let index = entry
            .envelopes
            .iter()
            .position(|pending| pending.delivery.message_id == message_id)
            .ok_or("pending mailbox acknowledgment identity is absent")?;
        let acknowledged = entry.envelopes.remove(index);
        let bytes = serde_json::to_vec(&acknowledged.delivery.envelope)
            .map_err(|_| "pending mailbox acknowledged envelope encoding failed")?;
        entry
            .acknowledged
            .insert(message_id.to_owned(), hash(&bytes));
        Ok(())
    }
}

fn ack_witness(
    key: PendingMailboxKey,
    message_id: &str,
    envelope_digest: &[u8; 32],
) -> PendingMailboxAckWitness {
    PendingMailboxAckWitness {
        manifest_digest: hex::encode(key.manifest_digest),
        subject: key.subject.to_string(),
        device: key.device.to_string(),
        message_id: message_id.to_owned(),
        envelope_digest: hex::encode(envelope_digest),
        stage: PendingMailboxWitnessStage::ReceiverAcknowledgedRetainedFrame,
    }
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

#[cfg(test)]
mod witness_tests {
    use super::*;

    #[test]
    fn ack_witness_exposes_exact_scope_and_digest_without_envelope_or_credential() {
        let key = PendingMailboxKey::new(
            "11111111-1111-1111-1111-111111111111".parse().unwrap(),
            DeviceId::from_bytes([2; 32]),
            [3; 32],
        );
        let message_id = "22222222-2222-2222-2222-222222222222";
        let witness = ack_witness(key, message_id, &[4; 32]);
        let json = serde_json::to_value(&witness).expect("witness encodes");
        assert_eq!(json["manifest_digest"], hex::encode([3; 32]));
        assert_eq!(json["device"], key.device.to_string());
        assert_eq!(json["message_id"], message_id);
        assert_eq!(json["envelope_digest"], hex::encode([4; 32]));
        assert_eq!(json["stage"], "receiver_acknowledged_retained_frame");
        assert_eq!(json.as_object().unwrap().len(), 6);
    }
}
