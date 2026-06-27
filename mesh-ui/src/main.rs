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
        self.broadcast_avg.push(metrics.broadcast_clone_dist_cycles);
        self.process_avg.push(metrics.process_packet_cycles);
        self.calculate_avg.push(metrics.calculate_state_cycles);
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
        let path = format!("logs/perf-log-{}.csv", unix_millis());
        let mut writer = BufWriter::new(File::create(&path)?);
        writeln!(
            writer,
            "unix_ms,node_id,broadcast_clone_dist_cycles,process_packet_cycles,\
             calculate_state_cycles,broadcast_clone_dist_ns,process_packet_ns,calculate_state_ns"
        )?;
        writer.flush()?;
        Ok(Self { writer, path })
    }

    fn log(&mut self, node_id: &str, m: &PerformanceMetrics) -> io::Result<()> {
        writeln!(
            self.writer,
            "{},{},{},{},{},{:.3},{:.3},{:.3}",
            unix_millis(),
            node_id,
            m.broadcast_clone_dist_cycles,
            m.process_packet_cycles,
            m.calculate_state_cycles,
            cycles_to_ns(m.broadcast_clone_dist_cycles as f64),
            cycles_to_ns(m.process_packet_cycles as f64),
            cycles_to_ns(m.calculate_state_cycles as f64),
        )?;
        // Flush every row: samples arrive slowly (~20/s) and we don't want to
        // lose data if the UI is killed with Ctrl-C.
        self.writer.flush()
    }
}

/// Appends every received RSSI sample to a CSV file so the RSSI->distance model
/// can be tuned offline (path-loss exponent `N`, reference RSSI at 1m, smoothing
/// window size, EMA coefficient, spike thresholds). Raw, unfiltered samples are
/// logged on purpose: all filtering is replayed/varied offline, so nothing is
/// baked in here.
///
/// The file name embeds the label from the `RSSI_LOG_LABEL` env var (set it to
/// the ground-truth distance, e.g. `1m`, `5m`, `10m`) so a run in each condition
/// lands in a clearly named file. The label is also written as a column, so
/// files can be concatenated for analysis without losing the distance.
///
/// Waypoint markers (keypresses) go into the same file as `kind=marker` rows
/// sharing the `unix_ms` clock, so a stop-and-go staircase run can be aligned to
/// the samples offline (filter by `kind`) without precise per-sample timing.
struct RssiCsvLogger {
    writer: BufWriter<File>,
    path: String,
    label: String,
    count: u64,
    marker_count: u64,
}

impl RssiCsvLogger {
    fn new() -> io::Result<Self> {
        let dir = env::var("RSSI_LOG_DIR").unwrap_or_else(|_| "logs".to_string());
        let label = env::var("RSSI_LOG_LABEL").unwrap_or_else(|_| "unlabeled".to_string());
        fs::create_dir_all(&dir)?;
        let path = format!("{dir}/rssi-{label}-{}.csv", unix_millis());

        let mut writer = BufWriter::new(File::create(&path)?);
        // `kind` is `sample` or `marker`; sample rows fill src/rssi, marker rows
        // fill marker_index. label is constant per file but kept for concat.
        writeln!(writer, "unix_ms,kind,node_id,src,label,rssi,marker_index")?;
        writer.flush()?;

        Ok(Self {
            writer,
            path,
            label,
            count: 0,
            marker_count: 0,
        })
    }

    fn log(&mut self, node_id: &str, src: [u8; 6], rssi: i8) -> io::Result<()> {
        self.count += 1;
        // sample row: marker_index column left empty.
        writeln!(
            self.writer,
            "{},sample,{},{},{},{},",
            unix_millis(),
            node_id,
            mac_str(src),
            self.label,
            rssi
        )?;
        // Flush every row: samples arrive slowly and we don't want to lose data
        // if the UI is killed with Ctrl-C mid-run.
        self.writer.flush()
    }

    /// Record a waypoint marker at the current time; returns its index.
    fn mark(&mut self) -> io::Result<u64> {
        self.marker_count += 1;
        // marker row: node_id/src/rssi columns left empty.
        writeln!(
            self.writer,
            "{},marker,,,{},,{}",
            unix_millis(),
            self.label,
            self.marker_count
        )?;
        self.writer.flush()?;
        Ok(self.marker_count)
    }
}

/// Lowercase colon-separated MAC, e.g. `aa:bb:cc:dd:ee:ff`.
fn mac_str(mac: [u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn read_mqtt(tx: &Sender<UiEvent>, marker_rx: &Receiver<()>) {
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

    // Best-effort CSV logging of raw RSSI samples for offline model tuning.
    let mut rssi_logger = match RssiCsvLogger::new() {
        Ok(logger) => {
            let _ = tx.send(UiEvent::Error(format!(
                "Logging RSSI [{}] to {}",
                logger.label, logger.path
            )));
            Some(logger)
        }
        Err(e) => {
            let _ = tx.send(UiEvent::Error(format!("Could not open RSSI CSV log: {e}")));
            None
        }
    };

    for event in connection.iter() {
        // Drain any waypoint keypresses queued by the UI thread. The MQTT
        // iterator blocks between packets, so markers land on the next sample
        // (~50ms) — fine for coarse stop-and-go boundaries.
        while marker_rx.try_recv().is_ok() {
            if let Some(logger) = rssi_logger.as_mut() {
                match logger.mark() {
                    Ok(idx) => {
                        let _ = tx.send(UiEvent::Error(format!(
                            "Marker {idx} -> {}",
                            logger.path
                        )));
                    }
                    Err(e) => eprintln!("marker write failed: {e}"),
                }
            }
        }

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
                    Ok(TelemetryMessage::Rssi { src, rssi }) => {
                        if let Some(logger) = rssi_logger.as_mut() {
                            if let Err(e) = logger.log(&node_id, src, rssi) {
                                eprintln!("RSSI CSV write failed: {e}");
                            }
                            let _ = tx.send(UiEvent::Error(format!(
                                "RSSI [{}] {}<-{} n={} last={rssi}dBm | space=marker q=quit",
                                logger.label,
                                node_id,
                                mac_str(src),
                                logger.count
                            )));
                        }
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
    // UI thread -> logging thread: a unit per waypoint keypress.
    let (marker_tx, marker_rx) = mpsc::channel::<()>();
    thread::spawn(move || read_mqtt(&tx, &marker_rx));

    ratatui::run(|terminal| app(terminal, &rx, &marker_tx))?;
    Ok(())
}

fn app(
    terminal: &mut DefaultTerminal,
    rx: &Receiver<UiEvent>,
    marker_tx: &Sender<()>,
) -> std::io::Result<()> {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};

    let mut state = AppState::new();

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => state.handle(event),
            Err(RecvTimeoutError::Timeout) => {}
            Err(e) => state.status = format!("MPSC error: {e}"),
        }

        terminal.draw(|frame| render(frame, &state))?;

        if event::poll(Duration::from_millis(0))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                // q / Esc quit; space / m drop a waypoint marker into the log.
                KeyCode::Char('q') | KeyCode::Esc => break Ok(()),
                KeyCode::Char(' ') | KeyCode::Char('m') => {
                    let _ = marker_tx.send(());
                }
                _ => {}
            }
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
                time_display(s.latest.broadcast_clone_dist_cycles, s.broadcast_avg.avg()),
                time_display(s.latest.process_packet_cycles, s.process_avg.avg()),
                time_display(s.latest.calculate_state_cycles, s.calculate_avg.avg()),
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
