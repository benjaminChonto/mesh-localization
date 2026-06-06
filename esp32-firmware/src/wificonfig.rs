/**
 * Environment variables and global constants
 */
pub const WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(v) => v,
    None => "fried-pilk-enjoyer",
};
pub const WIFI_PASS: &str = match option_env!("WIFI_PASS") {
    Some(v) => v,
    None => "asdfghjkl",
};
pub const IP_ADDR: &str = match option_env!("IP_ADDR") {
    Some(v) => v,
    None => "172.20.10.4",
};
