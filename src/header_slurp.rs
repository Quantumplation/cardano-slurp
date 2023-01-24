use std::{path::PathBuf, sync::mpsc, fs, thread::{JoinHandle, self}};

use pallas::{network::{miniprotocols::{Point, chainsync::{HeaderContent, self}}, multiplexer::StdChannel}, codec::minicbor, ledger::{primitives::{byron::{self, BlockHead}, alonzo, babbage}, traverse::ComputeHash}};


pub struct HeaderSlurp {
    pub directory: PathBuf,
    pub batch_size: u8,

    pub block_batches: mpsc::SyncSender<(Point, Point)>
}

impl HeaderSlurp {

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<byron::EbbHead>(cbor).ok()?;
        Some(Point::Specific(header.consensus_data.epoch_id * 21600, header.compute_hash().to_vec())) 
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<byron::BlockHead>(cbor).ok()?;
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
        
        let path = crate::utils::artifact_path(directory.clone(), point.clone());
        fs::create_dir_all(path.clone()).expect(format!("unable to creact directory {:?}", path).as_str());
        fs::write(path, h.cbor).expect("could not save header");
        return point;
    }

    pub fn slurp(&self, channel: StdChannel) -> JoinHandle<()> {
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