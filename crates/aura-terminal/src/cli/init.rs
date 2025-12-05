use bpaf::{construct, long, Parser};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitArgs {
    /// Number of devices (short `-n`)
    pub num_devices: u32,
    /// Threshold (short `-t`)
    pub threshold: u32,
    /// Output folder
    pub output: PathBuf,
}

pub fn init_parser() -> impl Parser<InitArgs> {
    let num_devices = long("num-devices")
        .short('n')
        .help("Number of devices in the threshold group")
        .argument::<u32>("NUM_DEVICES")
        .fallback(1);
    let threshold = long("threshold")
        .short('t')
        .help("Threshold (minimum devices required)")
        .argument::<u32>("THRESHOLD")
        .fallback(1);
    let output = long("output")
        .short('o')
        .help("Directory to store generated configs")
        .argument::<PathBuf>("DIR");
    construct!(InitArgs {
        num_devices,
        threshold,
        output
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpaf::Args;

    #[test]
    fn parses_init_args() {
        let parser = init_parser().to_options();
        let args = Args::from(&["--num-devices", "3", "--threshold", "2", "--output", "out"]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(
            parsed,
            InitArgs {
                num_devices: 3,
                threshold: 2,
                output: PathBuf::from("out"),
            }
        );
    }

    #[test]
    fn uses_defaults() {
        let parser = init_parser().to_options();
        let args = Args::from(&["--output", "every"]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(parsed.num_devices, 1);
        assert_eq!(parsed.threshold, 1);
        assert_eq!(parsed.output, PathBuf::from("every"));
    }
}
