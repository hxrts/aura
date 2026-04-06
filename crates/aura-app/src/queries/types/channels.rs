//! Channel-, home-, and neighborhood-oriented query types.

use super::common::{
    get_bool, get_channel_id, get_int, get_optional_context_id, get_optional_string, get_string,
};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryCapability, QueryParseError,
};
use aura_core::types::ChannelId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChannelsQuery {
    pub channel_type: Option<crate::views::chat::ChannelType>,
    pub include_archived: bool,
}

impl Query for ChannelsQuery {
    type Result = Vec<crate::views::chat::Channel>;

    fn to_datalog(&self) -> DatalogProgram {
        use crate::views::chat::ChannelType;

        let mut body = vec![DatalogFact::new(
            "channel",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("type"),
                DatalogValue::var("topic"),
                DatalogValue::var("archived"),
            ],
        )];

        if let Some(channel_type) = self.channel_type {
            let channel_type = match channel_type {
                ChannelType::DirectMessage => Some("DirectMessage"),
                ChannelType::Guardian => Some("Guardian"),
                ChannelType::Home => Some("Home"),
                ChannelType::All => None,
            };
            if let Some(channel_type) = channel_type {
                body.push(DatalogFact::new(
                    "eq",
                    vec![
                        DatalogValue::var("type"),
                        DatalogValue::String(channel_type.to_string()),
                    ],
                ));
            }
        }

        if !self.include_archived {
            body.push(DatalogFact::new(
                "eq",
                vec![DatalogValue::var("archived"), DatalogValue::Boolean(false)],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("topic"),
                    DatalogValue::var("archived"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("channels")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("channel")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::{Channel, ChannelType};

        bindings
            .rows
            .into_iter()
            .map(|row| {
                let channel_type = match get_string(&row, "type").as_str() {
                    "DirectMessage" => ChannelType::DirectMessage,
                    "Guardian" => ChannelType::Guardian,
                    _ => ChannelType::Home,
                };

                Ok(Channel {
                    id: get_channel_id(&row, "id")?,
                    context_id: None,
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_ids: Vec::new(),
                    member_count: get_int(&row, "member_count") as u32,
                    last_message: get_optional_string(&row, "last_message"),
                    last_message_time: {
                        let timestamp = get_int(&row, "last_message_time");
                        (timestamp > 0).then_some(timestamp as u64)
                    },
                    last_activity: get_int(&row, "last_activity") as u64,
                    last_finalized_epoch: 0,
                })
            })
            .collect()
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
                    DatalogValue::Symbol("admin".to_string()),
                    DatalogValue::Symbol("owner".to_string()),
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

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("homes")]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct NeighborhoodQuery {
    pub position_home_id: Option<String>,
    pub max_depth: Option<u32>,
}

impl Query for NeighborhoodQuery {
    type Result = crate::views::neighborhood::NeighborhoodState;

    fn to_datalog(&self) -> DatalogProgram {
        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("one_hop_link"),
                    DatalogValue::var("shared_contacts"),
                    DatalogValue::var("can_traverse"),
                ],
            ),
            body: vec![DatalogFact::new(
                "neighbor_home",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("one_hop_link"),
                    DatalogValue::var("shared_contacts"),
                    DatalogValue::var("can_traverse"),
                ],
            )],
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("neighborhood")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("neighbor_home"),
            FactPredicate::new("home_one_hop_link"),
            FactPredicate::new("shared_contact"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::neighborhood::{NeighborHome, NeighborhoodState, OneHopLinkType};

        let neighbors = bindings
            .rows
            .into_iter()
            .map(|row| {
                let one_hop_link = match get_string(&row, "one_hop_link").as_str() {
                    "direct" => OneHopLinkType::Direct,
                    "two_hop" => OneHopLinkType::TwoHop,
                    _ => OneHopLinkType::Distant,
                };

                Ok(NeighborHome {
                    id: get_channel_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    one_hop_link,
                    shared_contacts: get_int(&row, "shared_contacts") as u32,
                    member_count: {
                        let count = get_int(&row, "member_count");
                        (count > 0).then_some(count as u32)
                    },
                    can_traverse: get_bool(&row, "can_traverse"),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(NeighborhoodState::from_parts(
            ChannelId::default(),
            String::new(),
            neighbors,
        ))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct ChatQuery {}

impl Query for ChatQuery {
    type Result = crate::views::chat::ChatState;

    fn to_datalog(&self) -> DatalogProgram {
        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("unread_count"),
                ],
            ),
            body: vec![DatalogFact::new(
                "channel",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("type"),
                    DatalogValue::var("topic"),
                    DatalogValue::var("archived"),
                ],
            )],
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![
            QueryCapability::read("channels"),
            QueryCapability::read("messages"),
        ]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("channel"),
            FactPredicate::new("message"),
            FactPredicate::new("message_unread"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::{Channel, ChannelType, ChatState};

        let channels = bindings
            .rows
            .into_iter()
            .map(|row| {
                let channel_type = match get_string(&row, "type").as_str() {
                    "DirectMessage" => ChannelType::DirectMessage,
                    "Guardian" => ChannelType::Guardian,
                    _ => ChannelType::Home,
                };

                Ok(Channel {
                    id: get_channel_id(&row, "id")?,
                    context_id: None,
                    name: get_string(&row, "name"),
                    topic: get_optional_string(&row, "topic"),
                    channel_type,
                    is_dm: channel_type == ChannelType::DirectMessage,
                    unread_count: get_int(&row, "unread_count") as u32,
                    member_ids: Vec::new(),
                    member_count: 0,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(ChatState::from_channels(channels))
    }
}
