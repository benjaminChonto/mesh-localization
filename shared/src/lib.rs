#![no_std]
use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_SWARM_SIZE: usize = 10;
pub type MdsResult = Vec<Vec<f32, 2>, MAX_SWARM_SIZE>;

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum TelemetryMessage<'a> {
    Log { msg: &'a str },
    Mds(MdsResult),
    Perf(PerfSnapshot),
}

#[derive(Serialize, Clone, Copy, Default, Deserialize)]
pub struct PerfSnapshot {
    pub broadcast_ping_ms: u64,
    pub process_packet_ms: u64,
    pub calculate_state_ms: u64,
    pub rx_queue_depth: usize,
}
