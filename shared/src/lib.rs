#![no_std]
use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_SWARM_SIZE: usize = 10;
pub type MdsResult = Vec<Vec<f32, 2>, MAX_SWARM_SIZE>;

#[derive(Serialize, Deserialize)]
pub enum TelemetryMessage<'a> {
    Log { msg: &'a str },
    Mds(MdsResult),
    Perf(PerformanceMetrics),
}

#[derive(Serialize, Clone, Copy, Default, Deserialize)]
pub struct PerformanceMetrics {
    pub broadcast_clone_dist_ns: u64,
    pub process_packet_ns: u64,
    pub calculate_state_ns: u64,
    // pub avg_broadcast_ping_ms: u64,
    // pub avg_process_packet_ms: u64,
    // pub avg_calculate_state_ms: u64,
    // pub avg_rx_queue_depth: usize,
    // pub n_broadcast: u64,
    // pub n_packet: u64,
    // pub n_calculate_state: u64,
    // pub n_rx_queue_depth: usize,
}

impl PerformanceMetrics {
    pub fn new() -> PerformanceMetrics {
        PerformanceMetrics {
            broadcast_clone_dist_ns: 0,
            process_packet_ns: 0,
            calculate_state_ns: 0,
            // avg_broadcast_ping_ms: 0,
            // avg_process_packet_ms: 0,
            // avg_calculate_state_ms: 0,
            // avg_rx_queue_depth: 0,
            // n_broadcast: 0,
            // n_packet: 0,
            // n_calculate_state: 0,
            // n_rx_queue_depth: 0,
        }
    }
}
