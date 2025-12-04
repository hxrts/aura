use bpaf::{construct, long, Parser};

#[derive(Clone, Debug, PartialEq)]
pub struct NodeArgs {
    pub port: Option<u16>,
    pub daemon: bool,
    pub config: Option<std::path::PathBuf>,
}

pub fn node_parser() -> impl Parser<NodeArgs> {
    construct!(NodeArgs {
        port: long("port")
            .help("Port to listen on")
            .argument::<u16>("PORT")
            .optional(),
        daemon: long("daemon")
            .help("Run as daemon")
            .switch(),
        config: long("config")
            .short('c')
            .help("Config file path")
            .argument::<std::path::PathBuf>("CONFIG")
            .optional(),
    })
}
