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
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use log::info;
use mesh_localization::state::NodeState;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];
const ID: Option<&str> = option_env!("ID");

// TODO i do not like that this is global but it needs static lifetime for the channel to be shared
// across tasks.
static RX_CHANNEL: Channel<CriticalSectionRawMutex, RxPacket, 265> = Channel::new();

// TODO maybe move this struct to somewhere else?
#[derive(Clone, Copy)]
pub struct RxPacket {
    pub src: [u8; 6],
    pub rssi: i8,
    pub len: u16,
    pub data: [u8; 250], // TODO figure out max data packet len
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
        Timer::after_millis(50).await
    }
}

#[embassy_executor::task]
async fn receive_packet(rx: EspNowReceiver<'static>) {
    loop {
        while let Some(packet) = rx.receive() {
            let rssi = packet.info.rx_control.rssi as i8;
            let src = packet.info.src_address;

            // TODO figure out the max length of data
            let payload = packet.data();
            let len = payload.len().min(250);

            // copy to pass ownership
            let mut data = [0u8; 250];
            data[..len].copy_from_slice(&payload[..len]);

            let _ = RX_CHANNEL
                .send(RxPacket {
                    src,
                    rssi,
                    len: len as u16,
                    data,
                })
                .await; // yield if channel is full
        }

        Timer::after_millis(1).await;
    }
}

#[embassy_executor::task]
async fn process_packet(state: &'static mut NodeState) {
    loop {
        let mut changed = false;

        // batch update state
        while let Ok(pkt) = RX_CHANNEL.try_receive() {
            state.update(pkt.src, pkt.rssi);
            changed = true;
        }

        // dont print for every single update
        if changed {
            state.print_table();
        }

        // TODO this is random timing, might want to mess with it
        // if i didnt have this i think this task stalled the draining of the channel
        // and if i added a 3rd esp it would go slower for it
        Timer::after_millis(10).await;
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

    // Initialize HAL and RTOS
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    // Setup ESP-NOW
    let (_wifi_controller, interfaces) =
        esp_radio::wifi::new(peripherals.WIFI, Default::default()).unwrap();
    let mac = interfaces.station.mac_address();
    info!("Device MAC address {:?}", mac);
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
    spawner.spawn(receive_packet(rx).unwrap());
    spawner.spawn(process_packet(state).unwrap());

    loop {
        led.toggle();
        Timer::after(Duration::from_millis(50)).await;
    }
}
