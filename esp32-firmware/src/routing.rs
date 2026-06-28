extern crate alloc;

use crate::state::{MAX_SWARM_SIZE, State};
use crate::topology::Topology;
use hashbrown::HashMap;
use heapless::Vec;

fn collect_nodes(
    topology: &Topology,
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> Vec<[u8; 6], MAX_SWARM_SIZE> {
    let mut nodes: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
    for &mac in neighbours.keys() {
        if !nodes.contains(&mac) {
            let _ = nodes.push(mac);
        }
    }
    for &mac in topology.topology_table().keys() {
        if !nodes.contains(&mac) {
            let _ = nodes.push(mac);
        }
    }
    nodes
}

/// Shared Dijkstra core. Returns the ordered MAC path from own_mac → target,
/// or `None` if the target is unreachable.
fn dijkstra_inner(
    topology: &Topology,
    target: [u8; 6],
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> Option<Vec<[u8; 6], MAX_SWARM_SIZE>> {
    let own_mac = topology.own_mac();
    let all_nodes = collect_nodes(topology, neighbours);

    if !all_nodes.contains(&target) {
        return None;
    }

    let mut dist: HashMap<[u8; 6], f32> = HashMap::new();
    let mut prev: HashMap<[u8; 6], Option<[u8; 6]>> = HashMap::new();
    let mut visited: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();

    for &node in &all_nodes {
        dist.insert(node, f32::INFINITY);
        prev.insert(node, None);
    }
    dist.insert(own_mac, 0.0);

    loop {
        let current = all_nodes
            .iter()
            .filter(|n| !visited.contains(n))
            .min_by(|&&a, &&b| {
                dist.get(&a)
                    .unwrap_or(&f32::INFINITY)
                    .partial_cmp(dist.get(&b).unwrap_or(&f32::INFINITY))
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .copied();

        let current = match current {
            Some(n) if *dist.get(&n).unwrap_or(&f32::INFINITY) < f32::INFINITY => n,
            _ => break,
        };

        if current == target {
            break;
        }

        let _ = visited.push(current);

        if let Some(adj) = neighbours.get(&current) {
            for (&neighbor, state) in adj {
                if visited.contains(&neighbor) {
                    continue;
                }
                let new_dist = dist.get(&current).copied().unwrap_or(f32::INFINITY)
                    + state.dist.to_num::<f32>();
                if new_dist < *dist.get(&neighbor).unwrap_or(&f32::INFINITY) {
                    dist.insert(neighbor, new_dist);
                    prev.insert(neighbor, Some(current));
                }
            }
        }
    }

    if *dist.get(&target).unwrap_or(&f32::INFINITY) == f32::INFINITY {
        return None;
    }

    let mut path: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
    let mut cur = target;
    loop {
        let _ = path.push(cur);
        match prev.get(&cur).copied().flatten() {
            Some(p) => cur = p,
            None => break,
        }
    }
    path.as_mut_slice().reverse();
    Some(path)
}

pub fn dijkstra_path(
    topology: &Topology,
    target: [u8; 6],
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> Option<Vec<[u8; 6], MAX_SWARM_SIZE>> {
    dijkstra_inner(topology, target, neighbours)
}

pub fn dijkstra_rssi(
    topology: &Topology,
    target: [u8; 6],
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> Option<Vec<f32, MAX_SWARM_SIZE>> {
    let path = dijkstra_inner(topology, target, neighbours)?;

    let mut rssi_hops: Vec<f32, MAX_SWARM_SIZE> = Vec::new();
    for i in 0..path.len().saturating_sub(1) {
        let ema = neighbours
            .get(&path[i])
            .and_then(|adj| adj.get(&path[i + 1]))
            .map(|s| s.ema_rssi.to_num::<f32>())
            .unwrap_or(f32::NEG_INFINITY);
        let _ = rssi_hops.push(ema);
    }

    Some(rssi_hops)
}

/// Builds a symmetric adjacency from `neighbours`.
/// For each stored edge A→B, also inserts B→A, taking the minimum when both
/// directions have been measured directly.
fn build_symmetric_adjacency(
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> HashMap<[u8; 6], HashMap<[u8; 6], f32>> {
    let mut adj: HashMap<[u8; 6], HashMap<[u8; 6], f32>> = HashMap::new();
    for (&src, edges) in neighbours {
        for (&dst, state) in edges {
            let d = state.dist.to_num::<f32>();
            adj.entry(src)
                .or_insert_with(HashMap::new)
                .entry(dst)
                .and_modify(|e| {
                    if d < *e {
                        *e = d;
                    }
                })
                .or_insert(d);
            adj.entry(dst)
                .or_insert_with(HashMap::new)
                .entry(src)
                .and_modify(|e| {
                    if d < *e {
                        *e = d;
                    }
                })
                .or_insert(d);
        }
    }
    adj
}

/// Runs Dijkstra from every node in the symmetric adjacency and returns
/// estimated distances for all reachable (source, destination) pairs.
/// Used to fill in non-direct-neighbour entries in the distance matrix.
pub fn all_pairs_estimated_distances(
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> HashMap<([u8; 6], [u8; 6]), f32> {
    let adj = build_symmetric_adjacency(neighbours);
    let nodes: Vec<[u8; 6], MAX_SWARM_SIZE> = adj.keys().copied().collect();
    let mut result: HashMap<([u8; 6], [u8; 6]), f32> = HashMap::new();

    for &source in &nodes {
        let mut dist: HashMap<[u8; 6], f32> = HashMap::new();
        let mut visited: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
        for &n in &nodes {
            dist.insert(n, f32::INFINITY);
        }
        dist.insert(source, 0.0);

        loop {
            let current = nodes
                .iter()
                .filter(|n| !visited.contains(n))
                .min_by(|&&a, &&b| {
                    dist.get(&a)
                        .unwrap_or(&f32::INFINITY)
                        .partial_cmp(dist.get(&b).unwrap_or(&f32::INFINITY))
                        .unwrap_or(core::cmp::Ordering::Equal)
                })
                .copied();

            let current = match current {
                Some(n) if *dist.get(&n).unwrap_or(&f32::INFINITY) < f32::INFINITY => n,
                _ => break,
            };

            let _ = visited.push(current);

            if let Some(edges) = adj.get(&current) {
                for (&neighbor, &d) in edges {
                    if visited.contains(&neighbor) {
                        continue;
                    }
                    let new_dist =
                        dist.get(&current).copied().unwrap_or(f32::INFINITY) + d;
                    if new_dist < *dist.get(&neighbor).unwrap_or(&f32::INFINITY) {
                        dist.insert(neighbor, new_dist);
                    }
                }
            }
        }

        for (&dst, &d) in &dist {
            if dst != source && d < f32::INFINITY {
                result.insert((source, dst), d);
            }
        }
    }

    result
}

/// Runs Dijkstra from own_mac to all reachable nodes and returns the estimated
/// total distance (metres) for each. Direct neighbours are excluded because
/// their measured distance is more accurate than any estimate.
pub fn all_estimated_distances(
    topology: &Topology,
    neighbours: &HashMap<[u8; 6], HashMap<[u8; 6], State>>,
) -> HashMap<[u8; 6], f32> {
    let own_mac = topology.own_mac();
    let all_nodes = collect_nodes(topology, neighbours);

    let mut dist: HashMap<[u8; 6], f32> = HashMap::new();
    let mut visited: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();

    for &node in &all_nodes {
        dist.insert(node, f32::INFINITY);
    }
    dist.insert(own_mac, 0.0);

    loop {
        let current = all_nodes
            .iter()
            .filter(|n| !visited.contains(n))
            .min_by(|&&a, &&b| {
                dist.get(&a)
                    .unwrap_or(&f32::INFINITY)
                    .partial_cmp(dist.get(&b).unwrap_or(&f32::INFINITY))
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .copied();

        let current = match current {
            Some(n) if *dist.get(&n).unwrap_or(&f32::INFINITY) < f32::INFINITY => n,
            _ => break,
        };

        let _ = visited.push(current);

        if let Some(adj) = neighbours.get(&current) {
            for (&neighbor, state) in adj {
                if visited.contains(&neighbor) {
                    continue;
                }
                let new_dist = dist.get(&current).copied().unwrap_or(f32::INFINITY)
                    + state.dist.to_num::<f32>();
                if new_dist < *dist.get(&neighbor).unwrap_or(&f32::INFINITY) {
                    dist.insert(neighbor, new_dist);
                }
            }
        }
    }

    let direct = neighbours.get(&own_mac);
    dist.retain(|mac, &mut d| {
        *mac != own_mac && d < f32::INFINITY && direct.is_none_or(|dn| !dn.contains_key(mac))
    });

    dist
}
