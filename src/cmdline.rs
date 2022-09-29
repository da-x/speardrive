use std::{path::PathBuf};
use structopt::StructOpt;

#[derive(Debug, StructOpt, Clone)]
pub struct CommandArgs {
    #[structopt(long = "config-path", short = "c")]
    pub config: Option<PathBuf>,

    #[structopt(long)]
    pub dump_config: bool,

    #[structopt(subcommand)]
    pub cmd: Command,

    #[structopt(flatten)]
    pub logging: crate::logging::Opt,
}

#[derive(Debug, StructOpt, Clone)]
pub enum Command {
    ExampleConf,
    Serve,
}
