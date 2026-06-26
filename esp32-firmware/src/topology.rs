extern crate alloc;

use crate::state::{MAX_SWARM_SIZE, NodeState, State};
use embassy_time::Instant;
use hashbrown::HashMap;
use heapless::Vec;

pub struct Topology {
    node_state: NodeState,
    sequence: i32,
    topology_table: HashMap<[u8; 6], (Vec<[u8; 6], MAX_SWARM_SIZE>, i32, Instant)>, // MAC, neighbors, sequence, timestamp
}

impl Topology {
    pub fn new(node_state: NodeState) -> Topology {
        Topology {
            node_state,
            sequence: 0,
            topology_table: HashMap::<[u8; 6], (Vec<[u8; 6], MAX_SWARM_SIZE>, i32, Instant)>::new(),
        }
    }

    pub fn update_topology(
        &mut self,
        mac: [u8; 6],
        neighbors: Vec<[u8; 6], MAX_SWARM_SIZE>,
        sequence: i32,
    ) {
        if let Some((_, existing_sequence, _)) = self.topology_table.get(&mac) {
            if *existing_sequence <= sequence {
                return; // Ignore older or same sequence
            }
        }
        self.topology_table
            .insert(mac, (neighbors, sequence, Instant::now()));
    }

    pub fn generate_tc_message(&mut self) -> Vec<u8> {
        self.sequence += 1;
        let mut message = Vec::new();
        message.extend_from_slice(&self.node_state.mac);
        message.extend_from_slice(&self.sequence.to_be_bytes());
        message.extend_from_slice(
            &(self
                .node_state
                .neighbours
                .keys()
                .cloned()
                .collect::<Vec<[u8; 6], MAX_SWARM_SIZE>>())
            .to_be_bytes(),
        );
        message
    }
}
