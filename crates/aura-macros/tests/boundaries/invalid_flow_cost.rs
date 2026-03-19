use aura_macros::choreography;

choreography!(r#"
module invalid_flow_cost exposing (InvalidFlowCost)

protocol InvalidFlowCost =
  roles Alice, Bob
  Alice[flow_cost = "not_a_number"] -> Bob : Message
"#);

fn main() {}
