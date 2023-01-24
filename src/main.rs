use clap::Parser;
use slurp::Slurp;

mod args;
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
        let mut slurp = Slurp::new(args.directory.clone(), relay);
        slurp.slurp();
        connections.push(slurp);
    }

    for connection in connections.iter_mut() {
        connection.join().expect("error while slurping");
    }
}
