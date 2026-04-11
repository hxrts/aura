mod checks;

use std::{env, process};

use anyhow::{bail, Result};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    match args.as_slice() {
        [command, check, rest @ ..] if command == "check" => checks::run(check, rest),
        _ => bail!(
            "usage: cargo run --manifest-path policy/xtask/Cargo.toml -- check <name> [args...]"
        ),
    }
}
