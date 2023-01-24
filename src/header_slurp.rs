use std::{
    fs,
    path::{PathBuf, Path},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};

use anyhow::Context;
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

use crate::cursor::Cursor;

pub struct HeaderSlurp {
    pub base_directory: PathBuf,
    pub directory: PathBuf,
    pub batch_size: u8,

    pub block_batches: mpsc::SyncSender<(Point, Point)>,

    cursor_mutex: Arc<Mutex<()>>,
    default_point: Option<Point>,
    relay: String,
    join_handle: Option<JoinHandle<()>>,
}

impl HeaderSlurp {
    pub fn new(
        relay: String,
        directory: PathBuf,
        default_point: Option<Point>,
        batch_size: u8,
        cursor_mutex: Arc<Mutex<()>>,
        block_batches: mpsc::SyncSender<(Point, Point)>,
    ) -> Self {
        Self {
            base_directory: directory.clone(),
            directory: directory.join("headers"),
            relay,
            default_point,
            batch_size,
            block_batches,
            cursor_mutex,
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

    pub fn slurp(&mut self, channel: StdChannel) -> anyhow::Result<()> {
        fs::create_dir_all(&self.directory).expect("couldn't create directory");

        // Read the latest cursor
        let gaurd = self.cursor_mutex.lock().unwrap();
        
        let cursor_file = self.base_directory.join("cursors").join(&self.relay);
        let known_points = if cursor_file.exists() {
            log::info!(target: &self.relay[..11], "reading cursor file");
            let cursor_contents = fs::read(cursor_file).with_context(|| "unable to load cursor file")?;
            let cursor: Cursor = serde_cbor::from_slice(&cursor_contents[..]).with_context(|| "unable to parse cursor")?;
            cursor.to_points()
        } else if let Some(default_point) = self.default_point.clone() {
            log::info!(target: &self.relay[..11], "syncing from default point {:?}", &self.default_point);
            vec![default_point]
        } else {
            log::info!(target: &self.relay[..11], "syncing from origin");
            vec![Point::Origin]
        };

        drop(gaurd);

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
        Ok(())
    }

    pub fn join(&mut self) -> thread::Result<()> {
        match self.join_handle.take() {
            Some(jh) => jh.join(),
            None => Ok(()),
        }
    }
}
