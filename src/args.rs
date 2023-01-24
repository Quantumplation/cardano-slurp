use std::path::PathBuf;

use clap::{command, Parser};

#[derive(Parser)]
#[command(author, version, about)]
pub struct Args {
    /// The cardano relay node to connect to
    #[arg(short, long, default_value = "relays-new.cardano-mainnet.iohk.io:3001")]
    pub relay: Vec<String>,

    /// A topology file to read for relays to connect to
    #[arg(short, long)]
    pub topology_file: Option<PathBuf>,

    /// The directory to save blocks into
    #[arg(short, long, default_value = "db")]
    pub directory: PathBuf,
}
