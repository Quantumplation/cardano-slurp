use std::{thread::{self, JoinHandle}, sync::mpsc::Receiver, fs, path::PathBuf};

use pallas::{network::{miniprotocols::{blockfetch, Point}, multiplexer::StdChannel}, codec::minicbor, ledger::{primitives::{babbage, alonzo, byron}, traverse::ComputeHash}};

pub struct BodySlurp {
    pub directory: PathBuf,
}

impl BodySlurp {

    fn ebb_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, byron::EbBlock);
        let (_, block) = minicbor::decode::<BlockWrapper>(cbor).ok()?;
        let header = block.header;
        Some(Point::Specific(header.consensus_data.epoch_id * 21600, header.compute_hash().to_vec()))
    }

    fn byron_point(cbor: &Vec<u8>) -> Option<Point> {
        type BlockWrapper = (u16, byron::Block);
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

        let path = crate::utils::artifact_path(directory.clone(), point);
        fs::create_dir_all(path.clone()).expect(format!("unable to creact directory {:?}", path).as_str());
        fs::write(path, body).expect("could not save body");
    }

    pub fn slurp(&self, channel: StdChannel, block_batches: Receiver<(Point, Point)>) -> JoinHandle<()> {
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
