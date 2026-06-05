extern crate alloc;

use hashbrown::HashMap;

use heapless::Vec;
use libm::powf;
use log::info;

const RSSI_ONE_METER: f32 = -56.0;
const ENV_FACTOR: f32 = 2.5;

// EMA smoothing factor: higher = newer samples weighted more
const EMA_ALPHA: f32 = 0.3;

// Sliding window size
const WINDOW_SIZE: usize = 8;

// Spike rejection: if new sample deviates more than this from the window mean,
// it's clamped to mean ± threshold instead of used raw.
// Tune based on your measurement rate — at ~10 Hz, 12 dBm is reasonable.
const SPIKE_THRESHOLD: f32 = 12.0;

/// Ring-buffer sliding window
#[derive(Debug, Copy, Clone)]
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

#[derive(Debug, Copy, Clone)]
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

// --- NodeState unchanged below, except the print_table column --------

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
        if let Some(state) = self.neighbours.get_mut(&src_address) {
            state.update(rssi);
        } else {
            self.neighbours.insert(src_address, State::new(rssi));
        }
    }

    pub fn print_table(&self) {
        let mut entries: Vec<([u8; 6], State), 32> = Vec::new();
        for (addr, state) in &self.neighbours {
            let _ = entries.push((*addr, *state));
        }
        entries.sort_unstable_by_key(|(addr, _)| *addr);
        info!("+-------------------+------+--------+----------+--------+");
        info!("| MAC Address       | RSSI |  Sooth | Dist (m) | Count  |");
        info!("+-------------------+------+--------+----------+--------+");
        for (addr, state) in entries.iter() {
            info!(
                "| {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X} | {:>4} | {:>6.1} | {:>8.2} | {:>6} |",
                addr[0],
                addr[1],
                addr[2],
                addr[3],
                addr[4],
                addr[5],
                state.rssi,
                state.smoothed_rssi(),
                state.dist,
                state.count
            );
        }
        info!("+-------------------+------+--------+----------+--------+");
    }
}
