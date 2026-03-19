use aura_core::{OwnerPublication, OwnerEpoch, PublicationSequence};

struct FakePublication;

impl OwnerPublication for FakePublication {
    type OperationId = &'static str;
    type InstanceId = u64;
    type Trace = ();

    fn operation_id(&self) -> &Self::OperationId {
        &"fake"
    }

    fn instance_id(&self) -> &Self::InstanceId {
        &0
    }

    fn owner_epoch(&self) -> OwnerEpoch {
        OwnerEpoch::new(0)
    }

    fn publication_sequence(&self) -> PublicationSequence {
        PublicationSequence::new(0)
    }

    fn trace_context(&self) -> &Self::Trace {
        &()
    }
}

fn main() {}
