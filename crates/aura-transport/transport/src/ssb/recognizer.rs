//! SSB Envelope Recognition
//!
//! Implements envelope recognition as specified in docs/041_rendezvous.md Section 4.2
//! "CRDT-based Publishing & Recognition".
//!
//! Recognition Flow:
//! 1. Check if envelope is in recognition window (epoch±Δ, counter±k)
//! 2. Compare routing tag against all active relationships
//! 3. Attempt decryption with matching relationship keys
//! 4. Verify authentication and check for replay
//! 5. Return recognized payload or None

use crate::error::TransportResult;
use crate::infrastructure::envelope::{Envelope, RoutingTag};
use crate::ssb::publisher::{EnvelopePayload, SbbPublisher};
use std::collections::{BTreeMap, BTreeSet};

/// Recognition window parameters
#[derive(Debug, Clone)]
pub struct RecognitionWindow {
    /// Epoch delta (epochs before/after current)
    pub epoch_delta: u64,

    /// Counter window size
    pub counter_window: u64,
}

impl RecognitionWindow {
    /// Standard recognition window
    pub fn standard() -> Self {
        RecognitionWindow {
            epoch_delta: 2,      // ±2 epochs for clock skew
            counter_window: 100, // Last 100 counters per relationship
        }
    }

    /// Check if envelope is within recognition window
    pub fn contains(&self, current_epoch: u64, envelope_epoch: u64) -> bool {
        let epoch_diff = if current_epoch >= envelope_epoch {
            current_epoch - envelope_epoch
        } else {
            envelope_epoch - current_epoch
        };

        epoch_diff <= self.epoch_delta
    }
}

/// Active relationship for recognition
#[derive(Debug, Clone)]
pub struct ActiveRelationship {
    /// Relationship ID
    pub relationship_id: Vec<u8>,

    /// K_box for decryption
    pub k_box: [u8; 32],

    /// K_tag for routing tag verification
    pub k_tag: [u8; 32],

    /// Last seen counter (for replay detection)
    pub last_seen_counter: u64,
}

/// Recognized envelope with metadata
#[derive(Debug, Clone)]
pub struct RecognizedEnvelope {
    /// The decrypted payload
    pub payload: EnvelopePayload,

    /// Relationship this was recognized from
    pub relationship_id: Vec<u8>,

    /// Envelope epoch
    pub epoch: u64,

    /// Envelope counter
    pub counter: u64,

    /// Recognition timestamp
    pub recognized_at: u64,
}

/// SSB Envelope Recognizer
pub struct SbbRecognizer {
    /// Current epoch
    current_epoch: u64,

    /// Recognition window parameters
    window: RecognitionWindow,

    /// Active relationships for recognition
    relationships: BTreeMap<Vec<u8>, ActiveRelationship>,

    /// Routing tag index for fast lookup
    /// Maps (epoch, routing_tag) -> set of relationship_ids
    rtag_index: BTreeMap<(u64, RoutingTag), BTreeSet<Vec<u8>>>,
}

impl SbbRecognizer {
    /// Create a new recognizer
    pub fn new(current_epoch: u64) -> Self {
        SbbRecognizer {
            current_epoch,
            window: RecognitionWindow::standard(),
            relationships: BTreeMap::new(),
            rtag_index: BTreeMap::new(),
        }
    }

    /// Update current epoch
    pub fn set_epoch(&mut self, epoch: u64) {
        self.current_epoch = epoch;
        self.rebuild_rtag_index();
    }

    /// Add a relationship for recognition
    pub fn add_relationship(&mut self, relationship: ActiveRelationship) {
        let relationship_id = relationship.relationship_id.clone();
        self.relationships.insert(relationship_id, relationship);
        self.rebuild_rtag_index();
    }

    /// Remove a relationship
    pub fn remove_relationship(&mut self, relationship_id: &[u8]) {
        self.relationships.remove(relationship_id);
        self.rebuild_rtag_index();
    }

    /// Rebuild routing tag index
    ///
    /// Pre-computes routing tags for all relationships in the recognition window
    /// to enable O(1) lookup during recognition.
    fn rebuild_rtag_index(&mut self) {
        self.rtag_index.clear();

        // For each relationship
        for (rel_id, rel) in &self.relationships {
            // For each epoch in recognition window
            let start_epoch = self.current_epoch.saturating_sub(self.window.epoch_delta);
            let end_epoch = self.current_epoch + self.window.epoch_delta;

            for epoch in start_epoch..=end_epoch {
                // For each counter in window
                let start_counter = rel
                    .last_seen_counter
                    .saturating_sub(self.window.counter_window);
                let end_counter = rel.last_seen_counter + self.window.counter_window;

                for counter in start_counter..=end_counter {
                    // Compute routing tag
                    if let Ok(rtag) = SbbPublisher::compute_routing_tag(&rel.k_tag, epoch, counter)
                    {
                        self.rtag_index
                            .entry((epoch, rtag))
                            .or_insert_with(BTreeSet::new)
                            .insert(rel_id.clone());
                    }
                }
            }
        }
    }

    /// Attempt to recognize an envelope
    ///
    /// Returns Some(RecognizedEnvelope) if recognition succeeds, None otherwise.
    /// Recognition is O(relationships) not O(envelopes).
    pub fn recognize_envelope(
        &mut self,
        envelope: &Envelope,
        recognized_at: u64,
    ) -> TransportResult<Option<RecognizedEnvelope>> {
        let epoch = envelope.header.bare.epoch;
        let counter = envelope.header.bare.counter;
        let rtag = &envelope.header.bare.rtag;

        // 1. Check if within recognition window
        if !self.window.contains(self.current_epoch, epoch) {
            return Ok(None); // Outside window, not for us
        }

        // 2. Look up potential relationships via routing tag
        let candidate_relationships = self
            .rtag_index
            .get(&(epoch, rtag.clone()))
            .cloned()
            .unwrap_or_default();

        if candidate_relationships.is_empty() {
            return Ok(None); // No matching routing tags
        }

        // 3. Attempt decryption with each candidate
        for rel_id in candidate_relationships {
            if let Some(rel) = self.relationships.get(&rel_id) {
                // Check for replay
                if counter <= rel.last_seen_counter {
                    continue; // Replay, skip this relationship
                }

                // Attempt decryption
                let nonce = Self::compute_nonce(epoch, counter);
                if let Ok(payload) =
                    SbbPublisher::decrypt_payload(&rel.k_box, &envelope.ciphertext, &nonce)
                {
                    // Success! Update last_seen_counter
                    if let Some(rel_mut) = self.relationships.get_mut(&rel_id) {
                        rel_mut.last_seen_counter = counter;
                    }

                    return Ok(Some(RecognizedEnvelope {
                        payload,
                        relationship_id: rel_id,
                        epoch,
                        counter,
                        recognized_at,
                    }));
                }
            }
        }

        // 4. No successful recognition
        Ok(None)
    }

    /// Compute nonce from epoch and counter (same as publisher)
    fn compute_nonce(epoch: u64, counter: u64) -> [u8; 24] {
        let mut nonce = [0u8; 24];
        nonce[..8].copy_from_slice(&epoch.to_le_bytes());
        nonce[8..16].copy_from_slice(&counter.to_le_bytes());
        nonce
    }

    /// Get number of active relationships
    pub fn relationship_count(&self) -> usize {
        self.relationships.len()
    }

    /// Get recognition window
    pub fn window(&self) -> &RecognitionWindow {
        &self.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssb::publisher::SbbPublisher;

    fn create_test_relationship(id: u8) -> ActiveRelationship {
        ActiveRelationship {
            relationship_id: vec![id],
            k_box: [id; 32],
            k_tag: [id + 1; 32],
            last_seen_counter: 0,
        }
    }

    #[test]
    fn test_recognition_window() {
        let window = RecognitionWindow::standard();

        let current_epoch = 100;

        // Within window
        assert!(window.contains(current_epoch, 100)); // Same epoch
        assert!(window.contains(current_epoch, 98)); // -2
        assert!(window.contains(current_epoch, 102)); // +2

        // Outside window
        assert!(!window.contains(current_epoch, 97)); // -3
        assert!(!window.contains(current_epoch, 103)); // +3
    }

    #[test]
    fn test_add_remove_relationship() {
        let mut recognizer = SbbRecognizer::new(100);

        assert_eq!(recognizer.relationship_count(), 0);

        let rel = create_test_relationship(1);
        recognizer.add_relationship(rel.clone());

        assert_eq!(recognizer.relationship_count(), 1);

        recognizer.remove_relationship(&rel.relationship_id);

        assert_eq!(recognizer.relationship_count(), 0);
    }

    #[test]
    fn test_recognize_published_envelope() {
        let epoch = 100;
        let publisher = SbbPublisher::new(epoch);
        let mut recognizer = SbbRecognizer::new(epoch);

        // Set up relationship
        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];
        let relationship_id = vec![1, 2, 3];

        let rel = ActiveRelationship {
            relationship_id: relationship_id.clone(),
            k_box,
            k_tag,
            last_seen_counter: 0,
        };

        recognizer.add_relationship(rel);

        // Publish envelope
        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Hello, SSB!".to_vec(),
            metadata: vec![],
        };

        let published = publisher
            .publish_envelope(
                payload.clone(),
                &k_box,
                &k_tag,
                42,
                10,
                relationship_id.clone(),
                1000,
            )
            .unwrap();

        // Recognize envelope
        let recognized = recognizer
            .recognize_envelope(&published.envelope, 2000)
            .unwrap();

        assert!(recognized.is_some());
        let recognized = recognized.unwrap();
        assert_eq!(recognized.payload.content_type, payload.content_type);
        assert_eq!(recognized.payload.data, payload.data);
        assert_eq!(recognized.relationship_id, relationship_id);
        assert_eq!(recognized.counter, 42);
    }

    #[test]
    fn test_replay_detection() {
        let epoch = 100;
        let publisher = SbbPublisher::new(epoch);
        let mut recognizer = SbbRecognizer::new(epoch);

        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];
        let relationship_id = vec![1, 2, 3];

        let rel = ActiveRelationship {
            relationship_id: relationship_id.clone(),
            k_box,
            k_tag,
            last_seen_counter: 50, // Already seen up to counter 50
        };

        recognizer.add_relationship(rel);

        // Try to recognize old envelope (counter 42 < 50)
        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Old message".to_vec(),
            metadata: vec![],
        };

        let published = publisher
            .publish_envelope(
                payload,
                &k_box,
                &k_tag,
                42, // Counter below last_seen
                10,
                relationship_id,
                1000,
            )
            .unwrap();

        let recognized = recognizer
            .recognize_envelope(&published.envelope, 2000)
            .unwrap();

        // Should not recognize (replay)
        assert!(recognized.is_none());
    }

    #[test]
    fn test_wrong_relationship() {
        let epoch = 100;
        let publisher = SbbPublisher::new(epoch);
        let mut recognizer = SbbRecognizer::new(epoch);

        // Set up relationship with different keys
        let k_box_pub = [0x42; 32];
        let k_tag_pub = [0x43; 32];

        let k_box_rec = [0x44; 32]; // Different key
        let k_tag_rec = [0x45; 32]; // Different key

        let rel = ActiveRelationship {
            relationship_id: vec![1, 2, 3],
            k_box: k_box_rec,
            k_tag: k_tag_rec,
            last_seen_counter: 0,
        };

        recognizer.add_relationship(rel);

        // Publish with different keys
        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Wrong keys".to_vec(),
            metadata: vec![],
        };

        let published = publisher
            .publish_envelope(payload, &k_box_pub, &k_tag_pub, 42, 10, vec![1, 2, 3], 1000)
            .unwrap();

        let recognized = recognizer
            .recognize_envelope(&published.envelope, 2000)
            .unwrap();

        // Should not recognize (wrong keys)
        assert!(recognized.is_none());
    }

    #[test]
    fn test_outside_recognition_window() {
        let current_epoch = 100;
        let envelope_epoch = 95; // -5 epochs, outside window (delta=2)

        let publisher = SbbPublisher::new(envelope_epoch);
        let mut recognizer = SbbRecognizer::new(current_epoch);

        let k_box = [0x42; 32];
        let k_tag = [0x43; 32];

        let rel = ActiveRelationship {
            relationship_id: vec![1, 2, 3],
            k_box,
            k_tag,
            last_seen_counter: 0,
        };

        recognizer.add_relationship(rel);

        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"Old epoch".to_vec(),
            metadata: vec![],
        };

        let published = publisher
            .publish_envelope(payload, &k_box, &k_tag, 42, 10, vec![1, 2, 3], 1000)
            .unwrap();

        let recognized = recognizer
            .recognize_envelope(&published.envelope, 2000)
            .unwrap();

        // Should not recognize (outside window)
        assert!(recognized.is_none());
    }

    #[test]
    fn test_multiple_relationships() {
        let epoch = 100;
        let publisher = SbbPublisher::new(epoch);
        let mut recognizer = SbbRecognizer::new(epoch);

        // Add multiple relationships
        for i in 1..=5 {
            let rel = create_test_relationship(i);
            recognizer.add_relationship(rel);
        }

        assert_eq!(recognizer.relationship_count(), 5);

        // Publish to relationship 3
        let k_box = [3; 32];
        let k_tag = [4; 32];

        let payload = EnvelopePayload {
            content_type: "text/plain".to_string(),
            data: b"For relationship 3".to_vec(),
            metadata: vec![],
        };

        let published = publisher
            .publish_envelope(payload.clone(), &k_box, &k_tag, 42, 10, vec![3], 1000)
            .unwrap();

        let recognized = recognizer
            .recognize_envelope(&published.envelope, 2000)
            .unwrap();

        assert!(recognized.is_some());
        let recognized = recognized.unwrap();
        assert_eq!(recognized.relationship_id, vec![3]);
        assert_eq!(recognized.payload.data, payload.data);
    }
}
