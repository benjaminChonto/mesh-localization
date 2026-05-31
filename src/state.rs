extern crate alloc;

use hashbrown::HashMap;

use alloc::format;
use heapless::Vec;
use libm::powf;
use log::info;

const RSSI_ONE_METER: f32 = -56.0;
const ENV_FACTOR: f32 = 2.5;
const AVG_SMOOTHING: f32 = 0.7; // higher means newer is more important
const SPIKE_THRESHOLD: f32 = 15.0; // threshold for avoiding spikes

#[derive(Debug, Copy, Clone)]
pub struct State {
    pub rssi: i8,
    avg_rssi: f32,
    pub dist: f32,
    pub count: u32, // just for debugging, to see if we still receive messages
}

impl State {
    pub fn new(rssi: i8) -> State {
        let dist = powf(10.0, (RSSI_ONE_METER - rssi as f32) / (10.0 * ENV_FACTOR));
        State {
            rssi,
            avg_rssi: rssi as f32,
            dist,
            count: 1,
        }
    }

    pub fn update(&mut self, rssi: i8) {
        // try to not take spikes into account but this was not quite working, will have to tune
        // based on how fast we can get measurements
        // if (self.avg_rssi - (rssi as f32)).abs() < SPIKE_THRESHOLD {
        self.rssi = rssi;
        // exponential moving average, avg factor dictates if newer or older measruemenst are more
        // important
        self.avg_rssi = AVG_SMOOTHING * (rssi as f32) + (1.0 - AVG_SMOOTHING) * self.avg_rssi;
        self.dist = powf(10.0, (RSSI_ONE_METER - self.avg_rssi) / (10.0 * ENV_FACTOR));
        // }
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
