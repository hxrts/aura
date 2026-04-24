use super::super::common::{
    get_bool, get_channel_id, get_int, get_optional_context_id, get_optional_string, get_string,
};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryAccessPolicy, QueryCapability, QueryParseError,
};
use serde::{Deserialize, Serialize};

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

                Ok(HomeState {
                    id: get_channel_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    members: Vec::new(),
                    my_role,
                    storage: HomeFlowBudget::default(),
                    online_count: get_int(&row, "online_count") as u32,
                    member_count: get_int(&row, "member_count") as u32,
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
