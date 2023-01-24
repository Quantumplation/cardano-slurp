use std::fs;

use clap::Parser;
use slurp::Slurp;
use topology::Topology;

mod args;
mod cursor;
mod topology;
mod body_slurp;
mod header_slurp;
mod slurp;
mod utils;

fn main() {
    let args = args::Args::parse();

    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let mut connections = vec![];

    for relay in args.relay {
        let slurp = Slurp::new(args.directory.clone(), relay, args.fallback_point.clone());
        connections.push(slurp);
    }

    if let Some(topology_file) = args.topology_file {
        let file_contents = fs::read_to_string(topology_file).expect("unable to open topology file");
        let topology: Topology = serde_json::from_str(&file_contents).expect("unable to parse topology file");
        for producer in topology.producers {
            let url = format!("{}:{}", producer.address, producer.port);
            let slurp = Slurp::new(args.directory.clone(), url, args.fallback_point.clone());
            connections.push(slurp);
        }
    }

    for connection in connections.iter_mut() {
        if let Err(e) = connection.slurp() {
            log::warn!("connection refused from {}: {}", connection.relay, e);
        }
    }

    for connection in connections.iter_mut() {
        connection.join().expect("error while slurping");
    }
}
