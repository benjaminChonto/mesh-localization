use std::{
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

use ratatui::{
    DefaultTerminal, Frame, layout::{Constraint, Direction, Layout}, style::{Color, Style, Stylize}, symbols::Marker, widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph}
};
use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};

fn read_mqtt(tx: &Sender<(String, heapless::Vec<heapless::Vec<f32, 2>, 10>)>) {
    let mqtt_opt = MqttOptions::new("mesh-ui", "localhost", 1883);
    let (client, mut connection) = Client::new(mqtt_opt, 10);
    let _ = client
        .subscribe("mds", QoS::AtLeastOnce)
        .inspect_err(|e| eprintln!("Could not subscribe to topic: {e}"));

    for event in connection.iter() {
        match event {
            Ok(Event::Incoming(Incoming::Publish(packet))) => {
                let data: heapless::Vec<heapless::Vec<f32, 2>, 10> =
                    postcard::from_bytes(&packet.payload).unwrap();
                // let payload = String::from_utf8_lossy(&packet.payload);
                let _ = tx.send((String::new(), data));
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send((format!("MQTT error: {e}"), heapless::Vec::new()));
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

fn app(terminal: &mut DefaultTerminal, rx: &Receiver<(String, heapless::Vec<heapless::Vec<f32, 2>, 10>)>) -> std::io::Result<()> {
    let mut data: Option<(String, heapless::Vec<heapless::Vec<f32, 2>, 10>)>;

    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(msg) => {
                data = Some(msg);
                terminal.draw(|frame| render_split(frame, data.as_ref()))?;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(e) => {
                data = Some((format!("MPSC error: {e}"), heapless::Vec::new()));
                terminal.draw(|frame| render_split(frame, data.as_ref()))?;
            }
        }

        if crossterm::event::poll(Duration::from_millis(0))?
            && crossterm::event::read()?.is_key_press()
        {
            break Ok(());
        }
    }
}

fn render_split(
    frame: &mut Frame,
    logs: Option<&(String, heapless::Vec<heapless::Vec<f32, 2>, 10>)>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(frame.area());

    if let Some(logs) = logs {
        render_chart(frame, logs, chunks[0]);
    }
    render_messages(frame, logs, chunks[1]);
}

fn render_messages(
    frame: &mut Frame,
    logs: Option<&(String, heapless::Vec<heapless::Vec<f32, 2>, 10>)>,
    area: ratatui::layout::Rect,
) {
    let message = match logs {
        Some((text, _)) if !text.trim().is_empty() => text.clone(),
        _ => "No MQTT message received yet.".to_string(),
    };

    let paragraph = Paragraph::new(message)
        .block(Block::new().title("MQTT message").borders(Borders::ALL));

    frame.render_widget(paragraph, area);
}

fn render_chart(
    frame: &mut Frame,
    logs: &(String, heapless::Vec<heapless::Vec<f32, 2>, 10>),
    area: ratatui::layout::Rect,
) {
    let data: Vec<(f64, f64)> = logs
        .1
        .iter()
        .filter_map(|p| {
            if p.len() == 2 {
                Some((p[0] as f64, p[1] as f64))
            } else {
                None
            }
        })
        .collect();

    if data.is_empty() {
        return;
    }

    let x_bounds = data.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |acc, p| {
        [acc[0].min(p.0) - 10f64, acc[1].max(p.0) + 3f64]
    });

    let y_bounds = data.iter().fold([f64::INFINITY, f64::NEG_INFINITY], |acc, p| {
        [acc[0].min(p.1) - 10f64, acc[1].max(p.1) +3f64]
    });

    let dataset = Dataset::default()
        .name("MDS")
        .marker(Marker::Block)
        .graph_type(GraphType::Scatter)
        .style(Style::new().bg(Color::LightBlue))
        .data(&data);

    let x_axis = Axis::default()
        .title("X".blue())
        .bounds(x_bounds);

    let y_axis = Axis::default()
        .title("Y".blue())
        .bounds(y_bounds);

    let chart = Chart::new(vec![dataset])
        .x_axis(x_axis)
        .y_axis(y_axis);

    frame.render_widget(chart, area);
}