pub const _ID: Option<&str> = option_env!("ID");

// 2 x 10 matrix of f32 (4bytes) + 11 bytes type information = 91 bytes
pub const MDS_MAX_SIZE: usize = 128;
// (6+4) * 10 + type info = 101
pub const DISTANCE_MAP_MAX_SIZE: usize = 128;
