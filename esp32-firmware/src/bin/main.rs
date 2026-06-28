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
use esp_hal::Blocking;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::i2c::master::{Config as I2cConfig, I2c};
use esp_hal::timer::timg::TimerGroup;
use esp_radio::esp_now::{EspNowReceiver, EspNowSender};
use esp32_firmware::mds::MDS;
use esp32_firmware::routing;
use esp32_firmware::screen;
use esp32_firmware::state::{MAX_SWARM_SIZE, NodeState};
use esp32_firmware::topology::{Packet, Topology};
use esp32_firmware::utils::{
    ID, MDS_MAX_SIZE, MQTT_TX_CHANNEL_SIZE, NETWORK_RETRIES,
    RX_CHANNEL_SIZE, SEND_TELEMETRY, TX_CHANNEL_SIZE, cpu_cycles,
};
use esp32_firmware::wificonfig::{IP_ADDR, WIFI_PASS, WIFI_SSID};
use heapless::Vec;
use minimq::{Buffers, ConfigBuilder, Publication, Session};
use shared::{I16F16, PerformanceMetrics, TelemetryMessage};
use static_cell::StaticCell;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

const HEAP_SIZE: usize = 128 * 1024;
const _DUMMY_MSG: [u8; 6] = [0u8; 6];
const BROADCAST: [u8; 6] = [0xff; 6];

static STATE: StaticCell<Mutex<CriticalSectionRawMutex, NodeState>> = StaticCell::new();
static ROUTE: StaticCell<Mutex<CriticalSectionRawMutex, Option<Vec<[u8; 6], MAX_SWARM_SIZE>>>> =
    StaticCell::new();
static METRICS: StaticCell<Mutex<CriticalSectionRawMutex, PerformanceMetrics>> = StaticCell::new();
static DISPLAY: StaticCell<screen::Display<I2c<'static, Blocking>>> = StaticCell::new();
static TC_TOPOLOGY: StaticCell<Mutex<CriticalSectionRawMutex, Topology>> = StaticCell::new();
static STACK_RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
static RX_CHANNEL: Channel<CriticalSectionRawMutex, RxPacket, RX_CHANNEL_SIZE> = Channel::new();
static TX_CHANNEL: Channel<CriticalSectionRawMutex, TxPacket, TX_CHANNEL_SIZE> = Channel::new();
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

pub struct TxPacket {
    pub dst: [u8; 6],
    pub len: usize,
    pub data: [u8; 256],
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface<'static>>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn tx_task(mut tx: EspNowSender<'static>) {
    loop {
        let packet = TX_CHANNEL.receive().await;
        match tx.send(&packet.dst, &packet.data[..packet.len]) {
            Ok(waiter) => {
                let _ = waiter.wait();
            }
            Err(e) => {
                warn!("TX failed: {:?}", e);
            }
        }
    }
}

#[embassy_executor::task]
async fn broadcast_hello(state: &'static Mutex<CriticalSectionRawMutex, NodeState>) {
    loop {
        // Measure in raw CPU cycles for maximum precision (see `cpu_cycles`).
        let start_cycles = cpu_cycles();
        let distances = {
            state.lock().await.expire_stale();
            let node_state = state.lock().await;
            node_state.neighbours.get(&node_state.mac).cloned()
        };
        let _finish = cpu_cycles().wrapping_sub(start_cycles);

        let packet = Packet::Hello(distances.unwrap_or_default());
        let mut data = [0u8; 256];
        // Extract length first so the &mut borrow on data is dropped before we move data.
        if let Ok(len) = postcard::to_slice(&packet, &mut data).map(|s| s.len())
            && TX_CHANNEL
                .try_send(TxPacket {
                    dst: BROADCAST,
                    len,
                    data,
                })
                .is_err()
        {
            warn!("Hello dropped: TX channel full");
        }

        Timer::after_millis(1000).await;
    }
}

#[embassy_executor::task]
async fn broadcast_tc(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    topology: &'static Mutex<CriticalSectionRawMutex, Topology>,
) {
    use esp32_firmware::state::MAX_SWARM_SIZE;
    loop {
        Timer::after(Duration::from_secs(5)).await;

        let neighbors: Vec<[u8; 6], MAX_SWARM_SIZE> = {
            let node_state = state.lock().await;
            node_state
                .neighbours
                .get(&node_state.mac)
                .map(|m| m.keys().copied().collect())
                .unwrap_or_default()
        };

        // Drop the topology guard before serializing so we don't hold the lock
        // across the send path.
        let packet = {
            let mut topo = topology.lock().await;
            topo.expire_stale();
            Packet::Tc(topo.generate_tc_message(neighbors))
        };

        let mut data = [0u8; 256];
        if let Ok(len) = postcard::to_slice(&packet, &mut data).map(|s| s.len())
            && TX_CHANNEL
                .try_send(TxPacket {
                    dst: BROADCAST,
                    len,
                    data,
                })
                .is_err()
        {
            warn!("TC dropped: TX channel full");
        }
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

#[allow(clippy::large_stack_frames)]
#[embassy_executor::task]
async fn process_packet(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>,
    topology: &'static Mutex<CriticalSectionRawMutex, Topology>,
) {
    let mut fwd_buf = [0u8; 256];
    loop {
        let rx = RX_CHANNEL.receive().await;
        let start_cycles = cpu_cycles();
        match postcard::from_bytes::<Packet>(&rx.data[..rx.len]) {
            Ok(Packet::Hello(distances)) => {
                let mut node_state = state.lock().await;
                let mac = node_state.mac;
                node_state.update_distance_from_self(mac, rx.src, rx.rssi);
                node_state.update_measurements_from_neighbor(rx.src, distances);
            }
            Ok(Packet::Tc(tc)) => {
                {
                    let mut node_state = state.lock().await;
                    let mac = node_state.mac;
                    node_state.update_distance_from_self(mac, rx.src, rx.rssi);
                }
                let forward = topology.lock().await.process_tc_message(
                    tc.origin_mac,
                    tc.neighbors.clone(),
                    tc.sequence,
                );

                if forward && let Ok(msg) = postcard::to_slice(&Packet::Tc(tc), &mut fwd_buf) {
                    let len = msg.len();
                    let mut data = [0u8; 256];
                    data[..len].copy_from_slice(msg);
                    if TX_CHANNEL
                        .try_send(TxPacket {
                            dst: BROADCAST,
                            len,
                            data,
                        })
                        .is_err()
                    {
                        warn!("TC forward dropped: TX channel full");
                    }
                }
            }
            Err(e) => {
                error!("Failed to parse packet: {}", defmt::Debug2Format(&e));
            }
        }
        let finish = cpu_cycles().wrapping_sub(start_cycles);
        {
            perf.lock().await.process_packet_cycles = finish;
        }
    }
}

#[embassy_executor::task]
async fn expire_stale_neighbors(state: &'static Mutex<CriticalSectionRawMutex, NodeState>) {
    loop {
        Timer::after_secs(1).await;
        state.lock().await.expire_stale();
    }
}

// TODO idk if this will overflow
#[allow(clippy::large_stack_frames)]
#[embassy_executor::task]
async fn calculate_state(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>,
    topology: &'static Mutex<CriticalSectionRawMutex, Topology>,
) {
    let mut mds = MDS::default();
    loop {
        let estimates = {
            let topo = topology.lock().await;
            let node_state = state.lock().await;
            routing::all_estimated_distances(&topo, &node_state.neighbours)
        };
        {
            state.lock().await.update_estimated_distances(estimates);
        }

        let (neighbour_dist, anchor) = {
            let node_state = state.lock().await;
            let dist = node_state.neighbour_matrix();
            let anchor = node_state
                .get_ordered_mac_addresses()
                .iter()
                .position(|&mac| mac == node_state.mac);
            (dist, anchor)
        };
        if neighbour_dist.iter().any(|row| row.contains(&I16F16::MAX)) {
            // Neighbour matrix is incomplete
            Timer::after_millis(5000).await;
            continue;
        }

        // WATCH OUT mds yields, these timings not accurate
        let start_cycles = cpu_cycles();
        let mds = mds.compute(neighbour_dist, anchor).await;
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

/// Cycles the target node on each button press and recomputes the Dijkstra
/// route, which `update_screen` then uses to filter the MDS display.
/// Pressing past the last node deselects (route = None, full view restored).
#[embassy_executor::task]
async fn button_task(
    mut button: Input<'static>,
    mut led: Output<'static>,
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    topology: &'static Mutex<CriticalSectionRawMutex, Topology>,
    route: &'static Mutex<CriticalSectionRawMutex, Option<Vec<[u8; 6], MAX_SWARM_SIZE>>>,
) {
    loop {
        button.wait_for_falling_edge().await;
        Timer::after_millis(20).await; // debounce
        if button.is_high() {
            continue;
        }
        led.set_low(); // LED on (active-low) while held

        // Collect all nodes except self, sorted for consistent cycling order
        let (own_mac, nodes) = {
            let node_state = state.lock().await;
            let topo = topology.lock().await;
            let own = node_state.mac;
            let mut nodes: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
            for &mac in node_state.neighbours.keys().chain(topo.topology_table().keys()) {
                if mac != own && !nodes.contains(&mac) {
                    let _ = nodes.push(mac);
                }
            }
            nodes.sort_unstable();
            (own, nodes)
        };

        if !nodes.is_empty() {
            // Advance to next target; wrap past last → None (deselect)
            let current = route.lock().await.as_ref().and_then(|p| p.last().copied());
            let next_target = match current.and_then(|c| nodes.iter().position(|&m| m == c)) {
                Some(i) if i + 1 < nodes.len() => Some(nodes[i + 1]),
                Some(_) => None, // was last node, wrap to no selection
                None => Some(nodes[0]),
            };

            // Compute path without holding both locks at once
            let neighbours_snap = state.lock().await.neighbours.clone();
            let new_route = match next_target {
                Some(t) => {
                    let topo = topology.lock().await;
                    routing::dijkstra_path(&topo, t, &neighbours_snap)
                }
                None => None,
            };
            *route.lock().await = new_route;

            info!(
                "Button: target={:?} reachable={}",
                next_target,
                route.lock().await.is_some()
            );
        }

        button.wait_for_rising_edge().await;
        led.set_high(); // LED off on release

        // suppress unused warning when own_mac is only used in the node filter
        let _ = own_mac;
    }
}

#[allow(clippy::large_stack_frames)]
#[embassy_executor::task]
async fn update_screen(
    state: &'static Mutex<CriticalSectionRawMutex, NodeState>,
    route: &'static Mutex<CriticalSectionRawMutex, Option<Vec<[u8; 6], MAX_SWARM_SIZE>>>,
    mut terminal: Option<screen::ScreenTerminal<'static, I2c<'static, Blocking>>>,
) {
    loop {
        let node_state = state.lock().await;
        let mds = node_state.mds.clone();
        let macs = node_state.get_ordered_mac_addresses();
        let distances = node_state.get_ordered_distances();
        let id = node_state.mac;
        drop(node_state);

        let path = route.lock().await;
        if let Some(ref mut terminal) = terminal {
            screen::render_mds(terminal, &macs, &distances, &mds, &id, path.as_ref());
        }
        drop(path);

        Timer::after_millis(300).await;
    }
}

#[embassy_executor::task]
async fn publish_metrics(perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics>) {
    loop {
        let _ = MQTT_TX_CHANNEL.try_send(TelemetryMessage::Perf(*perf.lock().await));
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

    let mut network_connection_failed = false;
    let mut mqtt_enabled = SEND_TELEMETRY;
    let mut mqtt_stack = None;

    if SEND_TELEMETRY {
        wifi_controller
            .set_config(&esp_radio::wifi::Config::Station(
                esp_radio::wifi::sta::StationConfig::default()
                    .with_ssid(WIFI_SSID)
                    .with_password(WIFI_PASS.into()),
            ))
            .unwrap();
        info!("Connecting to '{}'…", WIFI_SSID);
        let mut connection_retries = 0;
        while let Err(e) = wifi_controller.connect_async().await {
            if connection_retries > NETWORK_RETRIES {
                error!("WiFi connection retries exhausted");
                network_connection_failed = true;
                break;
            }
            error!("WiFi connect failed ({:?}), retrying in 5s…", e);
            connection_retries += 1;
            Timer::after(Duration::from_secs(5)).await;
        }

        if !network_connection_failed {
            info!("Associated with '{}'", WIFI_SSID);

            let (stack, runner) = embassy_net::new(
                interfaces.station,
                Config::dhcpv4(Default::default()),
                STACK_RESOURCES.init(StackResources::new()),
                u64::from(esp_hal::rng::Rng::new().random()),
            );
            spawner.spawn(net_task(runner).unwrap());
            stack.wait_config_up().await;
            mqtt_stack = Some(stack);
        } else {
            mqtt_enabled = false;
            info!("WiFi connection failed; skipping MQTT setup");
        }
    } else {
        info!("Telemetry disabled; skipping WiFi and MQTT setup");
        mqtt_enabled = false;
    }

    let esp_now = interfaces.esp_now;

    let led = Output::new(peripherals.GPIO8, Level::High, OutputConfig::default());
    let button = Input::new(peripherals.GPIO9, InputConfig::default().with_pull(Pull::Up));

    let i2c = I2c::new(peripherals.I2C0, I2cConfig::default())
        .unwrap()
        .with_sda(peripherals.GPIO0)
        .with_scl(peripherals.GPIO1);

    let display = match screen::init(i2c) {
        Ok(d) => Some(DISPLAY.init(d)),
        Err(e) => {
            info!("Failed to initialize display: {}", defmt::Debug2Format(&e));
            None
        }
    };

    let state: &'static Mutex<CriticalSectionRawMutex, NodeState> =
        STATE.init(Mutex::new(NodeState::new(mac)));

    let perf: &'static Mutex<CriticalSectionRawMutex, PerformanceMetrics> =
        METRICS.init(Mutex::new(PerformanceMetrics::new()));

    let (_, tx, rx) = esp_now.split();

    let terminal = if let Some(display) = display {
        info!("Display initialized");
        screen::init_terminal(display).ok()
    } else {
        info!("Running without display");
        None
    };

    let topology: &'static Mutex<CriticalSectionRawMutex, Topology> =
        TC_TOPOLOGY.init(Mutex::new(Topology::new(mac)));

    let route: &'static Mutex<CriticalSectionRawMutex, Option<Vec<[u8; 6], MAX_SWARM_SIZE>>> =
        ROUTE.init(Mutex::new(None));

    // Spawn tasks
    spawner.spawn(tx_task(tx).unwrap());
    spawner.spawn(broadcast_hello(state).unwrap());
    spawner.spawn(broadcast_tc(state, topology).unwrap());
    spawner.spawn(receive_packet(rx).unwrap());
    spawner.spawn(process_packet(state, perf, topology).unwrap());
    spawner.spawn(calculate_state(state, perf, topology).unwrap());
    spawner.spawn(expire_stale_neighbors(state).unwrap());
    spawner.spawn(publish_metrics(perf).unwrap());
    spawner.spawn(button_task(button, led, state, topology, route).unwrap());
    spawner.spawn(update_screen(state, route, terminal).unwrap());

    if mqtt_enabled {
        let topic = alloc::format!("telemetry/{ID}");
        let mut serializer_buff = [0u8; MDS_MAX_SIZE];
        loop {
            // MQTT Setup
            let mut rx_mqtt = [0u8; 256];
            let mut tx_mqtt = [0u8; 1024];
            let mut rx_tcp = [0u8; 256];
            let mut tx_tcp = [0u8; 1024];
            let mut mqtt_session =
                Session::new(ConfigBuilder::new(Buffers::new(&mut rx_mqtt, &mut tx_mqtt)));

            let stack = mqtt_stack.as_mut().unwrap();
            let mut socket = TcpSocket::new(*stack, &mut rx_tcp, &mut tx_tcp);
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

                // The display is rendered by the `update_screen` task; here we just log
                // the current state for debugging.
                {
                    let topo = topology.lock().await;
                    info!("topology:\n{}", defmt::Debug2Format(topo.topology_table()));
                }
                {
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
    } else {
        loop {
            {
                let topo = topology.lock().await;
                info!("topology:\n{}", defmt::Debug2Format(topo.topology_table()));
            }
            {
                let node_state = state.lock().await;
                info!(
                    "neighbours:\n{}\nmds:\n{}",
                    defmt::Debug2Format(&node_state.neighbour_matrix()),
                    defmt::Debug2Format(&node_state.mds)
                );
            }
            Timer::after(Duration::from_secs(1)).await;
        }
    }
}
