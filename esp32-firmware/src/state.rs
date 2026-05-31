use core::cell::RefCell;

use hashbrown::HashMap;

const RSSI_SCALE: f32 = 190.0;

#[derive(Debug)]
pub struct NodeState {
    pub neighbours: RefCell<HashMap<[u8; 6], f32>>,
}

impl Default for NodeState {
    fn default() -> NodeState {
        NodeState {
            neighbours: RefCell::new(HashMap::new()),
        }
    }
}

impl NodeState {
    pub fn update(&self, src_address: [u8; 6], rssi: i32) {
        let distance = rssi as f32 / RSSI_SCALE;
        self.neighbours.borrow_mut().insert(src_address, distance);
    }

    pub fn num_neighbors(&self) -> usize {
        self.neighbours.borrow().len()
    }
}
