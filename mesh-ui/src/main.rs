use std::{
    fmt::format,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use ratatui::{
    DefaultTerminal, Frame,
    widgets::{Block, Borders, Paragraph},
};
use rumqttc::{Client, Event, Incoming, MqttOptions, QoS};

fn read_mqtt(tx: &Sender<String>) {
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
                let _ = tx.send(format!("{data:?}"));
            }
            Ok(_) => {}
            Err(e) => {
                let _ = tx.send(format!("MQTT error: {e}"));
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

fn app(terminal: &mut DefaultTerminal, rx: &Receiver<String>) -> std::io::Result<()> {
    let mut logs: Vec<String> = Vec::new();

    loop {
        match rx.try_recv() {
            Ok(msg) => logs.push(msg),
            Err(TryRecvError::Empty) => {}
            Err(e) => logs.push(format!("error: {e}")),
        }

        terminal.draw(|frame| render(frame, &logs))?;

        if crossterm::event::poll(Duration::from_millis(0))?
            && crossterm::event::read()?.is_key_press()
        {
            break Ok(());
        }
    }
}

fn render(frame: &mut Frame, logs: &Vec<String>) {
    let text = logs.join("\n");

    let paragraph = Paragraph::new(text).block(
        Block::new()
            .title("MQTT logs - press q to quit")
            .borders(Borders::ALL),
    );

    frame.render_widget(paragraph, frame.area());
    // frame.render_widget("hello world", frame.area());
}
