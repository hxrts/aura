//! Bounded broker holding for the pre-adoption Bob-to-Alice enrollment route.
//! These records are untrusted availability data. Alice must verify the
//! original issuer proof and each VM frame against retained enrollment custody.

use super::{BootstrapIssuerReturnMailboxEnvelope, IssuerReturnMailboxDelivery};
use aura_core::hash::hash;
use aura_core::{AuthorityId, DeviceId};
use aura_invitation::enrollment_mailbox::IssuerEnrollmentReturnRegistration;
use std::collections::HashMap;
use std::time::Duration;

const MAX_REGISTRATIONS: usize = 256;
const MAX_PENDING_ENVELOPES: usize = 32;
const MAX_ENVELOPES_PER_REGISTRATION: usize = 4;
const MAX_ACKNOWLEDGED_PER_REGISTRATION: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ReturnMailboxKey {
    subject: AuthorityId,
    issuer_device: DeviceId,
    manifest_digest: [u8; 32],
}

impl ReturnMailboxKey {
    pub(super) fn new(
        subject: AuthorityId,
        issuer_device: DeviceId,
        manifest_digest: [u8; 32],
    ) -> Self {
        Self {
            subject,
            issuer_device,
            manifest_digest,
        }
    }
}

#[derive(Debug)]
struct ReturnMailboxEntry {
    registration: IssuerEnrollmentReturnRegistration,
    credential_commitment: [u8; 32],
    retention_ends_at_ms: u64,
    envelopes: Vec<IssuerReturnMailboxDelivery>,
    acknowledged: HashMap<String, [u8; 32]>,
}

#[derive(Debug)]
pub(super) struct EnrollmentReturnMailboxes {
    entries: HashMap<ReturnMailboxKey, ReturnMailboxEntry>,
    retention: Duration,
    #[cfg(test)]
    fail_next_ack: bool,
}

impl EnrollmentReturnMailboxes {
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

    pub(super) fn register(
        &mut self,
        registration: IssuerEnrollmentReturnRegistration,
        retrieval_credential: &str,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.prune_expired(now_ms);
        if !(32..=256).contains(&retrieval_credential.len()) {
            return Err("return mailbox credential or selected validity is invalid");
        }
        let candidate = hash(retrieval_credential.as_bytes());
        if now_ms >= registration.statement.expires_at_ms()
            || !constant_time_eq(
                &registration.statement.retrieval_credential_commitment(),
                &candidate,
            )
        {
            return Err("return mailbox credential or selected validity is invalid");
        }
        let statement = &registration.statement;
        let key = ReturnMailboxKey::new(
            statement.subject(),
            statement.issuer_device(),
            statement.manifest_digest(),
        );
        if let Some(existing) = self.entries.get(&key) {
            return if constant_time_eq(&existing.credential_commitment, &candidate)
                && existing.registration == registration
            {
                Ok(())
            } else {
                Err("return mailbox registration conflicts with original scope")
            };
        }
        if self.entries.len() >= MAX_REGISTRATIONS {
            return Err("return mailbox registration capacity reached");
        }
        let retention_ends_at_ms = statement
            .expires_at_ms()
            .min(now_ms.saturating_add(self.retention.as_millis() as u64));
        self.entries.insert(
            key,
            ReturnMailboxEntry {
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
        key: ReturnMailboxKey,
        now_ms: u64,
    ) -> Option<IssuerEnrollmentReturnRegistration> {
        self.prune_expired(now_ms);
        self.entries
            .get(&key)
            .map(|entry| entry.registration.clone())
    }

    /// Call only after the setup-key wire verifier has checked this exact
    /// registration and envelope. No broker receipt transfers VM custody.
    pub(super) fn enqueue_verified(
        &mut self,
        key: ReturnMailboxKey,
        registration: &IssuerEnrollmentReturnRegistration,
        payload: BootstrapIssuerReturnMailboxEnvelope,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        let envelope = payload.envelope;
        self.prune_expired(now_ms);
        let pending_total: usize = self
            .entries
            .values()
            .map(|entry| entry.envelopes.len())
            .sum();
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or("return mailbox registration is absent")?;
        if &entry.registration != registration
            || now_ms >= entry.retention_ends_at_ms
            || envelope.source != registration.statement.provisional()
            || envelope.destination != key.subject
            || envelope.metadata.get("aura-source-device-id")
                != Some(&registration.statement.invitee_device().to_string())
            || envelope.metadata.get("aura-destination-device-id")
                != Some(&key.issuer_device.to_string())
        {
            return Err("return mailbox envelope differs from registered scope");
        }
        let message_id = envelope
            .metadata
            .get("aura-message-id")
            .ok_or("return mailbox envelope has no message identity")?
            .clone();
        let candidate = IssuerReturnMailboxDelivery {
            message_id: message_id.clone(),
            setup_request: payload.setup_request,
            authorization: payload.authorization,
            envelope,
        };
        let bytes = aura_core::util::serialization::to_vec(&candidate)
            .map_err(|_| "return mailbox envelope encoding failed")?;
        let digest = hash(&bytes);
        if let Some(previous) = entry.acknowledged.get(&message_id) {
            return if *previous == digest {
                Ok(())
            } else {
                Err("return mailbox acknowledged identity changed")
            };
        }
        if let Some(previous) = entry
            .envelopes
            .iter()
            .find(|delivery| delivery.message_id == message_id)
        {
            let previous_bytes = aura_core::util::serialization::to_vec(previous)
                .map_err(|_| "return mailbox retained envelope encoding failed")?;
            return if previous_bytes == bytes {
                Ok(())
            } else {
                Err("return mailbox message identity changed on retry")
            };
        }
        if entry.acknowledged.len() >= MAX_ACKNOWLEDGED_PER_REGISTRATION
            || pending_total >= MAX_PENDING_ENVELOPES
            || entry.envelopes.len() >= MAX_ENVELOPES_PER_REGISTRATION
        {
            return Err("return mailbox envelope capacity reached");
        }
        entry.envelopes.push(candidate);
        Ok(())
    }

    pub(super) fn take(
        &mut self,
        key: ReturnMailboxKey,
        retrieval_credential: &str,
        now_ms: u64,
    ) -> Result<Vec<IssuerReturnMailboxDelivery>, &'static str> {
        self.prune_expired(now_ms);
        let entry = self
            .entries
            .get(&key)
            .ok_or("return mailbox registration is absent")?;
        if !constant_time_eq(
            &hash(retrieval_credential.as_bytes()),
            &entry.credential_commitment,
        ) {
            return Err("return mailbox retrieval credential rejected");
        }
        Ok(entry.envelopes.clone())
    }

    pub(super) fn acknowledge(
        &mut self,
        key: ReturnMailboxKey,
        retrieval_credential: &str,
        message_id: &str,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.prune_expired(now_ms);
        let entry = self
            .entries
            .get_mut(&key)
            .ok_or("return mailbox registration is absent")?;
        if !constant_time_eq(
            &hash(retrieval_credential.as_bytes()),
            &entry.credential_commitment,
        ) {
            return Err("return mailbox retrieval credential rejected");
        }
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_ack) {
            return Err("injected return mailbox ACK interruption");
        }
        if entry.acknowledged.contains_key(message_id) {
            return Ok(());
        }
        let index = entry
            .envelopes
            .iter()
            .position(|delivery| delivery.message_id == message_id)
            .ok_or("return mailbox acknowledgment identity is absent")?;
        let delivery = entry.envelopes.remove(index);
        let bytes = aura_core::util::serialization::to_vec(&delivery)
            .map_err(|_| "return mailbox acknowledged envelope encoding failed")?;
        entry
            .acknowledged
            .insert(message_id.to_owned(), hash(&bytes));
        Ok(())
    }
}

fn constant_time_eq(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
