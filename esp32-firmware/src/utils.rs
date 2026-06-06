pub const ID: &str = match option_env!("ID") {
    Some(v) => v,
    None => "0",
};

// 2 x 10 matrix of f32 (4bytes) + 11 bytes type information = 91 bytes
pub const MDS_MAX_SIZE: usize = 128;
// (6+4) * 10 + type info = 101
pub const DISTANCE_MAP_MAX_SIZE: usize = 128;
pub const RX_CHANNEL_SIZE: usize = 256;
