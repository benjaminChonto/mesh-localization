use std::{
    collections::{HashMap, VecDeque},
    env,
    fs::{self, File},
    io::{self, BufWriter, Write},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Layout},
    style::{Color, Style, Stylize},
    symbols::Marker,
    widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph, Row, Table},
};
use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};
use shared::{MdsResult, PerformanceMetrics, TelemetryMessage};

const MOVING_AVG_WINDOW: usize = 20;

struct MovingAvg {
    buf: VecDeque<u32>,
}

impl MovingAvg {
    fn new() -> Self {
        Self {
            buf: VecDeque::new(),
        }
    }

    fn push(&mut self, value: u32) {
        if self.buf.len() >= MOVING_AVG_WINDOW {
            self.buf.pop_front();
        }
        self.buf.push_back(value);
    }

    #[allow(clippy::cast_precision_loss)]
    fn avg(&self) -> f64 {
        if self.buf.is_empty() {
            return 0.0;
        }
        self.buf.iter().sum::<u32>() as f64 / self.buf.len() as f64
    }
}

struct NodePerfState {
    latest: PerformanceMetrics,
    broadcast_avg: MovingAvg,
    process_avg: MovingAvg,
    calculate_avg: MovingAvg,
}

impl NodePerfState {
    fn new(metrics: PerformanceMetrics) -> Self {
        let mut state = Self {
            latest: metrics,
            broadcast_avg: MovingAvg::new(),
            process_avg: MovingAvg::new(),
            calculate_avg: MovingAvg::new(),
        };
        state.update(metrics);
        state
    }

    fn update(&mut self, metrics: PerformanceMetrics) {
        self.latest = metrics;
        self.broadcast_avg.push(metrics.broadcast_hello_cycles);
        self.process_avg.push(metrics.process_packet_hello_cycles);
        self.calculate_avg.push(metrics.calc_state_mds_total_cycles);
    }
}

enum UiEvent {
    Positions(MdsResult),
    Perf {
        node_id: String,
        metrics: PerformanceMetrics,
    },
    Error(String),
}

struct AppState {
    positions: Option<MdsResult>,
    perf_nodes: HashMap<String, NodePerfState>,
    status: String,
}

impl AppState {
    fn new() -> Self {
        Self {
            positions: None,
            perf_nodes: HashMap::new(),
            status: "Waiting for data...".to_string(),
        }
    }

    fn handle(&mut self, event: UiEvent) {
        match event {
            UiEvent::Positions(pos) => {
                self.positions = Some(pos);
            }
            UiEvent::Perf { node_id, metrics } => {
                self.perf_nodes
                    .entry(node_id)
                    .and_modify(|s| s.update(metrics))
                    .or_insert_with(|| NodePerfState::new(metrics));
            }
            UiEvent::Error(msg) => {
                self.status = msg;
            }
        }
    }
}

/// Milliseconds since the Unix epoch, used as the timestamp column in the CSV log.
fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Appends every received performance sample to a CSV file so runs can be
/// analyzed offline (e.g. with pandas/Excel). Each row carries both the raw
/// CPU cycle counts (the precise on-chip measurement) and their microsecond
/// equivalents derived from the firmware clock rate.
struct PerfCsvLogger {
    writer: BufWriter<File>,
    path: String,
}

impl PerfCsvLogger {
    fn new() -> io::Result<Self> {
        // Ensure the log directory exists; `File::create` does not create parents.
        fs::create_dir_all("logs")?;
        // Tag the filename with the node count of this run when `MESH_NODES` is
        // set, so logs are self-identifying (e.g. `perf-log-5nodes-<ts>.csv`).
        let node_tag = match env::var("MESH_NODES") {
            Ok(n) if !n.is_empty() => format!("{n}nodes-"),
            _ => String::new(),
        };
        let path = format!("logs/perf-log-{}{}.csv", node_tag, unix_millis());
        let mut writer = BufWriter::new(File::create(&path)?);
        writeln!(
            writer,
            "unix_ms,node_id,\
             broadcast_hello_cycles,broadcast_topo_cycles,\
             process_packet_hello_cycles,process_packet_topo_cycles,\
             calc_state_mds_total_cycles,calc_state_kabsch_cycles,\
             calc_state_mds_iter_cycles,calc_state_routing_update_cycles,\
             calc_state_build_neighbors_cycles,\
             update_screen_mds_cycles,update_screen_table_cycles,\
             broadcast_hello_ns,broadcast_topo_ns,\
             process_packet_hello_ns,process_packet_topo_ns,\
             calc_state_mds_total_ns,calc_state_kabsch_ns,\
             calc_state_mds_iter_ns,calc_state_routing_update_ns,\
             calc_state_build_neighbors_ns,\
             update_screen_mds_ns,update_screen_table_ns"
        )?;
        writer.flush()?;
        Ok(Self { writer, path })
    }

    fn log(&mut self, node_id: &str, m: &PerformanceMetrics) -> io::Result<()> {
        writeln!(
            self.writer,
            "{},{},\
             {},{},{},{},{},{},{},{},{},{},{},\
             {:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3}",
            unix_millis(),
            node_id,
            m.broadcast_hello_cycles,
            m.broadcast_topo_cycles,
            m.process_packet_hello_cycles,
            m.process_packet_topo_cycles,
            m.calc_state_mds_total_cycles,
            m.calc_state_kabsch_cycles,
            m.calc_state_mds_iter_cycles,
            m.calc_state_routing_update_cycles,
            m.calc_state_build_neighbors_cycles,
            m.update_screen_mds_cycles,
            m.update_screen_table_cycles,
            cycles_to_ns(m.broadcast_hello_cycles as f64),
            cycles_to_ns(m.broadcast_topo_cycles as f64),
            cycles_to_ns(m.process_packet_hello_cycles as f64),
            cycles_to_ns(m.process_packet_topo_cycles as f64),
            cycles_to_ns(m.calc_state_mds_total_cycles as f64),
            cycles_to_ns(m.calc_state_kabsch_cycles as f64),
            cycles_to_ns(m.calc_state_mds_iter_cycles as f64),
            cycles_to_ns(m.calc_state_routing_update_cycles as f64),
            cycles_to_ns(m.calc_state_build_neighbors_cycles as f64),
            cycles_to_ns(m.update_screen_mds_cycles as f64),
            cycles_to_ns(m.update_screen_table_cycles as f64),
        )?;
        // Flush every row: samples arrive slowly (~20/s) and we don't want to
        // lose data if the UI is killed with Ctrl-C.
        self.writer.flush()
    }
}

fn read_mqtt(tx: &Sender<UiEvent>) {
    let mqtt_opt = MqttOptions::new("mesh-ui", "localhost", 1883);
    let (client, mut connection) = Client::new(mqtt_opt, 10);
    if let Err(e) = client.subscribe("telemetry/+", QoS::AtLeastOnce) {
        eprintln!("Could not subscribe: {e}");
    }

    // Best-effort CSV logging of performance samples; if the file can't be
    // opened we still run the UI, just without persisting metrics.
    let mut csv_logger = match PerfCsvLogger::new() {
        Ok(logger) => {
            let _ = tx.send(UiEvent::Error(format!("Logging perf to {}", logger.path)));
            Some(logger)
        }
        Err(e) => {
            let _ = tx.send(UiEvent::Error(format!("Could not open perf CSV log: {e}")));
            None
        }
    };

    for event in connection.iter() {
        match event {
            Ok(Event::Incoming(Incoming::Publish(packet))) => {
                let node_id = packet.topic.split('/').nth(1).unwrap_or("?").to_string();
                match postcard::from_bytes::<TelemetryMessage<'_>>(packet.payload.as_ref()) {
                    Ok(TelemetryMessage::Mds(positions)) if node_id == "0" => {
                        let _ = tx.send(UiEvent::Positions(positions));
                    }
                    Ok(TelemetryMessage::Mds(_)) => {}
                    Ok(TelemetryMessage::Perf(metrics)) => {
                        if let Some(logger) = csv_logger.as_mut()
                            && let Err(e) = logger.log(&node_id, &metrics)
                        {
                            eprintln!("perf CSV write failed: {e}");
                        }
                        let _ = tx.send(UiEvent::Perf { node_id, metrics });
                    }
                    Ok(TelemetryMessage::Log { .. }) => {}
                    Err(e) => {
                        let _ = tx.send(UiEvent::Error(format!("Decode error: {e}")));
                    }
                }
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(UiEvent::Error(format!("MQTT error: {e}")));
                break;
            }
        }
    }
}

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || read_mqtt(&tx));

    ratatui::run(|terminal| app(terminal, &rx))?;
    Ok(())
}

fn app(terminal: &mut DefaultTerminal, rx: &Receiver<UiEvent>) -> std::io::Result<()> {
    let mut state = AppState::new();

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => state.handle(event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(e) => state.status = format!("MPSC error: {e}"),
        }

        terminal.draw(|frame| render(frame, &state))?;

        if crossterm::event::poll(Duration::from_millis(0))?
            && crossterm::event::read()?.is_key_press()
        {
            break Ok(());
        }
    }
}

fn render(frame: &mut Frame, state: &AppState) {
    let [chart_area, bottom_area] =
        Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
            .areas(frame.area());

    let [perf_area, status_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .areas(bottom_area);

    render_chart(frame, state.positions.as_ref(), chart_area);
    render_perf(frame, &state.perf_nodes, perf_area);
    render_status(frame, &state.status, status_area);
}

fn render_chart(frame: &mut Frame, positions: Option<&MdsResult>, area: ratatui::layout::Rect) {
    let Some(positions) = positions else {
        frame.render_widget(
            Paragraph::new("Waiting for MDS data...")
                .block(Block::bordered().title("Node positions")),
            area,
        );
        return;
    };

    let data: Vec<(f64, f64)> = positions
        .iter()
        .filter_map(|p| {
            if p.len() == 2 {
                // I16F16 raw bits / 65536 gives the float value without needing the fixed crate
                let x = p[0].to_bits() as f64 / 65536.0;
                let y = p[1].to_bits() as f64 / 65536.0;
                Some((x, y))
            } else {
                None
            }
        })
        .collect();

    if data.is_empty() {
        frame.render_widget(
            Paragraph::new("No position data").block(Block::bordered().title("Node positions")),
            area,
        );
        return;
    }

    let x_bounds = data
        .iter()
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| {
            [lo.min(p.0) - 10.0, hi.max(p.0) + 3.0]
        });
    let y_bounds = data
        .iter()
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], p| {
            [lo.min(p.1) - 10.0, hi.max(p.1) + 3.0]
        });

    let dataset = Dataset::default()
        .name("MDS")
        .marker(Marker::Block)
        .graph_type(GraphType::Scatter)
        .style(Style::new().bg(Color::LightBlue))
        .data(&data);

    let chart = Chart::new(vec![dataset])
        .block(Block::bordered().title("Node positions"))
        .x_axis(Axis::default().title("X".blue()).bounds(x_bounds))
        .y_axis(Axis::default().title("Y".blue()).bounds(y_bounds));

    frame.render_widget(chart, area);
}

fn cycles_to_ns(cycles: f64) -> f64 {
    cycles / (shared::CPU_CLOCK_HZ as f64 / 1_000_000_000.0)
}

fn time_display(current: u32, avg: f64) -> String {
    // Compute in nanoseconds for precision, then display as milliseconds (3 dp).
    format!(
        "{:.3}ms / {:.3}ms",
        cycles_to_ns(current as f64) / 1_000_000.0,
        cycles_to_ns(avg) / 1_000_000.0
    )
}

fn render_perf(
    frame: &mut Frame,
    nodes: &HashMap<String, NodePerfState>,
    area: ratatui::layout::Rect,
) {
    let block = Block::bordered().title("Performance (cur / 20-sample avg)");

    if nodes.is_empty() {
        frame.render_widget(Paragraph::new("No nodes yet").block(block), area);
        return;
    }

    let mut node_ids: Vec<&str> = nodes.keys().map(String::as_str).collect();
    node_ids.sort_unstable();

    let header = Row::new([
        "Node",
        "broadcast_clone",
        "process_packet",
        "calculate_state",
    ])
    .style(Style::new().bold());

    let rows: Vec<Row> = node_ids
        .iter()
        .map(|id| {
            let s = &nodes[*id];
            Row::new([
                (*id).to_string(),
                time_display(s.latest.broadcast_hello_cycles, s.broadcast_avg.avg()),
                time_display(s.latest.process_packet_hello_cycles, s.process_avg.avg()),
                time_display(s.latest.calc_state_mds_total_cycles, s.calculate_avg.avg()),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(6),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ];

    frame.render_widget(Table::new(rows, widths).header(header).block(block), area);
}

fn render_status(frame: &mut Frame, status: &str, area: ratatui::layout::Rect) {
    frame.render_widget(
        Paragraph::new(status.to_owned()).block(Block::bordered().title("Status")),
        area,
    );
}
