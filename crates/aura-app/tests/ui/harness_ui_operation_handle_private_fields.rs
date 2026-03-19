use aura_app::ui_contract::{HarnessUiOperationHandle, OperationId, OperationInstanceId};

fn main() {
    let _handle = HarnessUiOperationHandle {
        operation_id: OperationId::send_message(),
        instance_id: OperationInstanceId("instance-1".to_string()),
    };
}
