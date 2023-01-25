use std::{
    fs,
    path::PathBuf,
    sync::{mpsc::Receiver, Mutex, Arc},
    thread::{self, JoinHandle},
};

use pallas::{
    codec::minicbor,
    ledger::{
        primitives::{alonzo, babbage, byron},
        traverse::ComputeHash,
    },
    network::{
        miniprotocols::{blockfetch, Point},
        multiplexer::StdChannel,
    },
};

use ::minicbor::{to_vec as cbor_to_vec};

use crate::cursor::{Cursor};

pub struct BodySlurp {
    pub directory: PathBuf,

    cursor_mutex: Arc<Mutex<Cursor>>,
    relay: String,
    join_handle: Option<JoinHandle<()>>,
}

impl BodySlurp {
    pub fn new(relay: String, directory: PathBuf, cursor_mutex: Arc<Mutex<Cursor>>) -> Self {
        Self {
            directory,
            cursor_mutex,
            relay,
            join_handle: None,
        }
    }

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, byron::EbBlock);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(
            header.consensus_data.epoch_id * 21600,
            header.compute_hash().to_vec(),
        ))
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, byron::Block);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(
            header.consensus_data.0.epoch * 21600 + header.consensus_data.0.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn shelley_or_alonzo_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, alonzo::Block);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(
            header.header_body.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn babbage_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, babbage::Block);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(
            header.header_body.slot,
            header.compute_hash().to_vec(),
        ))
    }

    fn handle_body(cursor_mutex: Arc<Mutex<Cursor>>, relay: &String, base_directory: &PathBuf, body: Vec<u8>) {
        let point = BodySlurp::ebb_point(&body)
            .or_else(|| BodySlurp::byron_point(&body))
            .or_else(|| BodySlurp::shelley_or_alonzo_point(&body))
            .or_else(|| BodySlurp::babbage_point(&body))
            .expect("unrecognized block");
        log::info!(target: &relay[..11], "downloaded block {:?} ({} bytes)", point, body.len());

        let path = crate::utils::artifact_path(base_directory.join("bodies"), point.clone());
        fs::create_dir_all(path.parent().unwrap())
            .expect(format!("unable to creact directory {:?}", path).as_str());
        fs::write(path, body).expect("could not save body");

        {
          let mut cursor_gaurd = cursor_mutex.lock().expect("unable to acquire lock");

          cursor_gaurd.add_point(point);

          let cursor_bytes = cbor_to_vec::<&Cursor>(&cursor_gaurd).expect("unable to serialize cursor");
          fs::write(base_directory.join("cursors").join(relay), cursor_bytes).expect("unable to write cursor file");

          drop(cursor_gaurd);
        }
    }

    pub fn slurp(&mut self, channel: StdChannel, block_batches: Receiver<(Point, Point)>) {
        fs::create_dir_all(&self.directory).expect("couldn't create directory");

        let directory = self.directory.clone();
        let relay = self.relay.clone();
        let cursor = self.cursor_mutex.clone();
        self.join_handle = Some(thread::spawn(move || {
            let mut client = blockfetch::Client::new(channel);
            loop {
                let next_range = block_batches
                    .recv()
                    .expect("failed to receive next block range");
                let blocks = client
                    .fetch_range(next_range)
                    .expect("unable to query block range");
                for block in blocks {
                    BodySlurp::handle_body(cursor.clone(), &relay, &directory, block);
                }
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
