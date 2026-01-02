use aura_macros::choreography;

choreography! {
    #[namespace = "invalid_flow_cost"]
    choreography InvalidFlowCost {
        roles: Alice, Bob;

        Alice[flow_cost = "not_a_number"] -> Bob: Message;
    }
}

fn main() {}
