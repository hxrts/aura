use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!(
            "usage: cargo run -p aura-testkit --example protocol_compat -- <baseline.choreo> <current.choreo>"
        );
        std::process::exit(2);
    }

    let baseline_path = &args[1];
    let current_path = &args[2];

    let baseline = match fs::read_to_string(baseline_path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("failed to read baseline `{baseline_path}`: {err}");
            std::process::exit(2);
        }
    };

    let current = match fs::read_to_string(current_path) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("failed to read current `{current_path}`: {err}");
            std::process::exit(2);
        }
    };

    match aura_testkit::check_async_subtype_for_shared_roles(&baseline, &current) {
        Ok(()) => {
            println!(
                "compatible: `{current_path}` is async-subtype compatible with `{baseline_path}`"
            );
        }
        Err(err) => {
            eprintln!("incompatible: {err}");
            std::process::exit(1);
        }
    }
}
