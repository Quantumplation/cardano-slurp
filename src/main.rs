use std::{
    fmt, fs,
    path::{self, Path, PathBuf},
};

use clap::Parser;
use pallas::{
    codec::minicbor::{self, decode, encode::Error},
    crypto::hash::{Hash, Hasher},
    ledger::{
        primitives::byron::{BlockHead, EbbCons, EbbHead},
        traverse::ComputeHash,
    },
    network::{
        miniprotocols::{
            blockfetch,
            chainsync::{self, HeaderContent},
            handshake, txmonitor, Point, MAINNET_MAGIC,
        },
        multiplexer::{bearers::Bearer, StdChannel, StdPlexer},
    },
};

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

fn do_blockfetch(channel: StdChannel) {
    let range = (
        Point::Specific(
            43847831,
            hex::decode("15b9eeee849dd6386d3770b0745e0450190f7560e5159b1b3ab13b14b2684a45")
                .unwrap(),
        ),
        Point::Specific(
            43847844,
            hex::decode("ff8d558a3d5a0e058beb3d94d26a567f75cd7d09ff5485aa0d0ebc38b61378d4")
                .unwrap(),
        ),
    );

    let mut client = blockfetch::Client::new(channel);

    let blocks = client.fetch_range(range).unwrap();

    for block in blocks {
        log::info!("received block of size: {}", block.len());
    }
}

struct Slurp {
    directory: PathBuf,
}

impl Slurp {
    fn handle_header(&self, height: u32, h: HeaderContent) {
        let ebb_hash =
            minicbor::decode::<EbbHead>(&h.cbor).and_then(|b| Ok(b.compute_hash()));
        let byron_block_hash = ebb_hash.or_else(|_| {
            minicbor::decode::<BlockHead>(&h.cbor).and_then(|b| Ok(b.compute_hash()))
        });
        let modern_hash = byron_block_hash
            .or_else(|_| Ok::<_, decode::Error>(Hasher::<256>::hash(&h.cbor)));

        let hash = modern_hash.expect("couldn't compute hash");

        log::info!("rolling forward, {}", hash);

        let file =
            self.directory
                .join("headers")
                .join(format!("{}-{}", height, hex::encode(hash)));
        fs::write(file, h.cbor).expect("could not save header");
    }

    fn do_chainsync(&self, channel: StdChannel) {
        let known_points = vec![Point::Origin];

        let mut client = chainsync::N2NClient::new(channel);

        let (point, _) = client.find_intersect(known_points).unwrap();

        log::info!("intersected point is {:?}", point);

        for height in 0.. {
            let next = client.request_next().unwrap();

            match next {
                chainsync::NextResponse::RollForward(h, _) => self.handle_header(height, h),
                chainsync::NextResponse::RollBackward(x, _) => log::info!("rollback to {:?}", x),
                chainsync::NextResponse::Await => log::info!("tip of chaing reached"),
            };
        }
    }
}

#[derive(Parser)]
#[command(author, version, about)]
struct Args {
    // The cardano relay node to connect to
    #[arg(default_value = "relays-new.cardano-mainnet.iohk.io:3001")]
    relay: String,

    // The directory to save blocks into
    #[arg(default_value = "blocks")]
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

    fs::create_dir_all(&args.directory).expect("could not create target directory");
    fs::create_dir_all(&args.directory.join("headers")).expect("could not create target directory");

    // execute the required handshake against the relay
    do_handshake(channel0);

    // fetch an arbitrary batch of block
    do_blockfetch(channel3);

    let slurp = Slurp { directory: args.directory };

    // execute the chainsync flow from an arbitrary point in the chain
    slurp.do_chainsync(channel2);
}
