/**
 * Environment variables and global constants
 */
pub const ID: &str = match option_env!("ID") {
    Some(v) => v,
    None => "0",
};
pub const SEND_TELEMETRY: bool = match option_env!("SEND_TELEMETRY") {
    Some(v) => true,
    None => false,
};

// 2 x 10 matrix of f32 (4bytes) + 11 bytes type information = 91 bytes
pub const MDS_MAX_SIZE: usize = 128;
// ESP-NOW max payload is 250 bytes; State (~55 bytes with RssiWindow) + 6 MAC per entry
pub const DISTANCE_MAP_MAX_SIZE: usize = 250;
pub const RX_CHANNEL_SIZE: usize = 256;
pub const MQTT_TX_CHANNEL_SIZE: usize = 256;

// Number of times we try to connect to a hotspot
pub const NETWORK_RETRIES: usize = 5;

/// Raw CPU cycle counter for the ESP32-C3.
/// It is 32-bit and wraps every
/// ~26.8 s at 160 MHz, so `wrapping_sub` over short spans is exact.
#[inline]
pub fn cpu_cycles() -> u32 {
    let cycles: u32;
    // SAFETY: plain read of a machine-mode CSR; runs in M-mode on the C3.
    unsafe {
        core::arch::asm!("csrr {0}, 0x7e2", out(reg) cycles, options(nomem, nostack));
    }
    cycles
}
pub const TX_CHANNEL_SIZE: usize = 8;
