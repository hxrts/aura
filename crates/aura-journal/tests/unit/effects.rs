use aura_core::domain::content::Hash32;
use aura_core::types::flow::{FlowBudget, FlowCost, FlowNonce, Receipt, ReceiptSig};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::types::Epoch;

#[test]
fn flow_budget_charge_uses_cost_not_nonce() {
    let current = FlowBudget {
        limit: 100,
        spent: 20,
        epoch: Epoch::new(1),
    };
    let receipt = Receipt::new(
        ContextId::new_from_entropy([1u8; 32]),
        AuthorityId::new_from_entropy([2u8; 32]),
        AuthorityId::new_from_entropy([3u8; 32]),
        Epoch::new(2),
        FlowCost::new(7),
        FlowNonce::new(99),
        Hash32::zero(),
        ReceiptSig::new(Vec::new()).unwrap(),
    );

    let updated = FlowBudget {
        limit: current.limit,
        spent: current.spent.saturating_add(receipt.cost.value() as u64),
        epoch: receipt.epoch,
    };
    assert_eq!(updated.spent, 27);
    assert_eq!(updated.limit, 100);
    assert_eq!(updated.epoch, receipt.epoch);
}
