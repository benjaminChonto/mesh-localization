extern crate alloc;

use hashbrown::HashMap;

use alloc::format;
use heapless::Vec;
use libm::powf;
use log::info;

const RSSI_SCALE: f32 = 190.0;
const RSSI_ONE_METER: f32 = -56.0;
const ENV_FACTOR: f32 = 1.5;

#[derive(Debug, Copy, Clone)]
pub struct State {
    pub rssi: i8,
    pub dist: f32,
    pub count: u32, // just for debugging, to see if we still receive messages
}

impl State {
    pub fn new(rssi: i8) -> State {
        let dist = powf(10.0, (RSSI_ONE_METER - rssi as f32) / (10.0 * ENV_FACTOR));
        State {
            rssi,
            dist,
            count: 1,
        }
    }

    pub fn update(&mut self, rssi: i8) {
        self.rssi = rssi;
        self.dist = powf(10.0, (RSSI_ONE_METER - rssi as f32) / (10.0 * ENV_FACTOR));
        self.count += 1;
    }
}

#[derive(Debug)]
pub struct NodeState {
    pub neighbours: HashMap<[u8; 6], State>,
}

impl Default for NodeState {
    fn default() -> NodeState {
        NodeState {
            neighbours: HashMap::new(),
        }
    }
}

impl NodeState {
    pub fn update(&mut self, src_address: [u8; 6], rssi: i8) {
        // TODO: update with proper estimation
        if let Some(state) = self.neighbours.get_mut(&src_address) {
            state.update(rssi);
        } else {
            self.neighbours.insert(src_address, State::new(rssi));
        }
    }

    // this is just for easier debugging
    pub fn print_table(&self) {
        let mut entries: Vec<([u8; 6], State), 32> = Vec::new();

        for (addr, state) in &self.neighbours {
            let _ = entries.push((*addr, *state));
        }

        entries.sort_unstable_by_key(|(addr, _)| *addr);

        info!("+-------------------+------+----------+--------+");
        info!("| MAC Address       | RSSI | Dist (m) | Count  |");
        info!("+-------------------+------+----------+--------+");

        for (addr, state) in entries.iter() {
            info!(
                "| {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} | {:>4} | {:>8.2} | {:>6} |",
                addr[0],
                addr[1],
                addr[2],
                addr[3],
                addr[4],
                addr[5],
                state.rssi,
                state.dist,
                state.count
            );
        }

        info!("+-------------------+------+----------+--------+");
    }
}
