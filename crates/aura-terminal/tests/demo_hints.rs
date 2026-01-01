#![allow(missing_docs)]
#![allow(clippy::expect_used)]

#[cfg(feature = "development")]
mod development {
    use aura_terminal::demo::hints::DemoHints;

    #[test]
    fn test_invite_code_parseable_by_shareable_invitation() {
        use aura_agent::handlers::ShareableInvitation;

        let hints = DemoHints::new(2024);

        // Verify Alice's code can be parsed
        let alice_parsed = ShareableInvitation::from_code(&hints.alice_invite_code)
            .expect("Alice's invitation code should be parseable");
        assert_eq!(alice_parsed.version, 1);
        assert!(!alice_parsed.invitation_id.is_empty());
        // Verify the invitation type is Contact (not Guardian)
        match alice_parsed.invitation_type {
            aura_invitation::InvitationType::Contact { nickname } => {
                assert_eq!(nickname, Some("Alice".to_string()));
            }
            _ => panic!(
                "Expected Contact invitation type, got {:?}",
                alice_parsed.invitation_type
            ),
        }

        // Verify Carol's code can be parsed
        let carol_parsed = ShareableInvitation::from_code(&hints.carol_invite_code)
            .expect("Carol's invitation code should be parseable");
        assert_eq!(carol_parsed.version, 1);
        assert!(!carol_parsed.invitation_id.is_empty());
        match carol_parsed.invitation_type {
            aura_invitation::InvitationType::Contact { nickname } => {
                assert_eq!(nickname, Some("Carol".to_string()));
            }
            _ => panic!(
                "Expected Contact invitation type, got {:?}",
                carol_parsed.invitation_type
            ),
        }

        // Verify different seeds produce different codes
        assert_ne!(
            alice_parsed.sender_id, carol_parsed.sender_id,
            "Alice and Carol should have different sender IDs"
        );
    }
}
