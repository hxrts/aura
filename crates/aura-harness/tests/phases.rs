//! Phased harness evolution tests (phase 1 through phase 5).
//!
//! Each phase tests a progressive layer of harness capability: tool API,
//! routing replay, state machine, reliability, and API negotiation.

mod phases {
    mod phase1_regression;
    mod phase1_tool_api;
    mod phase2_regression;
    mod phase2_routing_replay;
    mod phase3_state_machine;
    mod phase4_reliability;
    mod phase5_api_negotiation;
    mod phase5_regression;
}
