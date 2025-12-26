use aura_guards::types::validate_charge_before_send;
use proptest::prelude::*;

#[derive(Debug, Clone)]
enum Cmd {
    Charge,
    Send,
    Other,
}

fn cmd_from_u8(value: u8) -> Cmd {
    match value % 3 {
        0 => Cmd::Charge,
        1 => Cmd::Send,
        _ => Cmd::Other,
    }
}

proptest! {
    #[test]
    fn charge_before_send_invariant(raw_cmds in proptest::collection::vec(any::<u8>(), 0..64)) {
        let cmds: Vec<Cmd> = raw_cmds.into_iter().map(cmd_from_u8).collect();

        let result = validate_charge_before_send(
            &cmds,
            |c| matches!(c, Cmd::Charge),
            |c| matches!(c, Cmd::Send),
        );

        if result.is_ok() {
            let mut saw_charge = false;
            for cmd in &cmds {
                if matches!(cmd, Cmd::Charge) {
                    saw_charge = true;
                }
                if matches!(cmd, Cmd::Send) {
                    prop_assert!(saw_charge, "send observed without prior charge");
                }
            }
        }
    }
}
