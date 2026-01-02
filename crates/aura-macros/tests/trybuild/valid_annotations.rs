use aura_macros::choreography;

choreography! {
    #[namespace = "valid_annotations"]
    choreography ValidAnnotations {
        roles: Alice, Bob;

        Alice[guard_capability = "send_message", flow_cost = 10, journal_facts = "message_sent"] -> Bob: Message;
        Alice[leak: (External, Neighbor)] -> Bob: LeakMessage;
        Alice[journal_merge = true] -> Bob: Merge;
        Alice[audit_log = "message_sent"] -> Bob: Audit;
    }
}

fn main() {}
