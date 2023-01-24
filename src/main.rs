use std::{
    path::{PathBuf}, sync::mpsc::{self},
};

use body_slurp::BodySlurp;
use clap::Parser;
use header_slurp::HeaderSlurp;
use pallas::{
    network::{
        miniprotocols::{
            handshake, MAINNET_MAGIC,
        },
        multiplexer::{bearers::Bearer, StdChannel, StdPlexer},
    },
};

mod utils;
mod body_slurp;
mod header_slurp;

fn do_handshake(channel: StdChannel) {
    let mut client = handshake::N2NClient::new(channel);

    let confirmation = client
        .handshake(handshake::n2n::VersionTable::v7_and_above(MAINNET_MAGIC))
        .unwrap();

    match confirmation {
        handshake::Confirmation::Accepted(v, _) => {
            log::info!("hand-shake accepted, using version {}", v)
        }
        handshake::Confirmation::Rejected(x) => {
            log::info!("hand-shake rejected with reason {:?}", x)
        }
    }
}

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    /// The cardano relay node to connect to
    #[arg(short, long, default_value = "relays-new.cardano-mainnet.iohk.io:3001")]
    relay: String,

    /// The directory to save blocks into
    #[arg(short, long, default_value = "blocks")]
    directory: PathBuf,
}

fn main() {
    let args = Args::parse();

    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    // setup a TCP socket to act as data bearer between our agents and the remote
    // relay.
    let bearer = Bearer::connect_tcp(args.relay).unwrap();

    // setup the multiplexer by specifying the bearer and the IDs of the
    // miniprotocols to use
    let mut plexer = StdPlexer::new(bearer);
    let channel0 = plexer.use_channel(0);
    let channel2 = plexer.use_channel(2);
    let channel3 = plexer.use_channel(3);

    plexer.muxer.spawn();
    plexer.demuxer.spawn();

    // execute the required handshake against the relay
    do_handshake(channel0);

    let (sender, receiver) = mpsc::sync_channel(10);

    let headers = HeaderSlurp { directory: args.directory.join("headers"), batch_size: 5, block_batches: sender };
    let bodies = BodySlurp { directory: args.directory.join("bodies") };

    // execute the chainsync flow from an arbitrary point in the chain
    let headers = headers.slurp(channel2);
    let bodies = bodies.slurp(channel3, receiver);

    headers.join().expect("error while slurping headers");
    bodies.join().expect("error while slurping bodies");
}
