/**
 * Environment variables and global constants
 */
pub const WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(v) => v,
    None => "bcsonto_network",
};
pub const WIFI_PASS: &str = match option_env!("WIFI_PASS") {
    Some(v) => v,
    None => "Charlie123",
};
pub const IP_ADDR: &str = match option_env!("IP_ADDR") {
    Some(v) => v,
    None => "10.62.198.13",
};
