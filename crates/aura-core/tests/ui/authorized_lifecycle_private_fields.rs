use aura_core::ownership::{
    AuthorizedLifecyclePublication, LifecyclePublicationCapability, OperationLifecycle,
};

fn main() {
    let capability = LifecyclePublicationCapability::new("semantic:lifecycle");
    let lifecycle = OperationLifecycle::<&'static str, (), &'static str>::submitted();

    let _publication = AuthorizedLifecyclePublication {
        capability,
        lifecycle,
    };
}
