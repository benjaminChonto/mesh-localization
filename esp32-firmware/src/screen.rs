use mousefood::prelude::*;
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, Terminal};
use ssd1306::mode::BufferedGraphicsMode;
use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};

pub fn init<I>(
    i2c: I,
) -> Ssd1306<I2CInterface<I>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>
where
    I: embedded_hal::i2c::I2c,
{
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init().unwrap();
    display
}

pub fn render(
    display: &mut Ssd1306<
        I2CInterface<impl embedded_hal::i2c::I2c + 'static>,
        DisplaySize128x64,
        BufferedGraphicsMode<DisplaySize128x64>,
    >,
) {
    let backend = EmbeddedBackend::new(display, EmbeddedBackendConfig::default());
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.clear().unwrap();
    terminal.draw(draw).unwrap();
    terminal.flush().unwrap();
}

fn draw(frame: &mut Frame) {
    let block = Block::bordered().title("Mousefood");
    let paragraph = Paragraph::new("Hello from Mousefood!").block(block);
    frame.render_widget(paragraph, frame.area());
}
