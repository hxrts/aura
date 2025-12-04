use bpaf::{construct, long, Parser};
use std::path::PathBuf;

/// Arguments accepted by the `Status` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusArgs {
    /// Optional config file passed via `--config`
    pub config: Option<PathBuf>,
}

pub fn status_parser() -> impl Parser<StatusArgs> {
    let config = long("config")
        .short('c')
        .help("Path to the config file")
        .argument::<PathBuf>("CONFIG")
        .optional();
    construct!(StatusArgs { config })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpaf::Args;

    #[test]
    fn parses_config() {
        let parser = status_parser().to_options();
        let args = Args::from(&["--config", "foo.toml"]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(parsed.config, Some(PathBuf::from("foo.toml")));
    }

    #[test]
    fn accepts_no_config() {
        let parser = status_parser().to_options();
        let args = Args::from(&[]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(parsed.config, None);
    }
}
