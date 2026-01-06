use aura_macros::choreography;

choreography!(r#"
module invalid_guard_capability exposing (InvalidGuardCapability)

protocol InvalidGuardCapability =
  roles Alice, Bob
  Alice[guard_capability = 42] -> Bob : Message
"#);

fn main() {}
