//! Shared query helpers and common query types.

use aura_core::query::{
    DatalogBindings, DatalogProgram, DatalogRow, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryCapability, QueryParseError,
};
use aura_core::types::{AuthorityId, ChannelId, ContextId};
use serde::{Deserialize, Serialize};

pub(super) fn get_string(row: &DatalogRow, key: &str) -> String {
    row.get(key)
        .and_then(|value| match value {
            DatalogValue::String(string) => Some(string.clone()),
            DatalogValue::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

pub(super) fn get_optional_string(row: &DatalogRow, key: &str) -> Option<String> {
    let value = get_string(row, key);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(super) fn get_authority_id(
    row: &DatalogRow,
    key: &str,
) -> Result<AuthorityId, QueryParseError> {
    let value = get_string(row, key);
    if value.is_empty() {
        return Err(QueryParseError::MissingField {
            field: key.to_string(),
        });
    }
    value
        .parse::<AuthorityId>()
        .map_err(|error| QueryParseError::InvalidValue {
            field: key.to_string(),
            reason: error.to_string(),
        })
}

pub(super) fn get_optional_authority_id(
    row: &DatalogRow,
    key: &str,
) -> Result<Option<AuthorityId>, QueryParseError> {
    let value = get_string(row, key);
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<AuthorityId>()
            .map(Some)
            .map_err(|error| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: error.to_string(),
            })
    }
}

pub(super) fn get_channel_id(row: &DatalogRow, key: &str) -> Result<ChannelId, QueryParseError> {
    let value = get_string(row, key);
    if value.is_empty() {
        return Err(QueryParseError::MissingField {
            field: key.to_string(),
        });
    }
    value
        .parse::<ChannelId>()
        .map_err(|error| QueryParseError::InvalidValue {
            field: key.to_string(),
            reason: error.to_string(),
        })
}

pub(super) fn get_optional_channel_id(
    row: &DatalogRow,
    key: &str,
) -> Result<Option<ChannelId>, QueryParseError> {
    let value = get_string(row, key);
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<ChannelId>()
            .map(Some)
            .map_err(|error| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: error.to_string(),
            })
    }
}

pub(super) fn get_optional_context_id(
    row: &DatalogRow,
    key: &str,
) -> Result<Option<ContextId>, QueryParseError> {
    let value = get_string(row, key);
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<ContextId>()
            .map(Some)
            .map_err(|error| QueryParseError::InvalidValue {
                field: key.to_string(),
                reason: error.to_string(),
            })
    }
}

pub(super) fn get_int(row: &DatalogRow, key: &str) -> i64 {
    row.get(key)
        .and_then(|value| match value {
            DatalogValue::Integer(integer) => Some(*integer),
            DatalogValue::String(string) => string.parse().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

pub(super) fn get_bool(row: &DatalogRow, key: &str) -> bool {
    row.get(key)
        .map(|value| match value {
            DatalogValue::Boolean(boolean) => *boolean,
            DatalogValue::String(string) => string == "true",
            _ => false,
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct UnreadCountQuery {
    pub channel_id: Option<String>,
}

impl Query for UnreadCountQuery {
    type Result = u32;

    fn to_datalog(&self) -> DatalogProgram {
        let body = if let Some(channel_id) = &self.channel_id {
            vec![
                aura_core::query::DatalogFact::new("message_unread", vec![DatalogValue::var("id")]),
                aura_core::query::DatalogFact::new(
                    "message",
                    vec![
                        DatalogValue::var("id"),
                        DatalogValue::String(channel_id.clone()),
                        DatalogValue::var("_sender"),
                        DatalogValue::var("_content"),
                        DatalogValue::var("_time"),
                    ],
                ),
            ]
        } else {
            vec![aura_core::query::DatalogFact::new(
                "message_unread",
                vec![DatalogValue::var("id")],
            )]
        };

        DatalogProgram::new(vec![DatalogRule {
            head: aura_core::query::DatalogFact::new("result", vec![DatalogValue::var("id")]),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("messages")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("message_unread")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        Ok(bindings.rows.len() as u32)
    }
}
