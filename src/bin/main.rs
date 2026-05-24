#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use core::error;

use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::peripherals;
use esp_hal::rtc_cntl::sleep;
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNow, EspNowManager, PeerInfo};
use log::info;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();
const HEAP_SIZE: usize = 128 * 1024;
#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]

const DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];

// test to see if my ssh key is working
#[esp_rtos::main]
async fn main(_spawner: embassy_executor::Spawner) {
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    esp_println::logger::init_logger_from_env();

    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);

    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    let (_wifi_controller, interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).unwrap();

    let mac = interfaces.station.mac_address();
    info!("Device MAC address {:?}", mac);
    let esp_now = interfaces.esp_now;
    esp_now.set_channel(1).unwrap();

    let (manager, mut tx, rx) = esp_now.split();
    loop {
        match tx.send(&BROADCAST, &DUMMY_MSG) {
            Ok(waiter) => {
                let _ = waiter.wait();
            }
            Err(e) => {
                log::warn!("Could not send message {e}");
            }
        }

        if let Some(packet) = rx.receive() {
            let rssi = packet.info.rx_control.rssi;
            let src = packet.info.src_address;
            let data = packet.data();

            info!("from={:02x?} rssi={}", src, rssi);
        }

        Timer::after(Duration::from_millis(1000)).await;
    }
}

