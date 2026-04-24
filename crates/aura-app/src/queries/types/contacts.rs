//! Contact- and invitation-oriented query types.

use super::common::{
    get_authority_id, get_bool, get_int, get_optional_authority_id, get_optional_channel_id,
    get_optional_string, get_string,
};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryAccessPolicy, QueryCapability, QueryParseError,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct InvitationsQuery {
    pub direction: Option<String>,
    pub status: Option<String>,
    pub invitation_type: Option<String>,
}

impl Query for InvitationsQuery {
    type Result = crate::views::invitations::InvitationsState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "invitation",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("type"),
                DatalogValue::var("status"),
                DatalogValue::var("direction"),
                DatalogValue::var("from"),
                DatalogValue::var("to"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(direction) = &self.direction {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("direction"),
                    DatalogValue::String(direction.clone()),
                ],
            ));
        }

        if let Some(status) = &self.status {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String(status.clone()),
                ],
            ));
        }

        if let Some(invitation_type) = &self.invitation_type {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("type"),
                    DatalogValue::String(invitation_type.clone()),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("type"),
                    DatalogValue::var("status"),
                    DatalogValue::var("direction"),
                    DatalogValue::var("from"),
                    DatalogValue::var("to"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn access_policy(&self) -> QueryAccessPolicy {
        QueryAccessPolicy::protected(QueryCapability::read("invitations"))
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("invitation")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::invitations::{
            Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
        };

        let invitations: Vec<Invitation> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let invitation_type = match get_string(&row, "type").as_str() {
                    "guardian" => InvitationType::Guardian,
                    "chat" => InvitationType::Chat,
                    "home" => InvitationType::Home,
                    _ => InvitationType::Home,
                };
                let status = match get_string(&row, "status").as_str() {
                    "pending" => InvitationStatus::Pending,
                    "accepted" => InvitationStatus::Accepted,
                    "rejected" => InvitationStatus::Rejected,
                    "expired" => InvitationStatus::Expired,
                    "revoked" => InvitationStatus::Revoked,
                    _ => InvitationStatus::Pending,
                };
                let direction = match get_string(&row, "direction").as_str() {
                    "sent" => InvitationDirection::Sent,
                    "received" => InvitationDirection::Received,
                    _ => InvitationDirection::Received,
                };

                Ok(Invitation {
                    id: get_string(&row, "id"),
                    invitation_type,
                    status,
                    direction,
                    from_id: get_authority_id(&row, "from_id")?,
                    from_name: get_string(&row, "from"),
                    to_id: get_optional_authority_id(&row, "to_id")?,
                    to_name: get_optional_string(&row, "to"),
                    created_at: get_int(&row, "created_at") as u64,
                    expires_at: {
                        let timestamp = get_int(&row, "expires_at");
                        (timestamp > 0).then_some(timestamp as u64)
                    },
                    message: get_optional_string(&row, "message"),
                    home_id: get_optional_channel_id(&row, "home_id")?,
                    home_name: get_optional_string(&row, "home_name"),
                })
            })
            .collect::<Result<_, _>>()?;

        let pending = invitations
            .iter()
            .filter(|invitation| {
                invitation.status == InvitationStatus::Pending
                    && invitation.direction == InvitationDirection::Received
            })
            .cloned()
            .collect();
        let sent = invitations
            .iter()
            .filter(|invitation| {
                invitation.status == InvitationStatus::Pending
                    && invitation.direction == InvitationDirection::Sent
            })
            .cloned()
            .collect();
        let history = invitations
            .iter()
            .filter(|invitation| invitation.status != InvitationStatus::Pending)
            .cloned()
            .collect();

        Ok(InvitationsState::from_parts(pending, sent, history))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ContactsQuery {
    pub search: Option<String>,
    pub guardians_only: bool,
    pub members_only: bool,
}

impl Query for ContactsQuery {
    type Result = crate::views::contacts::ContactsState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "contact",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("nickname"),
                DatalogValue::var("nickname_suggestion"),
                DatalogValue::var("is_guardian"),
                DatalogValue::var("is_member"),
                DatalogValue::var("last_interaction"),
                DatalogValue::var("is_online"),
            ],
        )];

        if self.guardians_only {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("is_guardian"),
                    DatalogValue::Boolean(true),
                ],
            ));
        }

        if self.members_only {
            body.push(DatalogFact::new(
                "eq",
                vec![DatalogValue::var("is_member"), DatalogValue::Boolean(true)],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("nickname"),
                    DatalogValue::var("nickname_suggestion"),
                    DatalogValue::var("is_guardian"),
                    DatalogValue::var("is_member"),
                    DatalogValue::var("last_interaction"),
                    DatalogValue::var("is_online"),
                ],
            ),
            body,
        }])
    }

    fn access_policy(&self) -> QueryAccessPolicy {
        QueryAccessPolicy::protected(QueryCapability::read("contacts"))
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("contact")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::contacts::{
            Contact, ContactRelationshipState, ContactsState, ReadReceiptPolicy,
        };

        let contacts = bindings
            .rows
            .into_iter()
            .map(|row| {
                Ok(Contact {
                    id: get_authority_id(&row, "id")?,
                    nickname: get_string(&row, "nickname"),
                    nickname_suggestion: get_optional_string(&row, "nickname_suggestion"),
                    is_guardian: get_bool(&row, "is_guardian"),
                    is_member: get_bool(&row, "is_member"),
                    last_interaction: {
                        let timestamp = get_int(&row, "last_interaction");
                        (timestamp > 0).then_some(timestamp as u64)
                    },
                    is_online: get_bool(&row, "is_online"),
                    read_receipt_policy: ReadReceiptPolicy::default(),
                    relationship_state: ContactRelationshipState::Contact,
                    // The datalog "contact" predicate does not currently
                    // surface the invitation code. The authoritative
                    // ContactFact::Added.invitation_code is projected into
                    // the Contact view through ContactsSignalView; this
                    // query-based path is used for ad-hoc reads that do
                    // not need the code.
                    invitation_code: None,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ContactsState::from_contacts(contacts))
    }
}
