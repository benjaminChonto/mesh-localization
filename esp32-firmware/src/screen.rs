use crate::state::MAX_SWARM_SIZE;
use crate::topology::NODE_ID_LEN;
extern crate alloc;
use alloc::boxed::Box;
use alloc::format;
use alloc::vec;
use core::cmp::Ordering;
use defmt::error;
use display_interface::DisplayError;
use embedded_graphics::pixelcolor::BinaryColor;
use fixed::types::I16F16;
use hashbrown::HashMap;
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

type NodeIds = HashMap<[u8; 6], heapless::String<NODE_ID_LEN>>;

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

#[allow(clippy::too_many_arguments)]
pub fn render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    node_id: &str,
    node_ids: &NodeIds,
    path: Option<&Vec<[u8; 6], MAX_SWARM_SIZE>>,
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = try_render_mds(terminal, macs, distances, mds, id, node_id, node_ids, path) {
        error!("screen::render_mds: {}", defmt::Debug2Format(&e));
    }
}

#[allow(clippy::too_many_arguments)]
fn try_render_mds<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    node_id: &str,
    node_ids: &NodeIds,
    path: Option<&Vec<[u8; 6], MAX_SWARM_SIZE>>,
) -> Result<(), RenderError>
where
    I: embedded_hal::i2c::I2c + 'static,
{
    terminal.clear()?;
    terminal.draw(|frame| draw_mds(frame, macs, distances, mds, id, node_id, node_ids, path))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_mds(
    frame: &mut Frame,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    mds: &Vec<Vec<I16F16, 2>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    node_id: &str,
    node_ids: &NodeIds,
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

    // Collect non-self visible nodes, translated so we are at the canvas origin
    let mut visible = [(0.0f64, 0.0f64); MAX_SWARM_SIZE];
    let mut visible_labels = [0usize; MAX_SWARM_SIZE];
    let mut count = 0usize;

    let push_node = |visible: &mut [(f64, f64); MAX_SWARM_SIZE],
                     visible_labels: &mut [usize; MAX_SWARM_SIZE],
                     count: &mut usize,
                     idx: usize| {
        if *count < MAX_SWARM_SIZE && idx < mds.len() {
            visible[*count] = (
                (mds[idx][0] - centerpoint[0]).to_num::<f64>(),
                (mds[idx][1] - centerpoint[1]).to_num::<f64>(),
            );
            visible_labels[*count] = idx;
            *count += 1;
        }
    };

    if let Some(path) = path {
        for &mac in path.iter() {
            if mac == *id {
                continue;
            }
            if let Some(idx) = macs.iter().position(|&m| m == mac) {
                push_node(&mut visible, &mut visible_labels, &mut count, idx);
            }
        }
    } else {
        // Default: show all nodes
        for i in 0..mds.len() {
            if i == current_node_index {
                continue;
            }
            push_node(&mut visible, &mut visible_labels, &mut count, i);
        }
    }

    // Resolve target index + direct distance for labelling and bounds
    let target_info: Option<(usize, f32)> = path.and_then(|p| p.last()).and_then(|&tgt_mac| {
        let idx = macs.iter().position(|&m| m == tgt_mac)?;
        let dist =
            if current_node_index < distances.len() && idx < distances[current_node_index].len() {
                distances[current_node_index][idx].to_num::<f32>()
            } else {
                0.0
            };
        Some((idx, dist))
    });

    let title = match target_info {
        Some((idx, _)) => {
            let tgt_label = macs
                .get(idx)
                .and_then(|m| node_ids.get(m))
                .map(|s| s.as_str())
                .unwrap_or("?");
            format!("MDS [{}] {} →{}", node_id, macs.len(), tgt_label)
        }
        None => format!("MDS [{}] {}", node_id, macs.len()),
    };

    // Scale to always keep the target on screen (we are at origin after translation)
    let bound: f64 = if let Some((target_idx, _)) = target_info {
        if target_idx < mds.len() {
            let tx = (mds[target_idx][0] - centerpoint[0]).to_num::<f64>().abs();
            let ty = (mds[target_idx][1] - centerpoint[1]).to_num::<f64>().abs();
            (tx.max(ty) * 1.3).max(2.0)
        } else {
            10.0
        }
    } else {
        10.0
    };

    // Pre-compute owned labels so the 'static paint closure doesn't borrow non-static refs.
    let mut labels: [alloc::string::String; MAX_SWARM_SIZE] =
        core::array::from_fn(|_| alloc::string::String::new());
    for i in 0..count {
        let mac = macs[visible_labels[i]];
        labels[i] =
            alloc::string::String::from(node_ids.get(&mac).map(|s| s.as_str()).unwrap_or("?"));
    }
    let canvas = Canvas::default()
        .block(Block::bordered().title(title.as_str()))
        .marker(Marker::Dot)
        .x_bounds([-bound, bound])
        .y_bounds([-bound, bound])
        .paint(move |ctx| {
            // All non-self nodes as dots
            ctx.draw(&Points {
                coords: &visible[..count],
                color: Color::White,
            });

            // Self as "x" at origin
            ctx.print(0.0, 0.0, "x");

            // Only label the targeted node
            if let Some((target_idx, dist)) = target_info {
                for (i, &(x, y)) in visible[..count].iter().enumerate() {
                    if visible_labels[i] == target_idx {
                        ctx.print(x, y, format!("{} {:.1}m", labels[i], dist));
                        break;
                    }
                }
            }
        });

    frame.render_widget(canvas, frame.area());
}

#[allow(clippy::too_many_arguments)]
pub fn render_table<'d, I>(
    terminal: &mut ScreenTerminal<'d, I>,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    node_id: &str,
    node_ids: &NodeIds,
    estimated_macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    target_mac: Option<[u8; 6]>,
) where
    I: embedded_hal::i2c::I2c + 'static,
{
    if let Err(e) = terminal.draw(|frame| {
        draw_table(
            frame,
            macs,
            distances,
            id,
            node_id,
            node_ids,
            estimated_macs,
            target_mac,
        )
    }) {
        error!("screen::render_table: {}", defmt::Debug2Format(&e));
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_table(
    frame: &mut Frame,
    macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    distances: &Vec<Vec<I16F16, MAX_SWARM_SIZE>, MAX_SWARM_SIZE>,
    id: &[u8; 6],
    node_id: &str,
    node_ids: &NodeIds,
    estimated_macs: &Vec<[u8; 6], MAX_SWARM_SIZE>,
    target_mac: Option<[u8; 6]>,
) {
    if macs.is_empty() {
        let paragraph =
            Paragraph::new("Waiting for nodes...").block(Block::bordered().title("Nodes"));
        frame.render_widget(paragraph, frame.area());
        return;
    }

    let self_idx = macs.iter().position(|&m| m == *id).unwrap_or(0);
    let target_idx = target_mac.and_then(|t| macs.iter().position(|&m| m == t));

    // Collect all non-self nodes with their distance (None = unknown)
    let mut candidates: alloc::vec::Vec<(usize, Option<I16F16>)> = alloc::vec::Vec::new();
    for i in 0..macs.len() {
        if i == self_idx {
            continue;
        }
        let dist = if self_idx < distances.len() && i < distances[self_idx].len() {
            let d = distances[self_idx][i];
            if d < I16F16::MAX { Some(d) } else { None }
        } else {
            None
        };
        candidates.push((i, dist));
    }

    // Sort: target first, then known distances ascending, then unknowns
    candidates.sort_unstable_by(|a, b| {
        let a_target = target_idx.is_some_and(|t| t == a.0);
        let b_target = target_idx.is_some_and(|t| t == b.0);
        if a_target != b_target {
            return if a_target {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        match (a.1, b.1) {
            (Some(da), Some(db)) => da.partial_cmp(&db).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    });

    let rows: alloc::vec::Vec<Row> = candidates
        .iter()
        .map(|&(idx, dist)| {
            let is_target = target_idx.is_some_and(|t| t == idx);
            let peer_label = macs
                .get(idx)
                .and_then(|m| node_ids.get(m))
                .map(|s| s.as_str())
                .unwrap_or("?");
            let id_cell = if is_target {
                format!(">{}", peer_label)
            } else {
                format!(" {}", peer_label)
            };
            let is_estimated = estimated_macs.contains(&macs[idx]);
            let dist_cell = match dist {
                Some(d) if is_estimated => format!("~{:.1}m", d.to_num::<f32>()),
                Some(d) => format!(" {:.1}m", d.to_num::<f32>()),
                None => "   ?".into(),
            };
            Row::new(vec![id_cell, dist_cell])
        })
        .collect();

    let title = format!("Nodes [{}]", node_id);
    let table = Table::new(rows, [Constraint::Length(6), Constraint::Min(6)])
        .block(Block::bordered().title(title.as_str()));
    frame.render_widget(table, frame.area());
}
