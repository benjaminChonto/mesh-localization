extern crate alloc;

use hashbrown::HashMap;

use heapless::Vec;
use libm::powf;
use log::info;
use serde::{Deserialize, Serialize};

pub const MAX_SWARM_SIZE: usize = 10;
const RSSI_ONE_METER: f32 = -56.0;
const ENV_FACTOR: f32 = 2.5;
const EMA_ALPHA: f32 = 0.3;
const WINDOW_SIZE: usize = 8;
const SPIKE_THRESHOLD: f32 = 12.0;

/// Ring-buffer sliding window
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
struct RssiWindow {
    buf: [f32; WINDOW_SIZE],
    head: usize,
    sum: f32,
    len: usize,
}

impl RssiWindow {
    fn new(initial: f32) -> Self {
        let mut buf = [0.0f32; WINDOW_SIZE];
        buf[0] = initial;
        RssiWindow {
            buf,
            head: 1,
            sum: initial,
            len: 1,
        }
    }

    /// Push a new value; returns the updated window mean.
    fn push(&mut self, value: f32) -> f32 {
        if self.len < WINDOW_SIZE {
            // Still filling up — just append
            self.buf[self.head] = value;
            self.sum += value;
            self.len += 1;
            self.head = (self.head + 1) % WINDOW_SIZE;
        } else {
            // Overwrite oldest slot
            let oldest = self.buf[self.head];
            self.sum += value - oldest;
            self.buf[self.head] = value;
            self.head = (self.head + 1) % WINDOW_SIZE;
        }
        self.mean()
    }

    #[inline]
    fn mean(&self) -> f32 {
        self.sum / self.len as f32
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct State {
    pub rssi: i8,       // raw latest RSSI (for display/debug)
    ema_rssi: f32,      // EMA-smoothed, spike-filtered value
    window: RssiWindow, // sliding window for spike reference
    pub dist: f32,
    pub count: u32,
}

impl State {
    pub fn new(rssi: i8) -> State {
        let f = rssi as f32;
        State {
            rssi,
            ema_rssi: f,
            window: RssiWindow::new(f),
            dist: dist_from_rssi(f),
            count: 1,
        }
    }

    pub fn update(&mut self, rssi: i8) {
        self.rssi = rssi;
        self.count += 1;

        let raw = rssi as f32;
        let win_mean = self.window.mean();

        // If the new sample is a outlier vs. the recent window mean,
        // clamp it toward the mean rather than discarding it entirely.
        // Discarding causes stalls; clamping keeps the filter responsive
        // while suppressing the spike's magnitude.
        let filtered = if (raw - win_mean).abs() > SPIKE_THRESHOLD {
            // Clamp to the boundary: preserve direction but limit excursion
            let sign = if raw > win_mean { 1.0 } else { -1.0 };
            win_mean + sign * SPIKE_THRESHOLD
        } else {
            raw
        };

        self.window.push(raw);

        self.ema_rssi = EMA_ALPHA * filtered + (1.0 - EMA_ALPHA) * self.ema_rssi;

        self.dist = dist_from_rssi(self.ema_rssi);
    }

    /// Smoothed RSSI that drives distance estimation.
    #[inline]
    pub fn smoothed_rssi(&self) -> f32 {
        self.ema_rssi
    }
}

#[inline]
fn dist_from_rssi(rssi: f32) -> f32 {
    powf(10.0, (RSSI_ONE_METER - rssi) / (10.0 * ENV_FACTOR))
}

#[derive(Debug)]
pub struct NodeState {
    pub mac: [u8; 6],
    pub neighbours: HashMap<[u8; 6], HashMap<[u8; 6], State>>,
    pub mds: Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
}

impl NodeState {
    pub fn new(mac: [u8; 6]) -> NodeState {
        let mut state = NodeState {
            mac,
            neighbours: HashMap::new(),
            mds: Vec::default(),
        };
        state.neighbours.insert(mac, HashMap::new());
        state
    }

    pub fn register_neighbour(&mut self, mac: [u8; 6]) {
        self.neighbours.insert(mac, HashMap::new());
    }

    // method that updates the measurement from the current node to the node that it just received a
    // broadcast from
    // src = own node, address = node we received it from
    pub fn update_distance_from_self(&mut self, self_addr: [u8; 6], other_addr: [u8; 6], rssi: i8) {
        let state_map = self
            .neighbours
            .get_mut(&self_addr)
            .expect("Node's mac address should have been set upon initialization");
        state_map
            .entry(other_addr)
            .and_modify(|state| state.update(rssi as i8))
            .or_insert_with(|| State::new(rssi as i8));
    }

    // method that updates the matrix of measurements that a node has of its neighbours
    pub fn update_measurements_from_neighbor(
        &mut self,
        mac: [u8; 6],
        measurements: HashMap<[u8; 6], State>,
    ) {
        self.neighbours.insert(mac, measurements);
    }

    // method that generates adjacency matrix to be used for MDS
    pub fn neighbour_matrix(&self) -> Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
        let mut vec: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
        self.neighbours.keys().for_each(|&node| {
            let _ = vec.push(node);
        });
        // Mac addresses are unique, so this is fine (more performant then regular sort)
        vec.sort_unstable();

        let matrix: Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> = vec
            .iter()
            .map(|node| {
                let neighbours = &self.neighbours[node];
                vec.iter()
                    .map(|n| {
                        neighbours
                            .get(n)
                            .map(|state| state.dist)
                            .unwrap_or_else(|| if node == n { 0.0 } else { f32::INFINITY })
                    })
                    .collect()
            })
            .collect();
        matrix
    }
}
