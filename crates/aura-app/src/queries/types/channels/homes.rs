use super::super::common::{
    get_bool, get_channel_id, get_int, get_optional_context_id, get_optional_string, get_string,
};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRow, DatalogRule, DatalogValue,
    FactPredicate, Query, QueryAccessPolicy, QueryCapability, QueryParseError,
};
use serde::{Deserialize, Serialize};

fn required_count(row: &DatalogRow, field: &str) -> Result<u32, QueryParseError> {
    let value = row
        .get(field)
        .ok_or_else(|| QueryParseError::MissingField {
            field: field.to_string(),
        })?;
    match value {
        DatalogValue::Integer(value) => {
            u32::try_from(*value).map_err(|_| QueryParseError::InvalidValue {
                field: field.to_string(),
                reason: "count must be a nonnegative u32".into(),
            })
        }
        DatalogValue::String(value) => {
            value
                .parse::<u32>()
                .map_err(|_| QueryParseError::InvalidValue {
                    field: field.to_string(),
                    reason: "count must be a nonnegative u32".into(),
                })
        }
        _ => Err(QueryParseError::InvalidValue {
            field: field.to_string(),
            reason: "count must be a nonnegative u32".into(),
        }),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct HomesQuery {
    pub home_id: Option<String>,
    pub admin_only: bool,
}

impl Query for HomesQuery {
    type Result = crate::views::home::HomesState;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "home",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("is_primary"),
                DatalogValue::var("my_role"),
                DatalogValue::var("member_count"),
                DatalogValue::var("online_count"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(home_id) = &self.home_id {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::String(home_id.clone()),
                ],
            ));
        }

        if self.admin_only {
            body.push(DatalogFact::new(
                "in",
                vec![
                    DatalogValue::var("my_role"),
                    DatalogValue::symbol("admin"),
                    DatalogValue::symbol("owner"),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("is_primary"),
                    DatalogValue::var("my_role"),
                    DatalogValue::var("member_count"),
                    DatalogValue::var("online_count"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn access_policy(&self) -> QueryAccessPolicy {
        QueryAccessPolicy::protected(QueryCapability::read("homes"))
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("home"),
            FactPredicate::new("home_member"),
            FactPredicate::new("home_role"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::home::{HomeRole, HomeState, HomesState};
        use crate::workflows::budget::HomeFlowBudget;
        use std::collections::HashMap;

        let homes_list: Vec<HomeState> = bindings
            .rows
            .into_iter()
            .map(|row| {
                let my_role = match get_string(&row, "my_role").as_str() {
                    "owner" => HomeRole::Member,
                    "admin" => HomeRole::Moderator,
                    _ => HomeRole::Participant,
                };

                let member_count = required_count(&row, "member_count")?;
                let online_count = required_count(&row, "online_count")?;
                if online_count > member_count {
                    return Err(QueryParseError::InvalidValue {
                        field: "online_count".into(),
                        reason: "online count exceeds member count".into(),
                    });
                }

                Ok(HomeState {
                    id: get_channel_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    members: Vec::new(),
                    my_role,
                    storage: HomeFlowBudget::default(),
                    online_count,
                    member_count,
                    is_primary: get_bool(&row, "is_primary"),
                    topic: get_optional_string(&row, "topic"),
                    pinned_messages: Vec::new(),
                    pinned_metadata: HashMap::default(),
                    mode_flags: None,
                    access_overrides: HashMap::default(),
                    access_level_capabilities: None,
                    ban_list: HashMap::default(),
                    mute_list: HashMap::default(),
                    kick_log: Vec::new(),
                    created_at: get_int(&row, "created_at") as u64,
                    context_id: get_optional_context_id(&row, "context_id")?,
                })
            })
            .collect::<Result<_, _>>()?;

        let mut homes = std::collections::HashMap::new();
        let mut current_home_id = None;

        for home in homes_list {
            if home.is_primary && current_home_id.is_none() {
                current_home_id = Some(home.id);
            }
            homes.insert(home.id, home);
        }

        if current_home_id.is_none() {
            current_home_id = homes.keys().next().copied();
        }

        Ok(HomesState::from_parts(homes, current_home_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::query::DatalogRow;
    use aura_core::types::identifiers::ChannelId;

    fn row(member_count: DatalogValue, online_count: DatalogValue) -> DatalogRow {
        DatalogRow::new()
            .with_binding(
                "id",
                DatalogValue::String(ChannelId::from_bytes([1; 32]).to_string()),
            )
            .with_binding("name", DatalogValue::String("Den".into()))
            .with_binding("my_role", DatalogValue::String("owner".into()))
            .with_binding("member_count", member_count)
            .with_binding("online_count", online_count)
            .with_binding("created_at", DatalogValue::Integer(1))
    }

    #[test]
    fn home_query_rejects_missing_or_invalid_counts_without_fabricating_defaults() {
        let valid = HomesQuery::parse(
            DatalogBindings::new()
                .with_row(row(DatalogValue::Integer(2), DatalogValue::Integer(1))),
        )
        .unwrap();
        let home = valid.all_homes().next().unwrap();
        assert_eq!((home.member_count, home.online_count), (2, 1));

        for invalid in [
            row(DatalogValue::Integer(-1), DatalogValue::Integer(0)),
            row(DatalogValue::Integer(1), DatalogValue::Integer(2)),
            row(
                DatalogValue::String("unknown".into()),
                DatalogValue::Integer(0),
            ),
            row(
                DatalogValue::Integer(u32::MAX as i64 + 1),
                DatalogValue::Integer(0),
            ),
        ] {
            assert!(HomesQuery::parse(DatalogBindings::new().with_row(invalid)).is_err());
        }
        let missing = DatalogRow::new().with_binding(
            "id",
            DatalogValue::String(ChannelId::from_bytes([2; 32]).to_string()),
        );
        assert!(matches!(
            HomesQuery::parse(DatalogBindings::new().with_row(missing)),
            Err(QueryParseError::MissingField { field }) if field == "member_count"
        ));
    }
}
