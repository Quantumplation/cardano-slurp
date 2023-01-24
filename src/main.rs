use std::{
    fs,
    path::{PathBuf}, sync::mpsc::{self, Receiver}, thread::{self, JoinHandle},
};

use clap::Parser;
use pallas::{
    codec::minicbor::{self},
    ledger::{
        primitives::{byron::{BlockHead, EbbHead, Block, EbBlock}, alonzo, babbage},
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

struct BodySlurp {
    directory: PathBuf,
}

const LARGE_BUCKET_SIZE: u64 = 200_000_000;
const SMALL_BUCKET_SIZE: u64 =     200_000;

fn artifact_path(base_directory: PathBuf, point: Point) -> PathBuf {
    let Point::Specific(slot, hash) = point else { panic!("must call artifact_subdirectory with a specific point") };

    // [Bucketing]: We store files in {directory}/{upper_bucket}/{lower_bucket}/{slot}-{hash}
    // the upper/lower bucket ensures that we don't have *too* many files per directory
    // typically you should try to avoid having more than 10k files in a directory
    // each upper bucket rolls over every 20_000_000 slots, which is around 230 days
    // each lower bucket rolls over every 200_000 slots, which is around 2 days
    // each upper bucket will have 1000 subdirectories, and each subdirectory will have on average 10k files
    let upper_bucket = format!("{}", slot - (slot % LARGE_BUCKET_SIZE));
    let lower_bucket = format!("{}", slot - (slot % SMALL_BUCKET_SIZE));
    let sub_directory = base_directory.join(upper_bucket).join(lower_bucket);
    fs::create_dir_all(sub_directory.clone()).expect(format!("unable to creact directory {:?}", sub_directory).as_str());
    let file = format!("{}-{}", slot, hex::encode(hash));
    
    sub_directory.join(file)
}

impl BodySlurp {

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, EbBlock);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(header.consensus_data.epoch_id * 21600, header.compute_hash().to_vec()))
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, Block);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(header.consensus_data.0.epoch * 21600 + header.consensus_data.0.slot, header.compute_hash().to_vec()))
    }

    fn shelley_or_alonzo_point(cbor: &Vec<u8>) -> Option<Point> {
        let block = minicbor::decode::<alonzo::Block>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn babbage_point(cbor: &Vec<u8>) -> Option<Point> {
        let block = minicbor::decode::<babbage::Block>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn handle_body(directory: &PathBuf, body: Vec<u8>) {
        let point = BodySlurp::ebb_point(&body)
            .or_else(|| BodySlurp::byron_point(&body))
            .or_else(|| BodySlurp::shelley_or_alonzo_point(&body))
            .or_else(|| BodySlurp::babbage_point(&body))
            .expect("unrecognized block");
        log::info!("downloaded block {:?} ({} bytes)", point, body.len());

        let path = artifact_path(directory.clone(), point);
        fs::write(path, body).expect("could not save body");
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

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<EbbHead>(cbor).ok()?;
        Some(Point::Specific(header.consensus_data.epoch_id * 21600, header.compute_hash().to_vec())) 
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<BlockHead>(cbor).ok()?;
        Some(Point::Specific(header.consensus_data.0.epoch * 21600 + header.consensus_data.0.slot, header.compute_hash().to_vec()))
    }

    fn shelley_or_alonzo_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<alonzo::Header>(cbor).ok()?;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn babbage_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<babbage::Header>(cbor).ok()?;
        Some(Point::Specific(header.header_body.slot, header.compute_hash().to_vec()))
    }

    fn handle_header(directory: &PathBuf, h: HeaderContent) -> Point {
        // We skip byron blocks for now, because to know their slot, we need to know the slot of the *next* block, which is annoying
        let point = 
            HeaderSlurp::ebb_point(&h.cbor)
            .or_else(|| HeaderSlurp::byron_point(&h.cbor))
            .or_else(|| HeaderSlurp::shelley_or_alonzo_point(&h.cbor))
            .or_else(|| HeaderSlurp::babbage_point(&h.cbor))
            .expect("unrecognized block");

        log::info!("rolling forward, {:?}", point);
        
        let path = artifact_path(directory.clone(), point.clone());
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
            let mut prev: Point = Point::Origin;
            loop {
                let next = client.request_next().unwrap();

                match next {
                    chainsync::NextResponse::RollForward(h, _) => {
                        let point = HeaderSlurp::handle_header(&directory, h);
                        prev = point.clone();

                        if let Point::Origin = start {
                            start = point.clone();
                        }

                        if point.slot_or_default() - start.slot_or_default() >= batch_size.into() {
                            block_batches.send((start, point.clone())).expect("unable to send block batch");
                            start = point.clone();
                        }
                    },
                    chainsync::NextResponse::RollBackward(rollback_to, _) => {
                        log::info!("rollback to {:?}", rollback_to);
                        // Make sure we download these block ranges before rolling back
                        if start != prev && start != Point::Origin {
                            block_batches.send((start.clone(), prev.clone())).expect("unable to send block batch before rollback");
                        }
                        // Rolling back in our dumb client is easy :)
                        start = rollback_to.clone();
                        prev = rollback_to.clone();
                    },
                    chainsync::NextResponse::Await => log::info!("tip of chaing reached"),
                };
            }
        })
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
