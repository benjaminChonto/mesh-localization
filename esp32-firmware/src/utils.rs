pub const ID: &str = match option_env!("ID") {
    Some(v) => v,
    None => "0",
};

// 2 x 10 matrix of f32 (4bytes) + 11 bytes type information = 91 bytes
pub const MDS_MAX_SIZE: usize = 128;
// ESP-NOW max payload is 250 bytes; State (~55 bytes with RssiWindow) + 6 MAC per entry
pub const DISTANCE_MAP_MAX_SIZE: usize = 250;
pub const RX_CHANNEL_SIZE: usize = 256;
pub const MQTT_TX_CHANNEL_SIZE: usize = 256;
