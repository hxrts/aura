//! Message-oriented query types.

use super::common::{
    get_authority_id, get_bool, get_channel_id, get_int, get_optional_string, get_string,
};
use crate::views::MessageDeliveryStatus;
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryCapability, QueryParseError,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct MessagesQuery {
    pub channel_id: String,
    pub limit: u32,
    pub after: Option<String>,
    pub unread_only: bool,
}

impl Query for MessagesQuery {
    type Result = Vec<crate::views::chat::Message>;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![
            DatalogFact::new(
                "message",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("channel"),
                    DatalogValue::var("sender"),
                    DatalogValue::var("sender_name"),
                    DatalogValue::var("content"),
                    DatalogValue::var("timestamp"),
                ],
            ),
            DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("channel"),
                    DatalogValue::String(self.channel_id.clone()),
                ],
            ),
        ];

        if self.unread_only {
            body.push(DatalogFact::new(
                "message_unread",
                vec![DatalogValue::var("id")],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("sender"),
                    DatalogValue::var("sender_name"),
                    DatalogValue::var("content"),
                    DatalogValue::var("timestamp"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![
            QueryCapability::read("messages"),
            QueryCapability::read(format!("channel:{}", self.channel_id)),
        ]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::with_args("message", vec![("channel", &self.channel_id)]),
            FactPredicate::new("message_unread"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::chat::Message;

        bindings
            .rows
            .into_iter()
            .map(|row| {
                Ok(Message {
                    id: get_string(&row, "id"),
                    channel_id: get_channel_id(&row, "channel")?,
                    sender_id: get_authority_id(&row, "sender")?,
                    sender_name: get_string(&row, "sender_name"),
                    content: get_string(&row, "content"),
                    timestamp: get_int(&row, "timestamp") as u64,
                    reply_to: get_optional_string(&row, "reply_to"),
                    is_own: false,
                    is_read: get_bool(&row, "is_read"),
                    delivery_status: MessageDeliveryStatus::default(),
                    epoch_hint: None,
                    is_finalized: false,
                })
            })
            .collect()
    }
}
