use super::super::common::{get_bool, get_channel_id, get_int, get_string};
use aura_core::query::{
    DatalogBindings, DatalogFact, DatalogProgram, DatalogRule, DatalogValue, FactPredicate, Query,
    QueryAccessPolicy, QueryCapability, QueryParseError,
};
use aura_core::types::ChannelId;
use serde::{Deserialize, Serialize};

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

    fn access_policy(&self) -> QueryAccessPolicy {
        QueryAccessPolicy::protected(QueryCapability::read("neighborhood"))
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
