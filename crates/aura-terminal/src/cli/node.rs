use bpaf::{construct, long, Parser};

#[derive(Clone, Debug, PartialEq)]
pub struct NodeArgs {
    pub port: Option<u16>,
    pub daemon: bool,
    pub config: Option<std::path::PathBuf>,
}

#[must_use]
pub fn node_parser() -> impl Parser<NodeArgs> {
    let port = long("port")
        .help("Port to listen on")
        .argument::<u16>("PORT")
        .optional();
    let daemon = long("daemon").help("Run as daemon").switch();
    let config = long("config")
        .short('c')
        .help("Config file path")
        .argument::<std::path::PathBuf>("CONFIG")
        .optional();
    construct!(NodeArgs {
        port,
        daemon,
        config
    })
}
