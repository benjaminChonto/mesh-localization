extern crate alloc;

use crate::state::{MAX_SWARM_SIZE, State};
use embassy_time::Instant;
use hashbrown::HashMap;
use heapless::Vec;
use serde::{Deserialize, Serialize};

const NEIGHBOR_EXPIRY: u64 = 10; // seconds

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TcPayload {
    pub origin_mac: [u8; 6],
    pub sequence: i32,
    pub neighbors: Vec<[u8; 6], MAX_SWARM_SIZE>,
}

#[derive(Serialize, Deserialize)]
pub enum Packet {
    Hello(HashMap<[u8; 6], State>),
    Tc(TcPayload),
}

pub struct Topology {
    own_mac: [u8; 6],
    sequence: i32,
    // origin_mac → highest seen sequence
    seen: HashMap<[u8; 6], i32>,
    // origin_mac → (neighbor list, sequence, last-updated timestamp)
    topology_table: HashMap<[u8; 6], (Vec<[u8; 6], MAX_SWARM_SIZE>, i32, Instant)>,
}

impl Topology {
    pub fn new(own_mac: [u8; 6]) -> Self {
        Topology {
            own_mac,
            sequence: 0,
            seen: HashMap::new(),
            topology_table: HashMap::new(),
        }
    }

    pub fn generate_tc_message(&mut self, neighbors: Vec<[u8; 6], MAX_SWARM_SIZE>) -> TcPayload {
        self.sequence += 1;
        TcPayload {
            origin_mac: self.own_mac,
            sequence: self.sequence,
            neighbors,
        }
    }

    pub fn process_tc_message(
        &mut self,
        origin_mac: [u8; 6],
        neighbors: Vec<[u8; 6], MAX_SWARM_SIZE>,
        sequence: i32,
    ) -> bool {
        if origin_mac == self.own_mac {
            return false;
        }

        if let Some(&seen_seq) = self.seen.get(&origin_mac) {
            if seen_seq >= sequence {
                return false;
            }
        }

        self.seen.insert(origin_mac, sequence);
        self.update_topology(origin_mac, neighbors, sequence);
        self.expire_stale();
        true
    }

    pub fn own_mac(&self) -> [u8; 6] {
        self.own_mac
    }

    pub fn topology_table(
        &self,
    ) -> &HashMap<[u8; 6], (Vec<[u8; 6], MAX_SWARM_SIZE>, i32, Instant)> {
        &self.topology_table
    }

    fn update_topology(
        &mut self,
        mac: [u8; 6],
        neighbors: Vec<[u8; 6], MAX_SWARM_SIZE>,
        sequence: i32,
    ) {
        if let Some((_, existing_seq, _)) = self.topology_table.get(&mac) {
            if *existing_seq >= sequence {
                return;
            }
        }
        self.topology_table
            .insert(mac, (neighbors, sequence, Instant::now()));
    }

    fn expire_stale(&mut self) {
        let now = Instant::now();
        self.topology_table.retain(|_, (_, _, timestamp)| {
            now.duration_since(*timestamp).as_secs() < NEIGHBOR_EXPIRY
        });
    }
}
