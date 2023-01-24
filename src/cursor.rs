use pallas::network::miniprotocols::Point;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct SerializablePoint {
    pub slot: u64,
    pub block_hash: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
pub struct Cursor {
    pub points: Vec<SerializablePoint>
}

impl Cursor {
    pub fn to_points(&self) -> Vec<Point> {
        let mut ret = vec![];
        for p in self.points.iter() {
            ret.push(Point::Specific(p.slot, p.block_hash.clone()))
        }
        return ret;
    }
}