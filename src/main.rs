use std::{
    fs,
    path::{PathBuf}, sync::mpsc::{self, Receiver}, thread::{self, JoinHandle},
};

use clap::Parser;
use pallas::{
    codec::minicbor::{self, decode},
    crypto::hash::{Hasher, Hash},
    ledger::{
        primitives::{byron::{BlockHead, EbbHead}, alonzo, babbage},
        traverse::ComputeHash,
    },
    network::{
        miniprotocols::{
            blockfetch,
            chainsync::{self, HeaderContent},
            handshake, Point, MAINNET_MAGIC,
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

struct BodySlurp {
    directory: PathBuf,
}

impl BodySlurp {
    fn handle_body(directory: &PathBuf, body: Vec<u8>) {
        log::info!("received block of size: {}", body.len())
    }

    fn slurp(&self, channel: StdChannel, block_batches: Receiver<(Point, Point)>) -> JoinHandle<()> {
        fs::create_dir_all(&self.directory).expect("couldn't create directory");

        let directory = self.directory.clone();
        thread::spawn(move || {
            let mut client = blockfetch::Client::new(channel);
            loop {
                let next_range = block_batches.recv().expect("failed to receive next block range");
                let blocks = client.fetch_range(next_range).expect("unable to query block range");
                for block in blocks {
                    BodySlurp::handle_body(&directory, block);
                }
            }
        })
    }
}

struct HeaderSlurp {
    directory: PathBuf,
    batch_size: u8,

    block_batches: mpsc::SyncSender<(Point, Point)>
}

impl HeaderSlurp {

    fn ebb_hash_and_slot(height: u64, cbor: &Vec<u8>) -> Option<Point> {
        Some(Point::Specific(height, minicbor::decode::<EbbHead>(cbor).ok()?.compute_hash().to_vec()))
    }

    fn byron_hash_and_slot(height: u64, cbor: &Vec<u8>) -> Option<Point> {
        Some(Point::Specific(height, minicbor::decode::<BlockHead>(cbor).ok()?.compute_hash().to_vec()))
    }

    fn shelley_or_alonzo_hash_and_slot(_: u64, cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<alonzo::Header>(cbor).ok()?;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn babbage_hash_and_slot(_: u64, cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<babbage::Header>(cbor).ok()?;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn handle_header(directory: &PathBuf, height: u64, h: HeaderContent) -> Point {
        // We skip byron blocks for now, because to know their slot, we need to know the slot of the *next* block, which is annoying
        let point = 
            HeaderSlurp::ebb_hash_and_slot(height, &h.cbor)
            .or_else(|| HeaderSlurp::byron_hash_and_slot(height, &h.cbor))
            .or_else(|| HeaderSlurp::shelley_or_alonzo_hash_and_slot(height, &h.cbor))
            .or_else(|| HeaderSlurp::babbage_hash_and_slot(height, &h.cbor))
            .expect("unrecognized block");

        log::info!("rolling forward, {:?}", point);
        
        // This is guaranteed by the methods above
        let Point::Specific(slot, hash) = point.clone() else { unreachable!("should be guaranteed by the methods above") };
        let file = format!("{}-{}", slot, hex::encode(hash));
        let path = directory.join(file);
        fs::write(path, h.cbor).expect("could not save header");
        return point;
    }

    fn slurp(&self, channel: StdChannel) -> JoinHandle<()> {
        fs::create_dir_all(&self.directory).expect("couldn't create directory");

        let known_points = vec![Point::Origin];

        let mut client = chainsync::N2NClient::new(channel);

        let (point, _) = client.find_intersect(known_points).unwrap();

        log::info!("intersected point is {:?}", point);

        let directory = self.directory.clone();
        let batch_size = self.batch_size;
        let block_batches = self.block_batches.clone();
        thread::spawn(move || {
            let mut start: Point = Point::Origin;
            for height in 0.. {
                let next = client.request_next().unwrap();

                match next {
                    chainsync::NextResponse::RollForward(h, _) => {
                        let point = HeaderSlurp::handle_header(&directory, height, h);

                        if point.slot_or_default() - start.slot_or_default() > batch_size.into() {
                            block_batches.send((start, point.clone())).expect("unable to send block batch");
                            start = point.clone();
                        } 
                    },
                    chainsync::NextResponse::RollBackward(x, _) => log::info!("rollback to {:?}", x),
                    chainsync::NextResponse::Await => log::info!("tip of chaing reached"),
                };
            }
        })
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

    // execute the required handshake against the relay
    do_handshake(channel0);

    let (sender, receiver) = mpsc::sync_channel(10);

    let headers = HeaderSlurp { directory: args.directory.join("headers"), batch_size: 20, block_batches: sender };
    let bodies = BodySlurp { directory: args.directory.join("bodies") };

    // execute the chainsync flow from an arbitrary point in the chain
    let headers = headers.slurp(channel2);
    let bodies = bodies.slurp(channel3, receiver);

    headers.join().expect("error while slurping headers");
    bodies.join().expect("error while slurping bodies");
}
