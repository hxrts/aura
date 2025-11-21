//! Choreography annotations for AMP transport messages.
//!
//! Provides MPST-style metadata so guard capabilities/flow costs/journal facts
//! are enforced per message direction, aligning with docs/803_coordination_guide.md.

use aura_macros::choreography;

// Simple two-party choreography for AMP data + receipt exchange.
choreography! {
    #[namespace = "amp_transport"]
    protocol AmpTransport {
        roles: Sender, Receiver;

        // AMP ciphertext path; charge moderate flow and require send capability.
        Sender[guard_capability = "cap:amp_send", flow_cost = 25, journal_facts = "amp_send_msg"]
            -> Receiver: AmpData(AmpMessage);

        // Optional receipt/ack path; lightweight flow.
        Receiver[guard_capability = "cap:amp_recv", flow_cost = 5, journal_facts = "amp_recv_ack"]
            -> Sender: AmpAck(AmpReceipt);
    }
}
