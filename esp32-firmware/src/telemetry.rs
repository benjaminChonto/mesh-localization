extern crate alloc;

use crate::state::NodeState;
use alloc::format;
use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as _;
use log::info;
use smoltcp::wire::{IpAddress, IpEndpoint};

pub const TELEMETRY_PORT: u16 = 8080;

pub const WIFI_SSID: &str = match option_env!("WIFI_SSID") {
    Some(v) => v,
    None => "AIVD Deurbel 42",
};
pub const WIFI_PASS: &str = match option_env!("WIFI_PASS") {
    Some(v) => v,
    None => "RoombaRobinCasaHouse666",
};
pub const IP_ADDR: &str = match option_env!("IP_ADDR") {
    Some(v) => v,
    None => "192.168.1.100",
};

pub fn parse_endpoint(s: &str, port: u16) -> IpEndpoint {
    let mut octets = [0u8; 4];
    let mut idx = 0;
    let mut val = 0u32;
    for &b in s.as_bytes() {
        if b == b'.' {
            octets[idx] = val as u8;
            idx += 1;
            val = 0;
        } else {
            val = val * 10 + (b - b'0') as u32;
        }
    }
    octets[idx] = val as u8;
    IpEndpoint::new(
        IpAddress::v4(octets[0], octets[1], octets[2], octets[3]),
        port,
    )
}

#[embassy_executor::task]
pub async fn send_telemetry(stack: Stack<'static>, state: &'static NodeState) {
    stack.wait_config_up().await;
    info!("Network up, starting telemetry");

    let endpoint = parse_endpoint(IP_ADDR, TELEMETRY_PORT);
    let id = option_env!("ID").unwrap_or("0");

    loop {
        let num_neighbors = state.num_neighbors() as i32;
        let body = format!(r#"{{"id":"{}","num_neighbors":{}}}"#, id, num_neighbors);
        let request = format!(
            "POST /update HTTP/1.0\r\nHost: {IP_ADDR}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );

        let mut rx_buf = [0u8; 512];
        let mut tx_buf = [0u8; 512];
        let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(10)));

        match socket.connect(endpoint).await {
            Ok(()) => {
                if socket.write_all(request.as_bytes()).await.is_ok() {
                    socket.flush().await.ok();
                    info!("telemetry sent: {}", body);
                } else {
                    log::warn!("telemetry: write failed");
                }
                socket.close();
            }
            Err(e) => log::warn!("telemetry: connect failed {:?}", e),
        }

        Timer::after_secs(10).await;
    }
}
