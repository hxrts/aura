use aura_macros::choreography;

choreography!(r#"
module legacy_guard_capability_name exposing (LegacyGuardCapabilityName)

protocol LegacyGuardCapabilityName =
  roles Alice, Bob
  Alice[guard_capability = "send_message"] -> Bob : Message
"#);

fn main() {}
