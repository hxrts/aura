use super::super::common::{get_channel_id, get_int, get_optional_string, get_string};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryAccessPolicy, QueryCapability, QueryParseError,
};
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

    fn access_policy(&self) -> QueryAccessPolicy {
        QueryAccessPolicy::protected(QueryCapability::read("channels"))
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
