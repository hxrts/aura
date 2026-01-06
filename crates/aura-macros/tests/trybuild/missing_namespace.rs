use aura_macros::choreography;

choreography!(r#"
protocol MissingNamespace =
  roles Alice, Bob
  Alice -> Bob : Message
"#);

fn main() {}
