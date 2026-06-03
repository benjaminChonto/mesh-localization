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

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use hashbrown::HashMap;
use log::{error, info};
use mesh_localization::mds::MDS;
use mesh_localization::state::NodeState;
use postcard::Error;
use static_cell::StaticCell;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];
const _ID: Option<&str> = option_env!("ID");

static STATE: StaticCell<Mutex<CriticalSectionRawMutex, NodeState>> = StaticCell::new();

#[embassy_executor::task]
async fn broadcast_ping(
    mut tx: EspNowSender<'static>,
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
) {
    let static_buff: &'static mut [u8; 256] = Box::leak(Box::new([0u8; 256]));

    loop {
        let distances = {
            let node_state = state.lock().await;
            node_state.neighbours.get(&node_state.mac).cloned()
        };

        let msg = postcard::to_slice(&distances.unwrap_or_default(), &mut static_buff[..]).unwrap();

        match tx.send(&BROADCAST, msg) {
            Ok(waiter) => {
                let _ = waiter.wait();
            }
            Err(e) => {
                log::warn!("Could not send message {e}");
            }
        }
        Timer::after_secs(2).await
    }
}

#[embassy_executor::task]
async fn receive_packet(
    rx: EspNowReceiver<'static>,
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
) {
    loop {
        // TODO: Consider offloading to a queue and processing in a separate task
        while let Some(packet) = rx.receive() {
            let rssi = packet.info.rx_control.rssi;
            let src = packet.info.src_address;
            let data: Result<HashMap<[u8; 6], f32>, Error> = postcard::from_bytes(packet.data());

            let mut node_state = state.lock().await;
            let mac = node_state.mac;
            node_state.add_distance(mac, src, rssi);
            let _ = data
                .map(|d| node_state.add_neighbour_measurement(src, d))
                .inspect_err(|e| error!("Failed to update data: {:?}", e));

            // info!("from={:02x?} rssi={}; data={:?}", src, rssi, data.unwrap_or_default());
        }
        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::task]
async fn calculate_state(state: &'static Mutex<CriticalSectionRawMutex, NodeState>) {
    let mut mds = MDS::default();
    loop {
        let neighbour_dist = { state.lock().await.neighbour_matrix() };
        if neighbour_dist
            .iter()
            .any(|row| row.contains(&f32::INFINITY))
        {
            // Neighbour matrix is incomplete
            Timer::after_millis(5000).await;
            continue;
        }
        let mds = mds.compute(neighbour_dist);
        {
            state.lock().await.mds = mds.clone();
        }
        Timer::after_millis(5000).await;
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

    let state: &'static Mutex<CriticalSectionRawMutex, NodeState> =
        STATE.init(Mutex::new(NodeState::new(mac)));

    let (_, tx, rx) = esp_now.split();

    // Spawn tasks
    spawner.spawn(broadcast_ping(tx, state).unwrap());
    spawner.spawn(receive_packet(rx, state).unwrap());
    spawner.spawn(calculate_state(state).unwrap());

    loop {
        led.toggle();
        {
            let node_state = state.lock().await;
            info!(
                "neighbours:\n{:?}\nmds:\n{:?}",
                node_state.neighbour_matrix(),
                node_state.mds
            );
        }
        Timer::after(Duration::from_millis(3000)).await;
    }
}
