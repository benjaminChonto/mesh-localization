use crate::state::MAX_SWARM_SIZE;
use display_interface::DisplayError;
use embedded_graphics::pixelcolor::BinaryColor;
use heapless::Vec;
use log::error;
use mousefood::error::Error as RenderError;
use mousefood::prelude::*;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Paragraph};
use ratatui::{Frame, Terminal};
use ssd1306::mode::BufferedGraphicsMode;
use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};

pub type Display<I> =
    Ssd1306<I2CInterface<I>, DisplaySize128x64, BufferedGraphicsMode<DisplaySize128x64>>;

pub type ScreenTerminal<'d, I> = Terminal<EmbeddedBackend<'d, Display<I>, BinaryColor>>;

pub fn init<I>(i2c: I) -> Result<Display<I>, DisplayError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    let interface = I2CDisplayInterface::new(i2c);
    let mut display = Ssd1306::new(interface, DisplaySize128x64, DisplayRotation::Rotate0)
        .into_buffered_graphics_mode();
    display.init()?;
    display.clear_buffer();
    display.flush()?;
    Ok(display)
}

pub fn init_terminal<'d, I>(
    display: &'d mut Display<I>,
) -> Result<ScreenTerminal<'d, I>, RenderError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    let config = EmbeddedBackendConfig {
        flush_callback: Box::new(|d: &mut Display<I>| {
            let _ = d.flush();
        }),
        font_regular: ibm437::IBM437_9X14_NORMAL,
        ..EmbeddedBackendConfig::default()
    };
    Terminal::new(EmbeddedBackend::new(display, config))
}

pub fn render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    mds: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = try_render_mds(terminal, mds) {
        error!("screen::render_mds: {e:?}");
    }
}

fn try_render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    mds: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
) -> Result<(), RenderError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    terminal.clear()?;
    terminal.draw(|frame| draw_mds(frame, mds))?;
    Ok(())
}

fn draw_mds(frame: &mut Frame, mds: &Vec<Vec<f32, 2>, MAX_SWARM_SIZE>) {
    if mds.is_empty() {
        let paragraph =
            Paragraph::new("Waiting for nodes...").block(Block::bordered().title("MDS"));
        frame.render_widget(paragraph, frame.area());
        return;
    }

    let mut x_min = f32::MAX;
    let mut x_max = f32::MIN;
    let mut y_min = f32::MAX;
    let mut y_max = f32::MIN;

    for point in mds.iter() {
        x_min = x_min.min(point[0]);
        x_max = x_max.max(point[0]);
        y_min = y_min.min(point[1]);
        y_max = y_max.max(point[1]);
    }

    let x_pad = (x_max - x_min) * 0.15 + 0.01;
    let y_pad = (y_max - y_min) * 0.15 + 0.01;

    let mut coords = [(0.0f64, 0.0f64); MAX_SWARM_SIZE];
    for (i, point) in mds.iter().enumerate() {
        coords[i] = (point[0] as f64, point[1] as f64);
    }
    let coords = &coords[..mds.len()];

    let canvas = Canvas::default()
        .block(Block::bordered().title("MDS"))
        .marker(Marker::Block)
        .x_bounds([(x_min - x_pad) as f64, (x_max + x_pad) as f64])
        .y_bounds([(y_min - y_pad) as f64, (y_max + y_pad) as f64])
        .paint(|ctx| {
            ctx.draw(&Points {
                coords,
                color: Color::White,
            });
        });

    frame.render_widget(canvas, frame.area());
}
