use bpaf::{construct, Parser};

#[derive(Copy, Clone, Debug)]
pub enum SnapshotCommand {
    Propose,
}

pub fn snapshot_parser() -> impl Parser<SnapshotCommand> {
    construct!(SnapshotCommand {
        Propose: construct!(())
            .switch()
            .to_options()
            .help("Propose a snapshot")
            .command("propose"),
    })
}
