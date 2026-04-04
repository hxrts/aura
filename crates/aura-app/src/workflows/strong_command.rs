//! Strongly typed command boundary for slash command execution.
//!
//! This module introduces a strict parse/resolve boundary:
//! - `ParsedCommand` carries syntax-level values.
//! - `ResolvedCommand` carries canonical identifiers for executable targets.
//! - `CommandResolver` resolves against a single snapshot token.

#![allow(missing_docs)] // This API is being introduced incrementally.

mod execute;
mod parse;
mod resolve;

#[cfg(test)]
use execute::consistency_for_resolved;
pub use execute::{
    classify_terminal_execution_error, execute_planned, CommandCompletionOutcome,
    CommandConsistencySpec, CommandExecutionResult, CommandTerminalClassification,
    CommandTerminalOutcomeStatus, CommandTerminalReasonCode, ConsistencyDegradedReason,
    ConsistencyRequirement, ConsistencyWitness, PlannedCommand, COMMAND_CONSISTENCY_TABLE,
};
pub use parse::ParsedCommand;
pub use resolve::{
    ChannelResolveOutcome, CommandPlan, CommandPlanError, CommandResolver, CommandResolverError,
    CommandScope, ExistingChannelResolution, MembershipPlan, ModerationPlan, ModeratorPlan,
    PlanPrecondition, ResolveTarget, ResolvedAuthorityId, ResolvedChannelId, ResolvedCommand,
    ResolvedContextId, ResolverSnapshot, SnapshotToken,
};

/// Declare a command executor that can only accept `ResolvedCommand`.
///
/// This is a compile-time signature guard to prevent new executor APIs from
/// accepting untyped command payloads.
///
/// ```rust,compile_fail
/// use aura_app::workflows::strong_command::strong_command_executor;
///
/// strong_command_executor!(
///     fn bad_executor(_app: (), _cmd: String) -> () {}
/// );
/// ```
#[macro_export]
#[allow(unused_macros)]
macro_rules! strong_command_executor {
    (
        $(#[$meta:meta])*
        $vis:vis fn $name:ident(
            $app:ident : $app_ty:ty,
            $cmd:ident : $cmd_ty:ty $(,)?
        ) -> $ret:ty $body:block
    ) => {
        const _: fn() = || {
            let _signature_guard: fn($cmd_ty) =
                |_resolved: $crate::ui::workflows::strong_command::ResolvedCommand| {};
        };

        $(#[$meta])*
        $vis fn $name($app: $app_ty, $cmd: $cmd_ty) -> $ret $body
    };
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    #[cfg(feature = "signals")]
    use crate::core::StateSnapshot;
    #[cfg(feature = "signals")]
    use crate::ui::workflows::strong_command::execute::{home_for_scope, wait_for_consistency};
    use crate::views::{Channel, ChannelType, ChatState, Contact, ContactsState};
    #[cfg(feature = "signals")]
    use crate::AppConfig;
    #[cfg(feature = "signals")]
    use crate::AppCore;
    #[cfg(feature = "signals")]
    use crate::{
        signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        ui_contract::{
            AuthoritativeSemanticFact, OperationId, SemanticOperationKind, SemanticOperationPhase,
        },
        workflows::signals::read_signal_or_default,
    };
    #[cfg(feature = "signals")]
    use async_lock::RwLock;
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_core::AuraError;
    use proptest::prelude::*;
    #[cfg(feature = "signals")]
    use std::sync::Arc;

    #[tokio::test]
    async fn resolver_is_deterministic_for_repeated_resolution() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([1u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: Some("Bobby".to_string()),
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob.clone()]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let parsed = ParsedCommand::Kick {
            target: "bob".to_string(),
            reason: None,
        };

        let a = resolver.resolve(parsed.clone(), &snapshot).unwrap();
        let b = resolver.resolve(parsed, &snapshot).unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn resolver_reports_ambiguous_authority_matches() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([2u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };
        let bobby = Contact {
            id: AuthorityId::new_from_entropy([3u8; 32]),
            nickname: "bobby".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![
                    bob.clone(),
                    bobby.clone(),
                ]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let err = resolver
            .resolve(
                ParsedCommand::Mute {
                    target: "bo".to_string(),
                    duration: None,
                },
                &snapshot,
            )
            .expect_err("expected ambiguity");

        match err {
            CommandResolverError::AmbiguousTarget {
                target,
                input,
                candidates,
            } => {
                assert_eq!(target, ResolveTarget::Authority);
                assert_eq!(input, "bo");
                assert_eq!(candidates.len(), 2);
                assert!(candidates.iter().any(|c| c == &bob.id.to_string()));
                assert!(candidates.iter().any(|c| c == &bobby.id.to_string()));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_reports_stale_snapshot_token() {
        let app_core = crate::testing::default_test_app_core();
        let bob = Contact {
            id: AuthorityId::new_from_entropy([4u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob]));
        }

        let resolver = CommandResolver::default();
        let stale_snapshot = resolver.capture_snapshot(&app_core).await;
        let _fresh_snapshot = resolver.capture_snapshot(&app_core).await;

        let err = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: "bob".to_string(),
                },
                &stale_snapshot,
            )
            .expect_err("expected stale snapshot");

        match err {
            CommandResolverError::StaleSnapshot { provided, latest } => {
                assert!(provided.value() < latest.value());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_accepts_explicit_authority_id_without_contact_entry() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let authority_id = AuthorityId::new_from_entropy([5u8; 32]);

        let resolved = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: authority_id.to_string(),
                },
                &snapshot,
            )
            .expect("explicit authority ids should resolve without local contacts");

        match resolved {
            ResolvedCommand::Whois { target } => {
                assert_eq!(target, ResolvedAuthorityId(authority_id));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_resolves_existing_channel_for_mode() {
        let app_core = crate::testing::default_test_app_core();
        let channel_id = ChannelId::from_bytes([9u8; 32]);

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: None,
                    name: "slash-lab".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 0,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Mode {
                    channel: "slash-lab".to_string(),
                    flags: "+m".to_string(),
                },
                &snapshot,
            )
            .expect("channel should resolve");

        match resolved {
            ResolvedCommand::Mode {
                channel,
                channel_name,
                flags,
                ..
            } => {
                assert_eq!(channel.channel_id(), ResolvedChannelId(channel_id));
                assert_eq!(channel_name, "slash-lab");
                assert_eq!(flags, "+m");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_marks_unknown_join_channel_as_will_create() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "typo-room".to_string(),
                },
                &snapshot,
            )
            .expect("join should preserve create semantics");

        match resolved {
            ResolvedCommand::Join {
                channel_name,
                channel:
                    ChannelResolveOutcome::WillCreate {
                        channel_name: outcome_name,
                    },
            } => {
                assert_eq!(channel_name, "typo-room");
                assert_eq!(outcome_name, "typo-room");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[tokio::test]
    async fn resolver_rejects_unknown_join_channel_id_without_materialization() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let unknown_channel_id = ChannelId::from_bytes([9u8; 32]).to_string();

        let resolved = resolver.resolve(
            ParsedCommand::Join {
                channel: unknown_channel_id.clone(),
            },
            &snapshot,
        );

        assert_eq!(
            resolved,
            Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                input: unknown_channel_id,
            })
        );
    }

    #[tokio::test]
    async fn resolver_does_not_treat_home_names_as_existing_channels() {
        let app_core = crate::testing::default_test_app_core();
        let home_id = ChannelId::from_bytes([10u8; 32]);
        let owner = AuthorityId::new_from_entropy([16u8; 32]);
        let context_id = ContextId::new_from_entropy([17u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(crate::views::home::HomeState::new(
                home_id,
                Some("slash-lab".to_string()),
                owner,
                0,
                context_id,
            ));
            core.views_mut().set_homes(homes);
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let join = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "slash-lab".to_string(),
                },
                &snapshot,
            )
            .expect("join should treat unmatched channel name as create intent");
        match join {
            ResolvedCommand::Join {
                channel: ChannelResolveOutcome::WillCreate { channel_name },
                ..
            } => {
                assert_eq!(channel_name, "slash-lab");
            }
            other => panic!("unexpected join resolution: {other:?}"),
        }

        let mode = resolver.resolve(
            ParsedCommand::Mode {
                channel: "slash-lab".to_string(),
                flags: "+m".to_string(),
            },
            &snapshot,
        );
        assert!(matches!(
            mode,
            Err(CommandResolverError::UnknownTarget {
                target: ResolveTarget::Channel,
                ..
            })
        ));
    }

    #[test]
    fn moderation_plan_accepts_only_moderation_commands() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([11u8; 32]));
        let valid = ResolvedCommand::Mute {
            target,
            duration: None,
        };
        let invalid = ResolvedCommand::Who;

        assert!(ModerationPlan::from_resolved(valid).is_ok());
        assert_eq!(
            ModerationPlan::from_resolved(invalid),
            Err(CommandPlanError::NotModerationCommand)
        );
    }

    #[test]
    fn moderator_plan_accepts_only_moderator_commands() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([12u8; 32]));
        let valid = ResolvedCommand::Op { target };
        let invalid = ResolvedCommand::Leave;

        assert!(ModeratorPlan::from_resolved(valid).is_ok());
        assert_eq!(
            ModeratorPlan::from_resolved(invalid),
            Err(CommandPlanError::NotModeratorCommand)
        );
    }

    #[test]
    fn membership_plan_accepts_join_and_leave() {
        let valid_join = ResolvedCommand::Join {
            channel_name: "slash-lab".to_string(),
            channel: ChannelResolveOutcome::Existing(ExistingChannelResolution::new(
                ResolvedChannelId(ChannelId::from_bytes([13u8; 32])),
                None,
            )),
        };
        let valid_leave = ResolvedCommand::Leave;
        let invalid = ResolvedCommand::Nick {
            name: "new-name".to_string(),
        };

        assert!(MembershipPlan::from_resolved(valid_join).is_ok());
        assert!(MembershipPlan::from_resolved(valid_leave).is_ok());
        assert_eq!(
            MembershipPlan::from_resolved(invalid),
            Err(CommandPlanError::NotMembershipCommand)
        );
    }

    #[test]
    fn consistency_table_matches_command_requirements() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([14u8; 32]));
        let channel = ResolvedChannelId(ChannelId::from_bytes([15u8; 32]));
        let channel_mode = ResolvedCommand::Mode {
            channel_name: "slash-lab".to_string(),
            channel: ExistingChannelResolution::new(channel, None),
            flags: "+m".to_string(),
        };

        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Join {
                channel_name: "slash-lab".to_string(),
                channel: ChannelResolveOutcome::Existing(ExistingChannelResolution::new(
                    channel, None,
                )),
            }),
            ConsistencyRequirement::Replicated
        );
        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Mute {
                target,
                duration: None,
            }),
            ConsistencyRequirement::Enforced
        );
        assert_eq!(
            consistency_for_resolved(&channel_mode),
            ConsistencyRequirement::Enforced
        );
        assert_eq!(
            consistency_for_resolved(&ResolvedCommand::Who),
            ConsistencyRequirement::Accepted
        );
    }

    #[tokio::test]
    async fn join_create_plan_uses_global_scope_and_accepted_consistency() {
        let app_core = crate::testing::default_test_app_core();
        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;

        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "future-room".to_string(),
                },
                &snapshot,
            )
            .expect("join create intent should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("join create plan should succeed");

        match &plan {
            PlannedCommand::Membership(plan) => {
                assert_eq!(plan.scope, CommandScope::Global);
                assert!(
                    plan.preconditions.is_empty(),
                    "create intent should not claim canonical channel preconditions"
                );
            }
            other => panic!("unexpected plan: {other:?}"),
        }
        assert_eq!(
            plan.consistency_requirement(),
            ConsistencyRequirement::Accepted
        );
    }

    proptest! {
        #[test]
        fn moderation_plan_preserves_canonical_target_id(
            entropy in any::<[u8; 32]>()
        ) {
            let expected = ResolvedAuthorityId(AuthorityId::new_from_entropy(entropy));
            let plan = ModerationPlan::from_resolved(ResolvedCommand::Mute {
                target: expected,
                duration: None,
            })
            .expect("mute command should plan");

            match plan.command {
                ResolvedCommand::Mute { target, .. } => prop_assert_eq!(target, expected),
                other => prop_assert!(false, "unexpected command shape: {other:?}"),
            }
        }
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn whois_plan_does_not_reresolve_after_contacts_change() {
        let app_core = crate::testing::default_test_app_core();
        let original = Contact {
            id: AuthorityId::new_from_entropy([21u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };
        let replacement = Contact {
            id: AuthorityId::new_from_entropy([22u8; 32]),
            nickname: "bob".to_string(),
            nickname_suggestion: None,
            is_guardian: false,
            is_member: true,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![original.clone()]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Whois {
                    target: "bob".to_string(),
                },
                &snapshot,
            )
            .expect("initial resolve should succeed");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("planning should succeed");

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![replacement]));
        }

        let error = execute_planned(&app_core, plan)
            .await
            .expect_err("planned whois should not reresolve to replacement contact");
        assert!(
            error.to_string().contains(&original.id.to_string()),
            "expected missing original authority id in error, got: {error}"
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_reports_runtime_unavailable_degraded_state() {
        let app_core = crate::testing::default_test_app_core();
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([31u8; 32]));
        let plan = PlannedCommand::Moderator(CommandPlan {
            actor: None,
            scope: CommandScope::Global,
            preconditions: vec![PlanPrecondition::TargetExists(target)],
            operation: ModeratorPlan {
                command: ResolvedCommand::Op { target },
            },
        });

        let state = wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Enforced).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Degraded {
                requirement: ConsistencyRequirement::Enforced,
                reason: ConsistencyDegradedReason::RuntimeUnavailable,
            }
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_reports_replicated_when_join_is_visible() {
        let app_core = crate::testing::default_test_app_core();
        let channel_id = ChannelId::from_bytes([32u8; 32]);

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: None,
                    name: "replicated-room".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        let plan = PlannedCommand::Membership(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(channel_id),
                context_id: None,
            },
            preconditions: vec![PlanPrecondition::ChannelExists(ResolvedChannelId(
                channel_id,
            ))],
            operation: MembershipPlan {
                command: ResolvedCommand::Join {
                    channel_name: "replicated-room".to_string(),
                    channel: ChannelResolveOutcome::Existing(ExistingChannelResolution::new(
                        ResolvedChannelId(channel_id),
                        None,
                    )),
                },
            },
        });

        let state =
            wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Replicated).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Satisfied(ConsistencyWitness::Replicated)
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_treats_missing_leave_scope_as_replicated() {
        let app_core = crate::testing::default_test_app_core();
        let missing_channel = ChannelId::from_bytes([44u8; 32]);

        let plan = PlannedCommand::Membership(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(missing_channel),
                context_id: None,
            },
            preconditions: Vec::new(),
            operation: MembershipPlan {
                command: ResolvedCommand::Leave,
            },
        });

        let state =
            wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Replicated).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Satisfied(ConsistencyWitness::Replicated)
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn consistency_barrier_treats_missing_home_scope_as_timed_out_degraded_state() {
        let authority = AuthorityId::new_from_entropy([90u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        let missing_channel = ChannelId::from_bytes([45u8; 32]);
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([46u8; 32]));

        let plan = PlannedCommand::Moderation(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id: ResolvedChannelId(missing_channel),
                context_id: None,
            },
            preconditions: vec![PlanPrecondition::TargetExists(target)],
            operation: ModerationPlan {
                command: ResolvedCommand::Kick {
                    target,
                    reason: None,
                },
            },
        });

        let state = wait_for_consistency(&app_core, &plan, ConsistencyRequirement::Enforced).await;
        assert_eq!(
            state,
            CommandCompletionOutcome::Degraded {
                requirement: ConsistencyRequirement::Enforced,
                reason: ConsistencyDegradedReason::OperationTimedOut,
            }
        );
    }

    #[tokio::test]
    async fn invite_plan_uses_accepted_consistency_requirement() {
        let target = ResolvedAuthorityId(AuthorityId::new_from_entropy([49u8; 32]));
        let channel_id = ResolvedChannelId(ChannelId::from_bytes([50u8; 32]));
        let plan = PlannedCommand::Moderation(CommandPlan {
            actor: None,
            scope: CommandScope::Channel {
                channel_id,
                context_id: None,
            },
            preconditions: vec![
                PlanPrecondition::TargetExists(target),
                PlanPrecondition::ChannelExists(channel_id),
            ],
            operation: ModerationPlan {
                command: ResolvedCommand::Invite { target },
            },
        });

        assert_eq!(
            plan.consistency_requirement(),
            ConsistencyRequirement::Accepted
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn home_for_channel_scope_does_not_fallback_to_current_home() {
        use crate::views::home::HomesState;

        let scoped_channel_id = ChannelId::from_bytes([51u8; 32]);
        let current_home_id = ChannelId::from_bytes([52u8; 32]);
        let current_context_id = ContextId::new_from_entropy([53u8; 32]);
        let creator = AuthorityId::new_from_entropy([54u8; 32]);

        let mut homes = HomesState::new();
        let result = homes.add_home(crate::views::home::HomeState::new(
            current_home_id,
            Some("current-home".to_string()),
            creator,
            0,
            current_context_id,
        ));
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }

        let snapshot = StateSnapshot {
            homes,
            ..StateSnapshot::default()
        };

        let resolved = home_for_scope(
            &snapshot,
            &CommandScope::Channel {
                channel_id: ResolvedChannelId(scoped_channel_id),
                context_id: None,
            },
        );
        assert!(
            resolved.is_none(),
            "channel-scoped lookup should not silently fall back to current home"
        );
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn execute_planned_join_preserves_join_operation_id() {
        let app_core = crate::testing::default_test_app_core();
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Join {
                    channel: "semantic-room".to_string(),
                },
                &snapshot,
            )
            .expect("join should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, None, None)
            .expect("join should plan");

        execute_planned(&app_core, plan)
            .await
            .expect("join should execute");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                status,
                ..
            } if *operation_id == OperationId::join_channel()
                && status.kind == SemanticOperationKind::JoinChannel
                && status.phase == SemanticOperationPhase::Succeeded
        )));
    }

    #[cfg(feature = "signals")]
    #[tokio::test]
    async fn execute_planned_me_preserves_send_message_operation_id() {
        let authority = AuthorityId::new_from_entropy([47u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let channel_id = ChannelId::from_bytes([44u8; 32]);
        let context_id = ContextId::new_from_entropy([48u8; 32]);
        runtime.set_materialized_channel_name_matches("semantic-room", vec![channel_id]);
        runtime.set_amp_channel_context(channel_id, context_id);
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: Some(context_id),
                    name: "semantic-room".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        let resolver = CommandResolver::default();
        let snapshot = resolver.capture_snapshot(&app_core).await;
        let resolved = resolver
            .resolve(
                ParsedCommand::Me {
                    action: "wave".to_string(),
                },
                &snapshot,
            )
            .expect("me should resolve");
        let plan = resolver
            .plan(resolved, &snapshot, Some("semantic-room"), None)
            .expect("me should plan");

        let _error = execute_planned(&app_core, plan)
            .await
            .expect_err("me should still fail explicitly without a full runtime transport");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                status,
                ..
            } if *operation_id == OperationId::send_message()
                && status.kind == SemanticOperationKind::SendChatMessage
        )));
    }

    #[test]
    fn command_completion_outcome_maps_timeout_without_terminal_string_parsing() {
        let classification = CommandCompletionOutcome::Degraded {
            requirement: ConsistencyRequirement::Enforced,
            reason: ConsistencyDegradedReason::OperationTimedOut,
        }
        .terminal_classification()
        .expect("degraded timeout should classify");

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Failed);
        assert_eq!(
            classification.reason,
            CommandTerminalReasonCode::OperationTimedOut
        );
    }

    #[test]
    fn command_completion_outcome_maps_runtime_unavailable_without_terminal_string_parsing() {
        let classification = CommandCompletionOutcome::Degraded {
            requirement: ConsistencyRequirement::Replicated,
            reason: ConsistencyDegradedReason::RuntimeUnavailable,
        }
        .terminal_classification()
        .expect("runtime unavailable should classify");

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Failed);
        assert_eq!(
            classification.reason,
            CommandTerminalReasonCode::Unavailable
        );
    }

    #[test]
    fn classify_terminal_execution_error_maps_unknown_precondition_to_not_found() {
        let classification = classify_terminal_execution_error(&AuraError::invalid(
            "precondition failed: unknown channel target: channel-123",
        ));

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Invalid);
        assert_eq!(classification.reason, CommandTerminalReasonCode::NotFound);
    }

    #[test]
    fn classify_terminal_execution_error_maps_permission_detail_without_terminal_string_parsing() {
        let classification =
            classify_terminal_execution_error(&AuraError::permission_denied("target is muted"));

        assert_eq!(classification.status, CommandTerminalOutcomeStatus::Denied);
        assert_eq!(classification.reason, CommandTerminalReasonCode::Muted);
    }
}
