use bpaf::{construct, long, Args, Parser};
use std::path::PathBuf;

/// Arguments accepted by the `Status` subcommand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusArgs {
    /// Optional config file passed via `--config`
    pub config: Option<PathBuf>,
}

pub fn status_parser() -> impl Parser<StatusArgs> {
    construct!(StatusArgs {
        config: long("config")
            .short('c')
            .help("Path to the config file")
            .argument::<PathBuf>("CONFIG")
            .optional(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config() {
        let parser = status_parser();
        let args = Args::from(&["--config", "foo.toml"]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(parsed.config, Some(PathBuf::from("foo.toml")));
    }

    #[test]
    fn accepts_no_config() {
        let parser = status_parser();
        let args = Args::from(&[]);
        let parsed = parser.run_inner(args).unwrap();
        assert_eq!(parsed.config, None);
    }
}
