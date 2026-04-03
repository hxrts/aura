use aura_agent::reactive::app_signal_projection::map_channel_metadata;
use aura_invitation::InvitationType;

fn main() {
    let _ = map_channel_metadata(&InvitationType::Contact { nickname: None });
}
