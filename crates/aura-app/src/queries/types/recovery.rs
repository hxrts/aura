//! Recovery-oriented query types.

use super::common::{get_authority_id, get_int, get_string};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryCapability, QueryParseError,
};
use aura_core::types::AuthorityId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct GuardiansQuery {
    pub status: Option<String>,
    pub include_revoked: bool,
}

impl Query for GuardiansQuery {
    type Result = Vec<crate::views::recovery::Guardian>;

    fn to_datalog(&self) -> DatalogProgram {
        let mut body = vec![DatalogFact::new(
            "guardian",
            vec![
                DatalogValue::var("id"),
                DatalogValue::var("name"),
                DatalogValue::var("status"),
                DatalogValue::var("created_at"),
            ],
        )];

        if let Some(status) = &self.status {
            body.push(DatalogFact::new(
                "eq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String(status.clone()),
                ],
            ));
        }

        if !self.include_revoked {
            body.push(DatalogFact::new(
                "neq",
                vec![
                    DatalogValue::var("status"),
                    DatalogValue::String("revoked".to_string()),
                ],
            ));
        }

        DatalogProgram::new(vec![DatalogRule {
            head: DatalogFact::new(
                "result",
                vec![
                    DatalogValue::var("id"),
                    DatalogValue::var("name"),
                    DatalogValue::var("status"),
                    DatalogValue::var("created_at"),
                ],
            ),
            body,
        }])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("guardians")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![FactPredicate::new("guardian")]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::recovery::{Guardian, GuardianStatus};

        bindings
            .rows
            .into_iter()
            .map(|row| {
                let status = match get_string(&row, "status").as_str() {
                    "active" => GuardianStatus::Active,
                    "pending" => GuardianStatus::Pending,
                    "revoked" => GuardianStatus::Revoked,
                    "offline" => GuardianStatus::Offline,
                    _ => GuardianStatus::Pending,
                };

                Ok(Guardian {
                    id: get_authority_id(&row, "id")?,
                    name: get_string(&row, "name"),
                    status,
                    added_at: get_int(&row, "added_at") as u64,
                    last_seen: {
                        let timestamp = get_int(&row, "last_seen");
                        (timestamp > 0).then_some(timestamp as u64)
                    },
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct RecoveryQuery;

impl Query for RecoveryQuery {
    type Result = crate::views::recovery::RecoveryState;

    fn to_datalog(&self) -> DatalogProgram {
        DatalogProgram::new(vec![
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                        DatalogValue::var("contact_id"),
                        DatalogValue::var("contact_nickname"),
                        DatalogValue::var("contact_suggested"),
                        DatalogValue::var("contact_last_interaction"),
                    ],
                ),
                body: vec![
                    DatalogFact::new(
                        "recovery_config",
                        vec![
                            DatalogValue::var("threshold"),
                            DatalogValue::var("guardian_count"),
                        ],
                    ),
                    DatalogFact::new(
                        "contact",
                        vec![
                            DatalogValue::var("contact_id"),
                            DatalogValue::var("contact_nickname"),
                            DatalogValue::var("contact_suggested"),
                            DatalogValue::var("is_guardian"),
                            DatalogValue::var("is_member"),
                            DatalogValue::var("contact_last_interaction"),
                            DatalogValue::var("is_online"),
                        ],
                    ),
                    DatalogFact::new(
                        "eq",
                        vec![
                            DatalogValue::var("is_guardian"),
                            DatalogValue::Boolean(true),
                        ],
                    ),
                ],
            },
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::Integer(0),
                        DatalogValue::Integer(0),
                        DatalogValue::var("contact_id"),
                        DatalogValue::var("contact_nickname"),
                        DatalogValue::var("contact_suggested"),
                        DatalogValue::var("contact_last_interaction"),
                    ],
                ),
                body: vec![
                    DatalogFact::new(
                        "contact",
                        vec![
                            DatalogValue::var("contact_id"),
                            DatalogValue::var("contact_nickname"),
                            DatalogValue::var("contact_suggested"),
                            DatalogValue::var("is_guardian"),
                            DatalogValue::var("is_member"),
                            DatalogValue::var("contact_last_interaction"),
                            DatalogValue::var("is_online"),
                        ],
                    ),
                    DatalogFact::new(
                        "eq",
                        vec![
                            DatalogValue::var("is_guardian"),
                            DatalogValue::Boolean(true),
                        ],
                    ),
                ],
            },
            DatalogRule {
                head: DatalogFact::new(
                    "result",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                        DatalogValue::String(String::new()),
                        DatalogValue::String(String::new()),
                        DatalogValue::String(String::new()),
                        DatalogValue::Integer(0),
                    ],
                ),
                body: vec![DatalogFact::new(
                    "recovery_config",
                    vec![
                        DatalogValue::var("threshold"),
                        DatalogValue::var("guardian_count"),
                    ],
                )],
            },
        ])
    }

    fn required_capabilities(&self) -> Vec<QueryCapability> {
        vec![QueryCapability::read("recovery")]
    }

    fn dependencies(&self) -> Vec<FactPredicate> {
        vec![
            FactPredicate::new("recovery_config"),
            FactPredicate::new("contact"),
            FactPredicate::new("recovery_process"),
        ]
    }

    fn parse(bindings: DatalogBindings) -> Result<Self::Result, QueryParseError> {
        use crate::views::recovery::{Guardian, GuardianStatus, RecoveryState};

        if bindings.rows.is_empty() {
            return Ok(RecoveryState::default());
        }

        let mut guardians_by_id = std::collections::HashMap::<AuthorityId, Guardian>::new();
        let mut threshold = 0u32;

        for row in &bindings.rows {
            if threshold == 0 {
                threshold = get_int(row, "threshold") as u32;
            }

            let guardian_id = get_authority_id(row, "contact_id")?;
            let nickname = get_string(row, "contact_nickname");
            let suggested = get_string(row, "contact_suggested");
            let name = if !nickname.is_empty() {
                nickname
            } else if !suggested.is_empty() {
                suggested
            } else {
                guardian_id.to_string()
            };

            guardians_by_id.entry(guardian_id).or_insert(Guardian {
                id: guardian_id,
                name,
                status: GuardianStatus::Active,
                added_at: get_int(row, "contact_last_interaction") as u64,
                last_seen: {
                    let timestamp = get_int(row, "contact_last_interaction");
                    (timestamp > 0).then_some(timestamp as u64)
                },
            });
        }

        Ok(RecoveryState::from_parts(
            guardians_by_id.into_values(),
            threshold,
            None,
            Vec::new(),
            Vec::new(),
        ))
    }
}
