use std::collections::VecDeque;

use pallas::network::miniprotocols::Point;
use minicbor::{Encode, Decode, bytes::ByteArray};

#[derive(Clone, Encode, Decode)]
pub struct SerializablePoint {
    #[n(0)]
    pub slot: u64,
    #[n(1)]
    pub hash: ByteArray<32>,
}

impl From<Point> for SerializablePoint {
    fn from(value: Point) -> Self {
        match value {
            Point::Origin => SerializablePoint { slot: 0, hash: ByteArray::from([0; 32]) },
            Point::Specific(slot, hash) => SerializablePoint { slot, hash: ByteArray::from(TryInto::<[u8; 32]>::try_into(hash).expect("invalid hash")) },
        }
    }
}

impl Into<Point> for SerializablePoint {
    fn into(self) -> Point {
        if self.slot == 0 {
            Point::Origin
        } else {
            Point::Specific(self.slot, self.hash.to_vec())
        }
    }
}

#[derive(Encode, Decode)]
pub struct Cursor {
    #[n(0)]
    pub points: VecDeque<SerializablePoint>
}

const CURSOR_BACKLOG: usize = 20;
impl Cursor {
    pub fn add_point(&mut self, value: Point) {
        self.points.push_front(value.into());
        if self.points.len() > CURSOR_BACKLOG {
            self.points.pop_back();
        }
    }
}