use std::path::PathBuf;

use clap::{command, Parser};
use pallas::network::miniprotocols::Point;

#[derive(Parser)]
#[command(author, version, about)]
pub struct Args {
    /// The cardano relay node to connect to
    #[arg(short, long, default_value = "relays-new.cardano-mainnet.iohk.io:3001")]
    pub relay: Vec<String>,

    /// A topology file to read for relays to connect to
    #[arg(short, long)]
    pub topology_file: Option<PathBuf>,

    #[arg(short, long, value_parser = parse_point)]
    pub fallback_point: Option<Point>,

    /// The directory to save blocks into
    #[arg(short, long, default_value = "db")]
    pub directory: PathBuf,
}

fn parse_point(s: &str) -> Result<Point, String> {
  if s == "origin" {
    Ok(Point::Origin)
  } else { 
    let parts: Vec<_> = s.split("/").collect();
    let (slot, block) = (parts[0], parts[1]);
    let (slot, block) = (slot.parse::<u64>().unwrap(), hex::decode(block).unwrap());
    Ok(Point::Specific(slot, block))
  } 
}