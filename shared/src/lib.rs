#![no_std]
pub use fixed::types::I16F16;
use heapless::Vec;
use serde::{Deserialize, Serialize};

pub const MAX_SWARM_SIZE: usize = 10;
pub type MdsResult = Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>;

/// CPU clock the firmware runs at (ESP32-C3 at `CpuClock::max()` == 160 MHz).
/// Single source of truth for converting raw CPU cycle counts to wall-clock time.
pub const CPU_CLOCK_HZ: u64 = 160_000_000;

#[derive(Serialize, Deserialize)]
pub enum TelemetryMessage<'a> {
    Log { msg: &'a str },
    Mds(MdsResult),
    Perf(PerformanceMetrics),
}

/// Per-task timings measured in raw CPU cycles (read from the RISC-V `mcycle` CSR).
/// Cycles are the most precise clock available on the chip; divide by
/// [`CPU_CLOCK_HZ`] to convert to seconds.
#[derive(Serialize, Clone, Copy, Default, Deserialize)]
pub struct PerformanceMetrics {
    pub broadcast_hello_cycles: u32,
    pub broadcast_topo_cycles: u32,
    pub process_packet_hello_cycles: u32,
    pub process_packet_topo_cycles: u32,
    pub calc_state_mds_total_cycles: u32,
    pub calc_state_kabsch_cycles: u32,
    pub calc_state_mds_iter_cycles: u32,
    pub calc_state_routing_update_cycles: u32,
    pub calc_state_build_neighbors_cycles: u32,
    pub update_screen_mds_cycles: u32,
    pub update_screen_table_cycles: u32,
}

impl PerformanceMetrics {
    pub fn new() -> PerformanceMetrics {
        PerformanceMetrics {
            broadcast_hello_cycles: 0,
            broadcast_topo_cycles: 0,
            process_packet_hello_cycles: 0,
            process_packet_topo_cycles: 0,
            calc_state_mds_total_cycles: 0,
            calc_state_kabsch_cycles: 0,
            calc_state_mds_iter_cycles: 0,
            calc_state_routing_update_cycles: 0,
            calc_state_build_neighbors_cycles: 0,
            update_screen_mds_cycles: 0,
            update_screen_table_cycles: 0,
        }
    }
}
