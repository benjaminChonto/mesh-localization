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
    Rssi { src: [u8; 6], rssi: i8 },
}

/// Per-task timings measured in raw CPU cycles (read from the RISC-V `mcycle` CSR).
/// Cycles are the most precise clock available on the chip; divide by
/// [`CPU_CLOCK_HZ`] to convert to seconds.
#[derive(Serialize, Clone, Copy, Default, Deserialize)]
pub struct PerformanceMetrics {
    pub broadcast_clone_dist_cycles: u32,
    pub process_packet_cycles: u32,
    pub calculate_state_cycles: u32,
}

impl PerformanceMetrics {
    pub fn new() -> PerformanceMetrics {
        PerformanceMetrics {
            broadcast_clone_dist_cycles: 0,
            process_packet_cycles: 0,
            calculate_state_cycles: 0,
        }
    }
}
