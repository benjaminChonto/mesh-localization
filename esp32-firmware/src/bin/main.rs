#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use embassy_net::{Config, StackResources};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::rng::Rng;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use esp32_firmware::state::NodeState;
use esp32_firmware::telemetry::{WIFI_PASS, WIFI_SSID, send_telemetry};
use log::info;
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];
const ID: Option<&str> = option_env!("ID");

static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>) {
    runner.run().await
}

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
async fn receive_packet(rx: EspNowReceiver<'static>, state: &'static NodeState) {
    loop {
        while let Some(packet) = rx.receive() {
            let rssi = packet.info.rx_control.rssi;
            let src = packet.info.src_address;
            let data = packet.data();

            state.update(src, rssi);
            info!(
                "from={:02x?} rssi={}; data={}",
                src,
                rssi,
                core::str::from_utf8(data).unwrap_or("?")
            );
        }
        Timer::after_millis(1000).await;
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    esp_alloc::heap_allocator!(size: HEAP_SIZE);
    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let (mut wifi_controller, interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).unwrap();
    let mac = interfaces.station.mac_address();
    info!("Device MAC address {:?}", mac);

    wifi_controller
        .set_config(&esp_radio::wifi::Config::Station(
            esp_radio::wifi::sta::StationConfig::default()
                .with_ssid(WIFI_SSID)
                .with_password(WIFI_PASS.into()),
        ))
        .unwrap();
    info!("Connecting to '{}'…", WIFI_SSID);
    wifi_controller.connect_async().await.unwrap();
    info!("Associated with '{}'", WIFI_SSID);

    let (stack, runner) = embassy_net::new(
        interfaces.station,
        Config::dhcpv4(Default::default()),
        STACK_RESOURCES.init(StackResources::new()),
        Rng::new().random() as u64,
    );
    spawner.spawn(net_task(runner).unwrap());

    let esp_now = interfaces.esp_now;

    let mut led =
        esp_hal::gpio::Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());

    let state: &'static NodeState = Box::leak(Box::new(NodeState::default()));
    let (_, tx, rx) = esp_now.split();

    spawner.spawn(broadcast_ping(tx).unwrap());
    spawner.spawn(receive_packet(rx, state).unwrap());
    spawner.spawn(send_telemetry(stack, state).unwrap());

    loop {
        led.toggle();
        Timer::after(Duration::from_millis(1000)).await;
    }
}
