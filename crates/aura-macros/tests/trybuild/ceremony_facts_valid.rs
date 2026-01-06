use aura_macros::ceremony_facts;
use aura_core::identifiers::CeremonyId;
use aura_core::threshold::AgreementMode;

#[ceremony_facts]
pub enum DemoFact {
    Other,
    CeremonyInitiated {
        ceremony_id: CeremonyId,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyAcceptanceReceived {
        ceremony_id: CeremonyId,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyCommitted {
        ceremony_id: CeremonyId,
        relationship_id: String,
        agreement_mode: Option<AgreementMode>,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonyAborted {
        ceremony_id: CeremonyId,
        reason: String,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
    CeremonySuperseded {
        superseded_ceremony_id: CeremonyId,
        superseding_ceremony_id: CeremonyId,
        reason: String,
        trace_id: Option<String>,
        timestamp_ms: u64,
    },
}

fn main() {}
