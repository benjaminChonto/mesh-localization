use crate::state::MAX_SWARM_SIZE;
extern crate alloc;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec;
use core::cmp::Ordering;
use defmt::error;
use display_interface::DisplayError;
use embedded_graphics::pixelcolor::BinaryColor;
use fixed::types::I16F16;
use heapless::Vec;
use mousefood::error::Error as RenderError;
use mousefood::fonts::mono_4x6_atlas;
use mousefood::prelude::*;
use ratatui::layout::Constraint;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Points};
use ratatui::widgets::{Block, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};
use ssd1306::mode::BufferedGraphicsMode;
use ssd1306::{I2CDisplayInterface, Ssd1306, prelude::*};

#[derive(Clone, Copy, PartialEq)]
pub enum ScreenMode {
    Mds,
    Table,
}

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
        font_regular: mono_4x6_atlas(),
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
    path: Option<&Vec<[u8; 6], MAX_SWARM_SIZE>>,
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = try_render_mds(terminal, macs, distances, mds, id, path) {
        error!("screen::render_mds: {}", defmt::Debug2Format(&e));
    }
}

fn try_render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    path: Option<&Vec<[u8; 6], MAX_SWARM_SIZE>>,
) -> Result<(), RenderError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    terminal.clear()?;
    terminal.draw(|frame| draw_mds(frame, macs, distances, mds, id, path))?;
    Ok(())
}

fn draw_mds(
    frame: &mut Frame,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    path: Option<&Vec<[u8; 6], MAX_SWARM_SIZE>>,
) {
    if mds.is_empty() {
        let paragraph =
            Paragraph::new("Waiting for nodes...").block(Block::bordered().title("MDS"));
        frame.render_widget(paragraph, frame.area());
        return;
    }

    let current_node_index = macs.iter().position(|&mac| mac == *id).unwrap_or(0);
    let centerpoint = if current_node_index < mds.len() {
        &mds[current_node_index]
    } else {
        &mds[0]
    };

    let mut visible = [(0.0f64, 0.0f64); MAX_SWARM_SIZE];
    let mut visible_labels = [0usize; MAX_SWARM_SIZE];
    let mut count = 1usize; // self is always slot 0
    visible_labels[0] = current_node_index;

    if let Some(path) = path {
        // Show only nodes on the Dijkstra path to the selected target (skip self)
        for &mac in path.iter() {
            if mac == *id || count >= MAX_SWARM_SIZE {
                continue;
            }
            if let Some(idx) = macs
                .iter()
                .position(|&m| m == mac)
                .filter(|&i| i < mds.len())
            {
                visible[count] = (
                    mds[idx][0].to_num::<f64>() - centerpoint[0].to_num::<f64>(),
                    mds[idx][1].to_num::<f64>() - centerpoint[1].to_num::<f64>(),
                );
                visible_labels[count] = idx;
                count += 1;
            }
        }
    } else {
        // Default: show the 2 closest nodes by distance
        let mut first: Option<(usize, I16F16)> = None;
        let mut second: Option<(usize, I16F16)> = None;
        if current_node_index < distances.len() {
            for (i, &dist) in distances[current_node_index].iter().enumerate() {
                if i == current_node_index || i >= mds.len() {
                    continue;
                }
                match first {
                    None => first = Some((i, dist)),
                    Some((_, d)) if dist < d => {
                        second = first;
                        first = Some((i, dist));
                    }
                    _ => match second {
                        None => second = Some((i, dist)),
                        Some((_, d)) if dist < d => second = Some((i, dist)),
                        _ => {}
                    },
                }
            }
        }
        for opt in [first, second].iter().flatten() {
            let (idx, _) = *opt;
            visible[count] = (
                mds[idx][0].to_num::<f64>() - centerpoint[0].to_num::<f64>(),
                mds[idx][1].to_num::<f64>() - centerpoint[1].to_num::<f64>(),
            );
            visible_labels[count] = idx;
            count += 1;
        }
    }

    let visible_coords = &visible[..count];
    let visible_labels = &visible_labels[..count];

    let title = match path.and_then(|p| p.last()) {
        Some(&target_mac) => match macs.iter().position(|&m| m == target_mac) {
            Some(idx) => format!("MDS {} →{}", macs.len(), idx),
            None => format!("MDS {}", macs.len()),
        },
        None => format!("MDS {}", macs.len()),
    };

    const BOUND: f64 = 10.0;

    let canvas = Canvas::default()
        .block(Block::bordered().title(title.as_str()))
        .marker(Marker::Dot)
        .x_bounds([-BOUND, BOUND])
        .y_bounds([-BOUND, BOUND])
        .paint(|ctx| {
            ctx.draw(&Points {
                coords: visible_coords,
                color: Color::White,
            });
            for (i, &(x, y)) in visible_coords.iter().enumerate() {
                ctx.print(x, y, format!("{}", visible_labels[i]));
            }
        });

    frame.render_widget(canvas, frame.area());
}

pub fn render_table<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    target_mac: Option<[u8; 6]>,
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = terminal.draw(|frame| draw_table(frame, macs, distances, id, target_mac)) {
        error!("screen::render_table: {}", defmt::Debug2Format(&e));
    }
}

fn draw_table(
    frame: &mut Frame,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    target_mac: Option<[u8; 6]>,
) {
    if macs.is_empty() {
        let paragraph =
            Paragraph::new("Waiting for nodes...").block(Block::bordered().title("Nodes"));
        frame.render_widget(paragraph, frame.area());
        return;
    }

    let self_idx = macs.iter().position(|&m| m == *id).unwrap_or(0);

    // Collect (node_index, distance) for all non-self nodes with a known distance
    let mut candidates: alloc::vec::Vec<(usize, I16F16)> = alloc::vec::Vec::new();
    if self_idx < distances.len() {
        for (i, &dist) in distances[self_idx].iter().enumerate() {
            if i != self_idx && dist < I16F16::MAX {
                candidates.push((i, dist));
            }
        }
    }
    candidates.sort_unstable_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));

    // Take 2 closest; append target if not already present
    let mut shown: alloc::vec::Vec<(usize, I16F16)> = candidates.iter().take(2).copied().collect();
    if let Some(tgt) = target_mac {
        if let Some(tgt_idx) = macs.iter().position(|&m| m == tgt) {
            if !shown.iter().any(|&(i, _)| i == tgt_idx) {
                if let Some(&dist) = candidates
                    .iter()
                    .find(|&&(i, _)| i == tgt_idx)
                    .map(|(_, d)| d)
                {
                    shown.push((tgt_idx, dist));
                }
            }
        }
    }

    let rows: alloc::vec::Vec<Row> = shown
        .iter()
        .map(|&(idx, dist)| {
            let is_target = target_mac
                .and_then(|t| macs.iter().position(|&m| m == t))
                .is_some_and(|ti| ti == idx);
            let id_cell = if is_target {
                format!("{}→", idx)
            } else {
                format!("{}", idx)
            };
            let dist_cell = format!("{:.1}m", dist.to_num::<f32>());
            Row::new(vec![id_cell, dist_cell])
        })
        .collect();

    let table = Table::new(rows, [Constraint::Length(4), Constraint::Min(6)])
        .block(Block::bordered().title("Nodes"));
    frame.render_widget(table, frame.area());
}
