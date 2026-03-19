#[derive(Clone)]
struct DemoCommand;

#[aura_macros::actor_owned(
    owner = "demo-actor",
    domain = "demo-domain",
    gate = "demo-ingress",
    command = DemoCommand,
    category = "actor_owned"
)]
struct DemoActor;

fn main() {}
