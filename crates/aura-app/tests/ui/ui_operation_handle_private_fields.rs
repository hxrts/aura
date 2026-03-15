use aura_app::scenario_contract::UiOperationHandle;
use aura_app::ui::contract::{OperationId, OperationInstanceId};

fn main() {
    let _handle = UiOperationHandle {
        id: OperationId::send_message(),
        instance_id: OperationInstanceId("instance-1".to_string()),
    };
}
