use std::path::PathBuf;

use clap::{command, Parser};

#[derive(Parser)]
#[command(author, version, about)]
pub struct Args {
    /// The cardano relay node to connect to
    #[arg(short, long, default_value = "relays-new.cardano-mainnet.iohk.io:3001")]
    pub relay: Vec<String>,

    /// The directory to save blocks into
    #[arg(short, long, default_value = "blocks")]
    pub directory: PathBuf,
}
