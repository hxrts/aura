use aura_agent::runtime::AuthorityManager;
use aura_core::AuthorityId;
use aura_testkit::stateful_effects::random::MockRandomHandler;

#[tokio::test]
async fn creates_and_lists_authorities() {
    let mut manager = AuthorityManager::new("/tmp/aura-authority-tests".into());
    let random = MockRandomHandler::new_with_seed(42);

    let authority_id = manager
        .create_authority(&random, Vec::new(), 1)
        .await
        .expect("create authority");

    let authorities = manager.list_authorities();
    assert!(
        authorities.contains(&authority_id),
        "new authority should appear in listing"
    );
}

#[tokio::test]
async fn creates_relational_context() {
    let mut manager = AuthorityManager::new("/tmp/aura-context-tests".into());
    let a = AuthorityId::new();
    let b = AuthorityId::new();

    let context_id = manager
        .create_context(vec![a, b], "guardian".to_string())
        .await
        .expect("create context");

    let context = manager
        .get_context(&context_id)
        .expect("context should exist");
    assert!(context.participants.contains(&a));
}
