//! Garbage collection helpers for maintenance workflows.

use aura_core::epochs::Epoch;

/// GC plan for DKG transcript blobs tied to snapshot epochs.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptGcPlan {
    /// Earliest epoch to retain; anything before this may be deleted.
    pub retain_from_epoch: Epoch,
}

impl TranscriptGcPlan {
    /// Returns true if the transcript epoch is eligible for deletion.
    pub fn should_delete(self, transcript_epoch: Epoch) -> bool {
        transcript_epoch < self.retain_from_epoch
    }
}

/// Build a GC plan using the latest snapshot epoch and a retention window.
pub fn plan_dkg_transcript_gc(snapshot_epoch: Epoch, retain_epochs: u64) -> TranscriptGcPlan {
    let retain_from_epoch = Epoch::new(snapshot_epoch.value().saturating_sub(retain_epochs));
    TranscriptGcPlan { retain_from_epoch }
}
