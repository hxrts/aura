use super::models::Channel;
use aura_core::types::identifiers::ChannelId;
use std::collections::HashMap;

pub(super) mod channel_id_keyed_map {
    use super::{Channel, ChannelId, HashMap};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(map: &HashMap<ChannelId, Channel>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let keyed: HashMap<String, &Channel> = map
            .iter()
            .map(|(channel_id, channel)| (channel_id.to_string(), channel))
            .collect();
        keyed.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<ChannelId, Channel>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let keyed = HashMap::<String, Channel>::deserialize(deserializer)?;
        keyed
            .into_iter()
            .map(|(channel_id, channel)| {
                let parsed = channel_id
                    .parse::<ChannelId>()
                    .map_err(serde::de::Error::custom)?;
                Ok((parsed, channel))
            })
            .collect()
    }
}
