use std::{
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
    widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph},
};
use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};
use shared::{MdsResult, TelemetryMessage};

enum UiEvent {
    Positions(MdsResult),
    Error(String),
}

struct AppState {
    positions: Option<MdsResult>,
    status: String,
}

impl AppState {
    fn new() -> Self {
        Self {
            positions: None,
            status: "Waiting for data...".to_string(),
        }
    }

    fn handle(&mut self, event: UiEvent) {
        match event {
            UiEvent::Positions(pos) => {
                self.positions = Some(pos);
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

/// Reads an `I16F16` fixed-point value out of its raw bit pattern without
/// pulling in the `fixed` crate: the raw bits divided by 2^16 give the float.
fn fixed_to_f64(value: shared::I16F16) -> f64 {
    f64::from(value.to_bits()) / 65536.0
}

/// Appends every MDS solution (the estimated node layout produced by node 0)
/// to a CSV file so runs can be reproduced offline with `scripts/plot_mds.py`.
///
/// The log is in long format -- one row per node per frame -- which pandas and
/// numpy slice trivially:
///
/// ```text
/// unix_ms,frame,node_idx,x,y
/// ```
///
/// `frame` is a monotonic counter (one per received MDS message) so individual
/// solutions can be separated even when two land in the same millisecond.
struct MdsCsvLogger {
    writer: BufWriter<File>,
    path: String,
    frame: u64,
}

impl MdsCsvLogger {
    fn new() -> io::Result<Self> {
        // Ensure the log directory exists; `File::create` does not create parents.
        fs::create_dir_all("logs")?;
        let path = format!("logs/mds-log-{}.csv", unix_millis());
        let mut writer = BufWriter::new(File::create(&path)?);
        writeln!(writer, "unix_ms,frame,node_idx,x,y")?;
        writer.flush()?;
        Ok(Self {
            writer,
            path,
            frame: 0,
        })
    }

    fn log(&mut self, positions: &MdsResult) -> io::Result<()> {
        let ts = unix_millis();
        for (node_idx, p) in positions.iter().enumerate() {
            if p.len() == 2 {
                writeln!(
                    self.writer,
                    "{},{},{},{:.6},{:.6}",
                    ts,
                    self.frame,
                    node_idx,
                    fixed_to_f64(p[0]),
                    fixed_to_f64(p[1]),
                )?;
            }
        }
        self.frame += 1;
        // Flush every frame: solutions arrive slowly and we don't want to lose
        // data if the UI is killed with Ctrl-C.
        self.writer.flush()
    }
}

fn read_mqtt(tx: &Sender<UiEvent>) {
    let mqtt_opt = MqttOptions::new("mesh-ui", "localhost", 1883);
    let (client, mut connection) = Client::new(mqtt_opt, 10);
    if let Err(e) = client.subscribe("telemetry/+", QoS::AtLeastOnce) {
        eprintln!("Could not subscribe: {e}");
    }

    // Best-effort CSV logging of MDS solutions; if the file can't be opened we
    // still run the UI, just without persisting the layouts.
    let mut csv_logger = match MdsCsvLogger::new() {
        Ok(logger) => {
            let _ = tx.send(UiEvent::Error(format!("Logging MDS to {}", logger.path)));
            Some(logger)
        }
        Err(e) => {
            let _ = tx.send(UiEvent::Error(format!("Could not open MDS CSV log: {e}")));
            None
        }
    };

    for event in connection.iter() {
        match event {
            Ok(Event::Incoming(Incoming::Publish(packet))) => {
                let node_id = packet.topic.split('/').nth(1).unwrap_or("?").to_string();
                match postcard::from_bytes::<TelemetryMessage<'_>>(packet.payload.as_ref()) {
                    Ok(TelemetryMessage::Mds(positions)) if node_id == "0" => {
                        if let Some(logger) = csv_logger.as_mut()
                            && let Err(e) = logger.log(&positions)
                        {
                            eprintln!("MDS CSV write failed: {e}");
                        }
                        let _ = tx.send(UiEvent::Positions(positions));
                    }
                    Ok(TelemetryMessage::Mds(_)) => {}
                    Ok(TelemetryMessage::Perf(_)) => {}
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
    let [chart_area, status_area] =
        Layout::vertical([Constraint::Percentage(80), Constraint::Percentage(20)])
            .areas(frame.area());

    render_chart(frame, state.positions.as_ref(), chart_area);
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
                Some((fixed_to_f64(p[0]), fixed_to_f64(p[1])))
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

fn render_status(frame: &mut Frame, status: &str, area: ratatui::layout::Rect) {
    frame.render_widget(
        Paragraph::new(status.to_owned()).block(Block::bordered().title("Status")),
        area,
    );
}
