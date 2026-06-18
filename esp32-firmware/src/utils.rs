/**
 * Environment variables and global constants
 */
pub const WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(v) => v,
    None => "AIVD Deurbel 42"
};
pub const WIFI_PASS: &str = match option_env!("WIFI_PASS") {
    Some(v) => v,
    None => "RoombaRobinCasaHouse666",
};
pub const IP_ADDR: &str = match option_env!("IP_ADDR") {
    Some(v) => v,
    None => "10.51.232.13",
};
pub const _ID: Option<&str> = option_env!("ID");

// 2 x 10 matrix of f32 (4bytes) + 11 bytes type information = 91 bytes
pub const MDS_MAX_SIZE: usize = 128;
// (6+4) * 10 + type info = 101
pub const DISTANCE_MAP_MAX_SIZE: usize = 128;
pub const RX_CHANNEL_SIZE: usize = 256;
