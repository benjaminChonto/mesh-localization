use hashbrown::HashMap;


const RSSI_SCALE: f32 = 190.0;


#[derive(Debug)]
pub struct NodeState {
    pub neighbours: HashMap<[u8; 6], f32>,
}

impl Default for NodeState {

    fn default() -> NodeState {
        NodeState {
            neighbours: HashMap::new(),
        }
    }
}

impl NodeState {

    pub fn update(&mut self, src_address: [u8; 6], rssi:  i32) {
        // TODO: update with proper estimation
        let distance = rssi as f32 / RSSI_SCALE;
        self.neighbours.insert(src_address, distance);
    }

}
