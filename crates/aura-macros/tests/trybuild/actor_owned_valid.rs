#[derive(Clone)]
struct DemoCommand;

#[aura_macros::actor_owned(
    owner = "demo-actor",
    domain = "demo-domain",
    gate = "demo-ingress",
    command = DemoCommand,
    capacity = 64,
    category = "actor_owned"
)]
struct DemoActor;

fn main() {
    let ingress = DemoActor::actor_ingress();
    assert_eq!(ingress.owner_name(), "demo-actor");
    assert_eq!(ingress.capacity(), 64);
    let _ = DemoActor::ACTOR_OWNER_NAME;
    let declaration = DemoActor::actor_declaration();
    assert_eq!(declaration.domain_name(), "demo-domain");
    assert_eq!(declaration.ingress_gate(), "demo-ingress");
}
