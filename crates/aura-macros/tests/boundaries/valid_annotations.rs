use aura_macros::choreography;

choreography!(r#"
module valid_annotations exposing (ValidAnnotations)

protocol ValidAnnotations =
  roles Alice, Bob
  Alice[guard_capability = "chat:message:send", flow_cost = 10, journal_facts = "message_sent"] -> Bob : GuardedMsg
  Alice[leak = "External,Neighbor"] -> Bob : LeakMsg
  Alice[journal_merge = true] -> Bob : MergeMsg
  Alice[audit_log = "message_sent"] -> Bob : AuditMsg
"#);

fn main() {}
