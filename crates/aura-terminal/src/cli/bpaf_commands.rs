use bpaf::{construct, command, long, short, Parser};
use std::path::PathBuf;

use crate::cli::bpaf_init::{InitArgs, init_parser};
use crate::cli::bpaf_node::{NodeArgs, node_parser};
use crate::cli::bpaf_status::{StatusArgs, status_parser};

/// Top-level CLI commands exposed to the terminal.
#[derive(Debug, Clone)]
pub enum Commands {
    Init(InitArgs),
    Status(StatusArgs),
    Node(NodeArgs),
    Version,
}

fn init_command() -> impl Parser<Commands> {
    command("init", init_parser().map(Commands::Init))
        .help("Initialize a new threshold account")
}

fn status_command() -> impl Parser<Commands> {
    command("status", status_parser().map(Commands::Status))
        .help("Show account status")
}

fn node_command() -> impl Parser<Commands> {
    command("node", node_parser().map(Commands::Node)).help("Run node/agent daemon")
}

fn commands_parser() -> impl Parser<Commands> {
    construct!([init_command(), status_command(), node_command()])
}

#[derive(Debug, Clone)]
pub struct GlobalArgs {
    pub verbose: bool,
    pub config: Option<PathBuf>,
    pub command: Commands,
}

pub fn cli_parser() -> impl Parser<GlobalArgs> {
    construct!(GlobalArgs {
        verbose: short('v')
            .help("Enable verbose logging")
            .switch(),
        config: long("config")
            .short('c')
            .help("Global config file")
            .argument::<PathBuf>("CONFIG")
            .optional(),
        command: commands_parser(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpaf::Args;

    #[test]
    fn parses_init_command() {
        let parser = commands_parser();
        let args = Args::from(&["init", "--output", "out"]);
        let parsed = parser.run_inner(args).unwrap();
        match parsed {
            Commands::Init(init) => assert_eq!(init.output, PathBuf::from("out")),
            _ => panic!("expected Init"),
        }
    }

    #[test]
    fn parses_status_command() {
        let parser = commands_parser();
        let args = Args::from(&["status"]);
        let parsed = parser.run_inner(args).unwrap();
        if let Commands::Status(status) = parsed {
            assert!(status.config.is_none());
        } else {
            panic!("expected Status");
        }
    }
}
