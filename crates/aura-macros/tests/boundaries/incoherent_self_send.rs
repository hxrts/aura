use aura_macros::tell;

tell!(r#"
module incoherent_self_send exposing (IncoherentSelfSend)

protocol IncoherentSelfSend =
  roles Alice
  Alice -> Alice : Loopback
"#);

fn main() {}
