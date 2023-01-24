use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

use pallas::network::{
    miniprotocols::{handshake, Point, MAINNET_MAGIC},
    multiplexer::{bearers::Bearer, StdChannel, StdPlexer},
};

use anyhow::Error;

use crate::{body_slurp::BodySlurp, header_slurp::HeaderSlurp};

pub struct Slurp {
    pub directory: PathBuf,
    pub relay: String,

    receiver: Option<Receiver<(Point, Point)>>,
    headers: HeaderSlurp,
    bodies: BodySlurp,
}

impl Slurp {
    pub fn new(directory: PathBuf, relay: String) -> Self {
        let (sender, receiver) = mpsc::sync_channel(10);

        let headers = HeaderSlurp::new(relay.clone(), directory.join("headers"), 5, sender);
        let bodies = BodySlurp::new(relay.clone(), directory.join("bodies"));

        Self {
            directory: directory,
            relay: relay,
            receiver: Some(receiver),
            headers,
            bodies,
        }
    }

    fn do_handshake(&self, channel: StdChannel) {
        let mut client = handshake::N2NClient::new(channel);

        let confirmation = client
            .handshake(handshake::n2n::VersionTable::v7_and_above(MAINNET_MAGIC))
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
        let channel0 = plexer.use_channel(0);
        let channel2 = plexer.use_channel(2);
        let channel3 = plexer.use_channel(3);

        plexer.muxer.spawn();
        plexer.demuxer.spawn();

        // execute the required handshake against the relay
        self.do_handshake(channel0);

        // execute the chainsync flow from an arbitrary point in the chain
        self.headers.slurp(channel2);
        self.bodies.slurp(channel3, self.receiver.take().unwrap());
        Ok(())
    }

    pub fn join(&mut self) -> thread::Result<()> {
        self.headers.join()?;
        self.bodies.join()?;
        Ok(())
    }
}
