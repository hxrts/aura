use aura_macros::choreography;

choreography!(
    #[namespace = "macro_namespace"]
    r#"
module source_namespace exposing (NamespaceMismatch)

protocol NamespaceMismatch =
  roles Alice, Bob
  Alice[guard_capability = "chat:message:send"] -> Bob : Message
"#
);

fn main() {}
