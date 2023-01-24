use std::{path::PathBuf, fs};

use pallas::network::miniprotocols::Point;

const LARGE_BUCKET_SIZE: u64 = 200_000_000;
const SMALL_BUCKET_SIZE: u64 =     200_000;

pub fn artifact_path(base_directory: PathBuf, point: Point) -> PathBuf {
    let Point::Specific(slot, hash) = point else { panic!("must call artifact_subdirectory with a specific point") };
    
    // [Bucketing]: We store files in {directory}/{upper_bucket}/{lower_bucket}/{slot}-{hash}
    // the upper/lower bucket ensures that we don't have *too* many files per directory
    // typically you should try to avoid having more than 10k files in a directory
    // each upper bucket rolls over every 20_000_000 slots, which is around 230 days
    // each lower bucket rolls over every 200_000 slots, which is around 2 days
    // each upper bucket will have 1000 subdirectories, and each subdirectory will have on average 10k files
    let upper_bucket = format!("{}", slot - (slot % LARGE_BUCKET_SIZE));
    let lower_bucket = format!("{}", slot - (slot % SMALL_BUCKET_SIZE));
    let sub_directory = base_directory.join(upper_bucket).join(lower_bucket);
    let file = format!("{}-{}", slot, hex::encode(hash));
    
    sub_directory.join(file)
}