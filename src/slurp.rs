use std::{
    path::PathBuf,
    sync::{mpsc::{self, Receiver}, Mutex, Arc},
    thread, fs, collections::VecDeque,
};

use pallas::network::{
    miniprotocols::{handshake, Point, MAINNET_MAGIC},
    multiplexer::{bearers::Bearer, StdChannel, StdPlexer},
};

use crate::{body_slurp::BodySlurp, header_slurp::HeaderSlurp, cursor::Cursor};

pub struct Slurp {
    pub directory: PathBuf,
    pub relay: String,
    pub magic: Option<u64>,

    receiver: Option<Receiver<(Point, Point)>>,
    headers: HeaderSlurp,
    bodies: BodySlurp,
}

impl Slurp {
    pub fn new(directory: PathBuf, relay: String, default_point: Option<Point>, magic: Option<u64>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(10);

        fs::create_dir_all(directory.join("cursors")).expect("unable to create cursor directory");

        let cursor_file = directory.join("cursors").join(&relay);
        let cursor = if cursor_file.exists() {
            log::info!(target: &relay[..11], "reading cursor file");
            let cursor_contents = fs::read(cursor_file).expect("unable to load cursor file");
            minicbor::decode(&cursor_contents[..]).expect("unable to parse cursor")
        } else if let Some(default_point) = default_point.clone() {
            log::info!(target: &relay[..11], "syncing from default point {:?}", &default_point);
            Cursor { points: VecDeque::<_>::from([default_point.into()]) }
        } else {
            log::info!(target: &relay[..11], "syncing from origin");
            Cursor { points: VecDeque::<_>::from([Point::Origin.into()]) }
        };

        let cursor_mutex = Arc::new(Mutex::new(cursor));
        let headers = HeaderSlurp::new(relay.clone(), directory.clone(), 5, cursor_mutex.clone(), sender);
        let bodies = BodySlurp::new(relay.clone(), directory.clone(), cursor_mutex.clone());

        Self {
            directory: directory,
            relay: relay,
            receiver: Some(receiver),
            magic,
            headers,
            bodies,
        }
    }

    fn do_handshake(&self, channel: StdChannel) {
        let mut client = handshake::N2NClient::new(channel);

        let confirmation = client
            .handshake(handshake::n2n::VersionTable::v7_and_above(self.magic.unwrap_or(MAINNET_MAGIC)))
            .unwrap();

        match confirmation {
            handshake::Confirmation::Accepted(v, _) => {
                log::info!(target: &self.relay[..11], "hand-shake accepted, using version {}", v)
            }
            handshake::Confirmation::Rejected(x) => {
                log::info!(target: &self.relay[..11], "hand-shake rejected with reason {:?}", x)
            }
        }
    }

    pub fn slurp(&mut self) -> anyhow::Result<()> {
        log::info!(target: &self.relay[..11], "starting slurp for a relay");

        // setup a TCP socket to act as data bearer between our agents and the remote
        // relay.
        let bearer = Bearer::connect_tcp(self.relay.clone())?;

        // setup the multiplexer by specifying the bearer and the IDs of the
        // miniprotocols to use
        let mut plexer = StdPlexer::new(bearer);
        let handshake = plexer.use_channel(0);
        let chainsync = plexer.use_channel(2);
        let blockfetch = plexer.use_channel(3);

        plexer.muxer.spawn();
        plexer.demuxer.spawn();

        // execute the required handshake against the relay
        self.do_handshake(handshake);

        // execute the chainsync flow from an arbitrary point in the chain
        self.headers.slurp(chainsync).expect("unable to start slurping headers");
        self.bodies.slurp(blockfetch, self.receiver.take().unwrap());
        Ok(())
    }

    pub fn join(&mut self) -> thread::Result<()> {
        self.headers.join()?;
        self.bodies.join()?;
        Ok(())
    }
}
