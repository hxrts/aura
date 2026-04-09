use super::super::common::{get_channel_id, get_int, get_optional_string, get_string};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryCapability, QueryParseError,
};
use serde::{Deserialize, Serialize};

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
