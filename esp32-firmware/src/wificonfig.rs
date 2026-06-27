/**
 * Environment variables and global constants
 */
pub const WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(v) => v,
    None => "The Nether Why Fi",
};
pub const WIFI_PASS: &str = match option_env!("WIFI_PASS") {
    Some(v) => v,
    None => "@BlenderUbuntu4",
};
pub const IP_ADDR: &str = match option_env!("IP_ADDR") {
    Some(v) => v,
    None => "192.168.178.10",
};
