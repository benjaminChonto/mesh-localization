#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;
use core::num;

use alloc::boxed::Box;

use alloc::fmt::format;
use alloc::format;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::{TcpClient, TcpClientState};
use embassy_time::Duration;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::system::SleepSource::Timer;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use esp32_firmware::state::NodeState;
use log::info;
use reqwless::client::HttpClient;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];
const ID: Option<&str> = option_env!("ID");

const WIFI_SSID: &str = env!("WIFI_SSID").unwrap_or_else(|_| "AIVD Deurbel 42".into());
const WIFI_PASS: &str = env!("WIFI_PASS").unwrap_or_else(|_| "RoombaRobinCasaHouse".into());
const IP_ADDR: &str = env!("IP_ADDR").unwrap_or_else(|_| "192.168.1.100".into());

#[embassy_executor::task]
async fn broadcast_ping(mut tx: EspNowSender<'static>) {
    let mut seq: i32 = 0;
    let id = ID.unwrap_or("0");
    loop {
        let msg = format!("{}:\t{}", id, seq);
        match tx.send(&BROADCAST, msg.as_bytes()) {
            Ok(waiter) => {
                let _ = waiter.wait();
            }
            Err(e) => {
                log::warn!("Could not send message {e}");
            }
        }
        seq += 1;
        Timer::after_secs(2).await
    }
}

#[embassy_executor::task]
async fn receive_packet(rx: EspNowReceiver<'static>, state: &'static mut NodeState) {
    loop {
        // TODO: Consider offloading to a queue and processing in a separate task
        while let Some(packet) = rx.receive() {
            let rssi = packet.info.rx_control.rssi;
            let src = packet.info.src_address;
            let data = packet.data();

            state.update(src, rssi);
            info!(
                "from={:02x?} rssi={}; data={}",
                src,
                rssi,
                str::from_utf8(data).unwrap_or("?")
            );
        }
        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::task]
async fn send_telemetry(state: &'static NodeState) {
    let id = ID.unwrap_or("0");
    let num_neighbors = state.num_neighbors();
    let msg = format!(
        "{{\"id\": \"{}\", \"num_neighbors\": {}}}",
        id, num_neighbors
    );
    let stack = embassy_net::Stack::get();
    let dns = DnsSocket::new(stack);
    let tcp_state = TcpClientState::<1, 4096, 4096>::new();
    let tcp = TcpClient::new(stack, &tcp_state);

    let mut client = HttpClient::new(&tcp, &dns);
    let mut rx_buf = [0u8; 4096];
    let ip = format!("http://{}/update", IP_ADDR);

    match client
        .request(reqwless::request::Method::POST, ip.as_str())
        .with_body(msg.as_bytes())
        .send(&mut rx_buf)
        .await
    {
        Ok(response) => {
            info!("Telemetry sent successfully: {:?}", response);
        }
        Err(e) => {
            log::warn!("Failed to send telemetry: {e}");
        }
    }
    Timer::after_secs(10).await;
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    esp_alloc::heap_allocator!(size: HEAP_SIZE);
    esp_println::logger::init_logger_from_env();

    // Initialize HAL and RTOS
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    // Setup ESP-NOW
    let (wifi_controller, interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).unwrap();
    let mac = interfaces.station.mac_address();
    info!("Device MAC address {:?}", mac);

    // Connect to WiFi
    wifi_controller
        .set_config(&esp_radio::wifi::Config::Station(
            esp_radio::wifi::StationConfig::default()
                .with_ssid(WIFI_SSID)
                .with_password(WIFI_PASS),
        ))
        .unwrap();
    info!("Connecting to '{}'…", WIFI_SSID);
    wifi_controller.connect_async().await.unwrap();
    info!("Associated with '{}'", WIFI_SSID);

    let esp_now = interfaces.esp_now;
    esp_now.set_channel(1).unwrap();

    // On board status led
    let mut led =
        esp_hal::gpio::Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());

    let state = NodeState::default();
    let state = Box::leak(Box::new(state));
    let (_, tx, rx) = esp_now.split();

    // Spawn tasks
    spawner.spawn(broadcast_ping(tx).unwrap());
    spawner.spawn(receive_packet(rx, state).unwrap());

    loop {
        led.toggle();
        Timer::after(Duration::from_millis(1000)).await;
    }
}
