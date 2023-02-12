use std::{net::{TcpListener, Ipv4Addr, SocketAddrV4}, thread::{self}};

use clap::Parser;
use pallas::network::{multiplexer::{bearers::Bearer, StdPlexer}, miniprotocols::{txsubmission::{self, EraTxId, TxIdAndSize, Server, EraTxBody}, handshake, PROTOCOL_N2N_HANDSHAKE, PROTOCOL_N2N_TX_SUBMISSION, PROTOCOL_SERVER}};
use slurp::Slurp;

mod args;
mod cursor;
mod topology;
mod body_slurp;
mod header_slurp;
mod slurp;
mod utils;

fn main() {
    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(tracing::Level::TRACE)
            .finish(),
    )
    .unwrap();

    let args = args::Args::parse();

    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let mut connections: Vec<Slurp> = vec![];
/*
    for relay in args.relay {
        let slurp = Slurp::new(args.directory.clone(), relay, args.fallback_point.clone(), args.testnet_magic);
        connections.push(slurp);
    }

    if let Some(topology_file) = args.topology_file {
        let file_contents = fs::read_to_string(topology_file).expect("unable to open topology file");
        let topology: Topology = serde_json::from_str(&file_contents).expect("unable to parse topology file");
        for producer in topology.producers {
            let url = format!("{}:{}", producer.address, producer.port);
            let slurp = Slurp::new(args.directory.clone(), url, args.fallback_point.clone(), args.testnet_magic);
            connections.push(slurp);
        }
    }

    for connection in connections.iter_mut() {
        if let Err(e) = connection.slurp() {
            log::warn!("connection refused from {}: {}", connection.relay, e);
        }
    }
    
*/
    // Start listening in case someone wants to connect to us and tell us about transactions
    let listen_thread = thread::spawn(|| {
        log::info!("listening on port 58209");
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 58209)).unwrap();
        let (bearer, address) = Bearer::accept_tcp(listener).unwrap();
        log::info!("incoming connection received from {}", address);
        let mut plexer = StdPlexer::new(bearer);
        let handshake = plexer.use_server_channel(PROTOCOL_N2N_HANDSHAKE);
        let txsubmission = plexer.use_server_channel(PROTOCOL_N2N_TX_SUBMISSION);
        plexer.muxer.spawn();
        plexer.demuxer.spawn();

        let mut hs_client = handshake::N2NServer::new(handshake);
        
        log::info!("handshaking with client");
        let versions = hs_client
            .receive_proposed_versions()
            .unwrap();
        log::info!("possible versions: {:#?}", versions.values);
        let max = *versions.values.keys().max().unwrap();
        log::info!("selecting version {}", max);
        hs_client.accept_version(max, versions.values.get(&max).unwrap().clone()).unwrap();

        log::info!("handshake succesful");

        let mut server = txsubmission::Server::new(txsubmission);
        log::info!("waiting for init message");
        server.wait_for_init().unwrap();
        log::info!("requesting up to 4 transactions");
        server.acknowledge_and_request_tx_ids(true, 0, 4).unwrap();
        let mut fifo = 0;
        loop {
            match server.receive_next_reply().unwrap() {
                txsubmission::Reply::TxIds(ids_and_sizes) => {
                    fifo += ids_and_sizes.len() as u16;
                    log::info!("received {} tx ids: {:#?}", ids_and_sizes.len(), ids_and_sizes.iter().map(|id: &TxIdAndSize<EraTxId>| (hex::encode(&id.0.1), &id.1)).collect::<Vec<_>>());
                    if ids_and_sizes.len() > 0 {
                        log::info!("downloading these txs");
                        server.request_txs(ids_and_sizes.iter().map(|tx| tx.0.clone()).collect()).unwrap();
                    } else {
                        log::info!("no txs received, requesting more (fifo == {})", fifo);
                        server.acknowledge_and_request_tx_ids(fifo == 0, fifo, 4).unwrap();
                        fifo = 0;
                    }
                },
                txsubmission::Reply::Txs(txs) => {
                    log::info!("received {} txs, requesting more (fifo == {})", txs.len(), fifo);
                    server.acknowledge_and_request_tx_ids(true, fifo, 4).unwrap();
                    fifo = 0;
                },
                txsubmission::Reply::Done => {
                    log::info!("txsubmission protocol done");
                    break;
                },
            }
        }
    });

    for connection in connections.iter_mut() {
        connection.join().expect("error while slurping");
    }

    listen_thread.join().expect("error listening for incoming connections");
}
