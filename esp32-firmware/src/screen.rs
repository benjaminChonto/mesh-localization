use crate::state::MAX_SWARM_SIZE;
extern crate alloc;
use alloc::boxed::Box;
use alloc::format;
use display_interface::DisplayError;
use embedded_graphics::pixelcolor::BinaryColor;
use fixed::types::I16F16;
use heapless::Vec;
use defmt::error;
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
        ..EmbeddedBackendConfig::default()
    };
    Terminal::new(EmbeddedBackend::new(display, config))
}

pub fn render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = try_render_mds(terminal, macs, distances, mds, id) {
        error!("screen::render_mds: {}", defmt::Debug2Format(&e));
    }
}

fn try_render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
) -> Result<(), RenderError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    terminal.clear()?;
    terminal.draw(|frame| draw_mds(frame, macs, distances, mds, id))?;
    Ok(())
}

fn draw_mds(
    frame: &mut Frame,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
) {
    if mds.is_empty() {
        let paragraph =
            Paragraph::new("Waiting for nodes...").block(Block::bordered().title("MDS"));
        frame.render_widget(paragraph, frame.area());
        return;
    }
    let mut coords = [(0.0f64, 0.0f64); MAX_SWARM_SIZE];
    let current_node_index = macs.iter().position(|&mac| mac == *id).unwrap_or(0);
    let centerpoint = if current_node_index < mds.len() {
        &mds[current_node_index]
    } else {
        &mds[0]
    };

    for (i, point) in mds.iter().enumerate() {
        coords[i] = (
            point[0].to_num::<f64>() - centerpoint[0].to_num::<f64>(),
            point[1].to_num::<f64>() - centerpoint[1].to_num::<f64>(),
        );
    }

    let coords = &coords[..mds.len()];

    let x_abs_max = coords.iter().map(|&(x, _)| x.abs()).fold(0.0f64, f64::max);
    let y_abs_max = coords.iter().map(|&(_, y)| y.abs()).fold(0.0f64, f64::max);
    let x_pad = x_abs_max * 0.15 + 0.01;
    let y_pad = y_abs_max * 0.15 + 0.01;

    // canvas widget
    let canvas = Canvas::default()
        .block(Block::bordered().title("MDS"))
        .marker(Marker::Dot)
        .x_bounds([-(x_abs_max + x_pad), x_abs_max + x_pad])
        .y_bounds([-(y_abs_max + y_pad), y_abs_max + y_pad])
        .paint(|ctx| {
            ctx.draw(&Points {
                coords,
                color: Color::White,
            });
            for (i, &(x, y)) in coords.iter().enumerate() {
                ctx.print(x, y, format!("{i}"));
            }
            for (i, &(x, y)) in coords.iter().enumerate() {
                let line = if (x, y) == (0.0, 0.0) {
                    continue;
                } else {
                    format!("{:.2}m", distances[current_node_index][i].to_num::<f64>())
                };
                ctx.print(x, y - 0.02, line);
            }
        });

    // mds widget
    let mds_area = frame.area();
    let mds_height = mds_area.height - 1; // leave space for info
    let mds_area = ratatui::layout::Rect {
        x: mds_area.x,
        y: mds_area.y,
        width: mds_area.width,
        height: mds_height,
    };

    let paragraph = Paragraph::new(format!("Nodes: {}", macs.len()));
    let info_area = frame.area();
    let info_height = 1;
    let info_area = ratatui::layout::Rect {
        x: info_area.x,
        y: info_area.y + info_area.height - info_height,
        width: info_area.width,
        height: info_height,
    };

    frame.render_widget(canvas, mds_area);
    frame.render_widget(paragraph, info_area);
}
