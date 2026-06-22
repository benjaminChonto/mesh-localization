#![no_std]
#![no_main]
#![deny(
    clippy::mem_forget,
    reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

extern crate alloc;

use defmt::{error, info, warn};
use defmt_rtt as _;
use embassy_net::tcp::TcpSocket;
use embassy_net::{Config, IpAddress, IpEndpoint, StackResources};
use embassy_sync::mutex::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, OutputConfig};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use esp32_firmware::mds::MDS;
use esp32_firmware::state::{NodeState, State};
use esp32_firmware::utils::{
    DISTANCE_MAP_MAX_SIZE, ID, MDS_MAX_SIZE, MQTT_TX_CHANNEL_SIZE, RX_CHANNEL_SIZE, cpu_cycles,
};
use esp32_firmware::wificonfig::{IP_ADDR, WIFI_PASS, WIFI_SSID};
use hashbrown::HashMap;
use minimq::{Buffers, ConfigBuilder, Publication, Session};
use postcard::Error;
use shared::{I16F16, PerformanceMetrics, TelemetryMessage};
use static_cell::StaticCell;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];

static STATE: StaticCell<Mutex<CriticalSectionRawMutex, NodeState>> = StaticCell::new();
static METRICS: StaticCell<Mutex<CriticalSectionRawMutex, PerformanceMetrics>> = StaticCell::new();
static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static RX_CHANNEL: Channel<CriticalSectionRawMutex, RxPacket, RX_CHANNEL_SIZE> = Channel::new();
static MQTT_TX_CHANNEL: Channel<CriticalSectionRawMutex, TelemetryMessage, MQTT_TX_CHANNEL_SIZE> =
    Channel::new();

// TODO maybe move this struct to somewhere else?
#[derive(Clone, Copy)]
pub struct RxPacket {
    pub src: [u8; 6],
    pub rssi: i8,
    pub len: usize,
    pub data: [u8; 256], // TODO figure out max data packet len
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn broadcast_ping(
    mut tx: EspNowSender<'static>,
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>,
) {
    let mut serializer_buff = [0u8; DISTANCE_MAP_MAX_SIZE];

    loop {
        // Measure in raw CPU cycles for maximum precision (see `cpu_cycles`).
        let start_cycles = cpu_cycles();
        let mut distances: Option<HashMap<[u8; 6], State>> = None;
        {
            distances = {
                // mac addr-> 6 * 10 * 4
                let node_state = state.lock().await;
                node_state.neighbours.get(&node_state.mac).cloned()
            };
        }
        let finish = cpu_cycles().wrapping_sub(start_cycles);

        let Ok(msg) = postcard::to_slice(&distances.unwrap_or_default(), &mut serializer_buff)
        else {
            warn!("broadcast_ping: serializer buffer too small, skipping");
            Timer::after_millis(500).await;
            continue;
        };

        match tx.send(&BROADCAST, msg) {
            Ok(waiter) => {
                let _ = waiter.wait();
            }
            Err(e) => {
                warn!("Could not send message {:?}", e);
            }
        }
        {
            perf.lock().await.broadcast_clone_dist_cycles = finish;
        }
        Timer::after_millis(100).await;
    }
}

// TODO idk if this will overflow
#[allow(clippy::large_stack_frames)]
#[embassy_executor::task]
async fn receive_packet(mut rx: EspNowReceiver<'static>) {
    loop {
        // Park the task until the ESP-NOW RX interrupt wakes us (no polling).
        let packet = rx.receive_async().await;
        let rssi = packet.info.rx_control.rssi as i8;
        let src = packet.info.src_address;

        let payload = packet.data();
        let len = payload.len().min(256);

        // copy to pass ownership
        let mut data = [0u8; 256];
        data[..len].copy_from_slice(&payload[..len]);

        let _ = RX_CHANNEL
            .send(RxPacket {
                src,
                rssi,
                len,
                data,
            })
            .await; // yield if channel is full
    }
}

#[embassy_executor::task]
async fn process_packet(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>,
) {
    loop {
        // Wakes the instant a packet is pushed onto the channel; drains as fast
        // as packets arrive without the old fixed-interval polling.
        let packet = RX_CHANNEL.receive().await;
        let data: Result<HashMap<[u8; 6], State>, Error> =
            postcard::from_bytes(&packet.data[..packet.len]);

        let start_cycles = cpu_cycles();
        {
            let mut node_state = state.lock().await;
            let mac = node_state.mac; // own mac address
            node_state.update_distance_from_self(mac, packet.src, packet.rssi);
            let _ = data
                .map(|d| node_state.update_measurements_from_neighbor(packet.src, d))
                .inspect_err(|e| error!("Failed to update data: {}", defmt::Debug2Format(&e)));
        }
        let finish = cpu_cycles().wrapping_sub(start_cycles);
        {
            perf.lock().await.process_packet_cycles = finish;
        }
    }
}

#[embassy_executor::task]
async fn calculate_state(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>,
) {
    let mut mds = MDS::default();
    loop {
        let neighbour_dist = { state.lock().await.neighbour_matrix() };
        if neighbour_dist.iter().any(|row| row.contains(&I16F16::MAX)) {
            // Neighbour matrix is incomplete
            Timer::after_millis(5000).await;
            continue;
        }
        let start_cycles = cpu_cycles();
        let mds = mds.compute(neighbour_dist);
        let finish = cpu_cycles().wrapping_sub(start_cycles);
        {
            state.lock().await.mds = mds.clone(); // TODO the clone might be expensive
            // double clone :(
            // but publish state when available
            let _ = MQTT_TX_CHANNEL.try_send(TelemetryMessage::Mds(state.lock().await.mds.clone()));
            perf.lock().await.calculate_state_cycles = finish;
        }
        Timer::after_millis(100).await;
    }
}

#[embassy_executor::task]
async fn publish_metrics(perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>) {
    loop {
        let _ = MQTT_TX_CHANNEL.try_send(TelemetryMessage::Perf(perf.lock().await.clone()));
        Timer::after_millis(50).await;
    }
}

#[allow(
    clippy::large_stack_frames,
    reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) {
    esp_alloc::heap_allocator!(size: HEAP_SIZE);

    // Initialize HAL and RTOS
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let sw_interrupt =
        esp_hal::interrupt::software::SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw_interrupt.software_interrupt0);

    // Setup ESP-NOW
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
    while let Err(e) = wifi_controller.connect_async().await {
        error!("WiFi connect failed ({:?}), retrying in 5s…", e);
        Timer::after(Duration::from_secs(5)).await;
    }
    info!("Associated with '{}'", WIFI_SSID);

    let (stack, runner) = embassy_net::new(
        interfaces.station,
        Config::dhcpv4(Default::default()),
        STACK_RESOURCES.init(StackResources::new()),
        u64::from(esp_hal::rng::Rng::new().random()),
    );
    spawner.spawn(net_task(runner).unwrap());
    stack.wait_config_up().await;
    let esp_now = interfaces.esp_now;
    // esp_now.set_channel(1).unwrap();

    // On board status led
    let mut led =
        esp_hal::gpio::Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());

    let state: &'static Mutex<CriticalSectionRawMutex, NodeState> =
        STATE.init(Mutex::new(NodeState::new(mac)));

    let perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics> =
        METRICS.init(Mutex::new(PerformanceMetrics::new()));

    let (_, tx, rx) = esp_now.split();

    // Spawn tasks
    spawner.spawn(broadcast_ping(tx, state, perf).unwrap());
    spawner.spawn(receive_packet(rx).unwrap());
    spawner.spawn(process_packet(state, perf).unwrap());
    spawner.spawn(calculate_state(state, perf).unwrap());
    spawner.spawn(publish_metrics(perf).unwrap());

    let topic = alloc::format!("telemetry/{ID}");
    let mut serializer_buff = [0u8; MDS_MAX_SIZE];
    loop {
        // TODO why do we have this extra outer loop? why do we need to open a new mqtt session
        // every time?
        // MQTT Setup
        let mut rx_mqtt = [0u8; 256];
        let mut tx_mqtt = [0u8; 1024];
        let mut rx_tcp = [0u8; 256];
        let mut tx_tcp = [0u8; 1024];
        let mut mqtt_session =
            Session::new(ConfigBuilder::new(Buffers::new(&mut rx_mqtt, &mut tx_mqtt)));

        let mut socket = TcpSocket::new(stack, &mut rx_tcp, &mut tx_tcp);
        info!("Connecting to MQTT server ...");
        if let Err(e) = socket
            .connect(IpEndpoint::new(
                IpAddress::Ipv4(IP_ADDR.parse().unwrap()),
                1883,
            ))
            .await
        {
            error!("Failed to connect to mosquitto: {:?}", e);
        }

        // TODO handle properly and check results of connections / publishing
        let _ = mqtt_session.connect(socket).await.inspect_err(|e| {
            error!("Connection failed: {}", defmt::Debug2Format(&e));
        });

        loop {
            led.toggle();
            {
                // log some info
                let node_state = state.lock().await;
                info!(
                    "neighbours:\n{}\nmds:\n{}",
                    defmt::Debug2Format(&node_state.neighbour_matrix()),
                    defmt::Debug2Format(&node_state.mds)
                );
            }
            // drain message to server queue
            while let Ok(telmsg) = MQTT_TX_CHANNEL.try_receive() {
                let msg: &[u8] = match postcard::to_slice(&telmsg, &mut serializer_buff) {
                    Ok(rs) => rs,
                    Err(_) => &[], // todo consider creating error codes and publishing to mq
                };
                // TODO consider having different channels for different types of messages
                match mqtt_session
                    .publish(Publication::new(topic.as_str(), msg))
                    .await
                {
                    Ok(_) => {}
                    Err(minimq::PubError::Session(e)) => {
                        error!(
                            "Connection failed, reconnecting ... {}",
                            defmt::Debug2Format(&e)
                        );
                        break;
                    }
                    Err(e) => {
                        error!("Payload serialization error: {}", defmt::Debug2Format(&e));
                    }
                }
            }

            Timer::after(Duration::from_millis(500)).await; // made this much faster for
            // benchmarking
        }
    }
}
