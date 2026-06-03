use hashbrown::HashMap;
use heapless::Vec;

const RSSI_SCALE: f32 = 190.0;
pub const MAX_SWARM_SIZE: usize = 10;

#[derive(Debug)]
pub struct NodeState {
    pub mac: [u8; 6],
    pub neighbours: HashMap<[u8; 6], HashMap<[u8; 6], f32>>,
    pub mds: Vec<Vec<f32, 2>, MAX_SWARM_SIZE>,
}

impl NodeState {
    pub fn new(mac: [u8; 6]) -> NodeState {
        let mut state = NodeState {
            mac,
            neighbours: HashMap::new(),
            mds: Vec::default(),
        };
        state.neighbours.insert(mac, HashMap::new());
        state
    }

    pub fn register_neighbour(&mut self, mac: [u8; 6]) {
        self.neighbours.insert(mac, HashMap::new());
    }

    pub fn add_distance(&mut self, src_node: [u8; 6], address: [u8; 6], rssi: i32) {
        // TODO: update with proper estimation
        let distance = rssi as f32 / RSSI_SCALE;
        let state_map = self
            .neighbours
            .get_mut(&src_node)
            .expect("Node's mac address should have been set upon initialization");
        state_map.insert(address, distance);
    }

    pub fn add_neighbour_measurement(&mut self, mac: [u8; 6], measurements: HashMap<[u8; 6], f32>) {
        self.neighbours.insert(mac, measurements);
    }

    pub fn neighbour_matrix(&self) -> Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> {
        let mut vec: Vec<[u8; 6], MAX_SWARM_SIZE> = Vec::new();
        self.neighbours.keys().for_each(|&node| {
            let _ = vec.push(node);
        });
        vec.sort();

        let matrix: Vec<Vec<f32, MAX_SWARM_SIZE>, MAX_SWARM_SIZE> = vec
            .iter()
            .map(|node| {
                let neighbours = &self.neighbours[node];
                vec.iter()
                    .map(|n| {
                        *neighbours.get(n).unwrap_or_else(|| {
                            if node == n {
                                return &0.0;
                            }
                            &f32::INFINITY
                        })
                    })
                    .collect()
            })
            .collect();
        matrix
    }
}
