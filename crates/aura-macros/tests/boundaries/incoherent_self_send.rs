use aura_macros::choreography;

choreography!(r#"
module incoherent_self_send exposing (IncoherentSelfSend)

protocol IncoherentSelfSend =
  roles Alice
  Alice -> Alice : Loopback
"#);

fn main() {}
