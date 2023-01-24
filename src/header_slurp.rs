use std::{
    fs,
    path::PathBuf,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use pallas::{
    codec::minicbor,
    ledger::{
        primitives::{alonzo, babbage, byron},
        traverse::ComputeHash,
    },
    network::{
        miniprotocols::{
            chainsync::{self, HeaderContent},
            Point,
        },
        multiplexer::StdChannel,
    },
};

pub struct HeaderSlurp {
    pub directory: PathBuf,
    pub batch_size: u8,

    pub block_batches: mpsc::SyncSender<(Point, Point)>,

    relay: String,
    join_handle: Option<JoinHandle<()>>,
}

impl HeaderSlurp {
    pub fn new(
        relay: String,
        directory: PathBuf,
        batch_size: u8,
        block_batches: mpsc::SyncSender<(Point, Point)>,
    ) -> Self {
        Self {
            directory,
            relay,
            batch_size,
            block_batches,
            join_handle: None,
        }
    }

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<byron::EbbHead>(cbor).ok()?;
        Some(Point::Specific(
            header.consensus_data.epoch_id * 21600,
            header.compute_hash().to_vec(),
        ))
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<byron::BlockHead>(cbor).ok()?;
        Some(Point::Specific(
            header.consensus_data.0.epoch * 21600 + header.consensus_data.0.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn shelley_or_alonzo_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<alonzo::Header>(cbor).ok()?;
        Some(Point::Specific(
            header.header_body.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn babbage_point(cbor: &Vec<u8>) -> Option<Point> {
        let header = minicbor::decode::<babbage::Header>(cbor).ok()?;
        Some(Point::Specific(
            header.header_body.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn handle_header(relay: &String, directory: &PathBuf, h: HeaderContent) -> Point {
        // We skip byron blocks for now, because to know their slot, we need to know the slot of the *next* block, which is annoying
        let point = HeaderSlurp::ebb_point(&h.cbor)
            .or_else(|| HeaderSlurp::byron_point(&h.cbor))
            .or_else(|| HeaderSlurp::shelley_or_alonzo_point(&h.cbor))
            .or_else(|| HeaderSlurp::babbage_point(&h.cbor))
            .expect("unrecognized block header");

        log::info!(target: &relay[..11], "rolling forward, {:?}", point);

        let path = crate::utils::artifact_path(directory.clone(), point.clone());
        fs::create_dir_all(path.parent().unwrap())
            .expect(format!("unable to creact directory {:?}", path).as_str());
        fs::write(path, h.cbor).expect("could not save header");
        return point;
    }

    pub fn slurp(&mut self, channel: StdChannel) {
        fs::create_dir_all(&self.directory).expect("couldn't create directory");

        let known_points = vec![Point::Specific(82992205, hex::decode("fb2e8693657a73be00027e71d15c37cafc14a79e72d08b5506295dbde29c8f38").unwrap())];

        let mut client = chainsync::N2NClient::new(channel);

        let (point, _) = client.find_intersect(known_points).unwrap();

        let directory = self.directory.clone();
        let relay = self.relay.clone();
        let mut batch_size = self.batch_size;
        let block_batches = self.block_batches.clone();

        log::info!(target: &relay[..11], "intersected point is {:?}", point);

        self.join_handle = Some(thread::spawn(move || {
            let mut start: Option<Point> = None;
            let mut prev: Option<Point> = None;
            let mut current_batch = 0;
            loop {
                let next = if client.has_agency() {
                  client.request_next().unwrap()
                } else {
                  client.recv_while_can_await().unwrap()
                };

                match next {
                    chainsync::NextResponse::RollForward(h, _) => {
                        let point = HeaderSlurp::handle_header(&relay, &directory, h);
                        
                        if start.is_none() {
                            start = Some(point.clone());
                            current_batch = 1;
                        } else {
                            current_batch += 1;
                        }

                        let s = start.clone().unwrap_or(point.clone());
                        // (start, point) 
                        if current_batch >= batch_size.into() {
                            block_batches
                                .send((s, point.clone()))
                                .expect("unable to send block batch");
                            start = None;
                            current_batch = 0;
                        }
                        prev = Some(point.clone());
                    }
                    chainsync::NextResponse::RollBackward(rollback_to, _) => {
                        log::info!(target: &relay[..11], "rollback to {:?}", rollback_to);
                        // Make sure we download these block ranges before rolling back
                        // If we have a start point and a previous point, make sure to download the blocks in that range before we roll back
                        match (&start, &prev) {
                            (Some(s), Some(p)) => {
                                block_batches
                                    .send((s.clone(), p.clone()))
                                    .expect("unable to send block batch before rollback");
                            },
                            _ => {}
                        }
                        // And then set start to none, since we've already downloaded rollback_to (in theory)
                        start = None;
                    }
                    chainsync::NextResponse::Await => {
                        if batch_size > 1 {
                            log::info!(target: &relay[..11], "tip of chain reached");
                            batch_size = 1;
                        }
                    }
                };
            }
        }));
    }

    pub fn join(&mut self) -> thread::Result<()> {
        match self.join_handle.take() {
            Some(jh) => jh.join(),
            None => Ok(()),
        }
    }
}
