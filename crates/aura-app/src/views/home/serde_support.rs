use super::state::HomeState;
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use std::collections::HashMap;

pub(super) mod channel_id_keyed_map {
    use super::{ChannelId, HashMap, HomeState};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        map: &HashMap<ChannelId, HomeState>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let keyed: HashMap<String, &HomeState> = map
            .iter()
            .map(|(home_id, home)| (home_id.to_string(), home))
            .collect();
        keyed.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<ChannelId, HomeState>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let keyed = HashMap::<String, HomeState>::deserialize(deserializer)?;
        keyed
            .into_iter()
            .map(|(home_id, home)| {
                let parsed = home_id
                    .parse::<ChannelId>()
                    .map_err(serde::de::Error::custom)?;
                if parsed != home.id {
                    return Err(serde::de::Error::custom(
                        "home map key differs from home ID",
                    ));
                }
                Ok((parsed, home))
            })
            .collect()
    }
}

pub(super) mod authority_id_keyed_map {
    use super::{AuthorityId, HashMap};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S, V>(map: &HashMap<AuthorityId, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        V: Serialize,
    {
        let keyed: HashMap<String, &V> = map
            .iter()
            .map(|(authority_id, value)| (authority_id.to_string(), value))
            .collect();
        keyed.serialize(serializer)
    }

    pub fn deserialize<'de, D, V>(deserializer: D) -> Result<HashMap<AuthorityId, V>, D::Error>
    where
        D: Deserializer<'de>,
        V: Deserialize<'de>,
    {
        let keyed = HashMap::<String, V>::deserialize(deserializer)?;
        keyed
            .into_iter()
            .map(|(authority_id, value)| {
                let parsed = authority_id
                    .parse::<AuthorityId>()
                    .map_err(serde::de::Error::custom)?;
                Ok((parsed, value))
            })
            .collect()
    }
}
