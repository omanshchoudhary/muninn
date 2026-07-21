use std::collections::BTreeMap;

pub struct Ring {
    // ring position -> physical node name
    // the u64 is a position on the circle
    positions: BTreeMap<u64, String>,
    vnodes_per_node: usize,
}

impl Ring {
    // vnodes_per_node equals how many times each physical node is placed on the ring.
    pub fn new(vnodes_per_node: usize) -> Self {
        Self {
            positions: BTreeMap::new(),
            vnodes_per_node,
        }
    }

    pub fn add_node(&mut self, node: &str) {
        for i in 0..self.vnodes_per_node {
            let vnode_name = format!("{}#{}", node, i);
            let pos = fnv1a(&vnode_name);

            self.positions.insert(pos, node.to_string());
        }
    }

    pub fn remove_node(&mut self, node: &str) {
        for i in 0..self.vnodes_per_node {
            let vnode_name = format!("{}#{}", node, i);
            let pos = fnv1a(&vnode_name);

            self.positions.remove(&pos);
        }
    }

    pub fn get_node(&self, key: &str) -> Option<&str> {
        let pos = fnv1a(key);

        self.positions
            .range(pos..)
            .next() // first node at or clockwise of the key
            .or_else(|| self.positions.first_key_value()) // walked off the end → wrap around
            .map(|(_pos, node)| node.as_str()) // we want the node, not the position
    }
}

fn fnv1a(s: &str) -> u64 {
    let mut h: u64 = 14695981039346656037;

    for byte in s.as_bytes() {
        h ^= *byte as u64;
        h = h.wrapping_mul(1099511628211);
    }

    h
}
