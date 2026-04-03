use aura_macros::tell;

tell!(r#"
protocol MissingNamespace =
  roles Alice, Bob
  Alice -> Bob : Message
"#);

fn main() {}
