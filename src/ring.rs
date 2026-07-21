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

    // avalanche. plain fnv-1a barely moves its high bits when the last byte
    // changes, so "node-a#0".."node-a#149" would all land in one narrow slice
    // of the ring instead of scattering. the shifts drag high bits down and
    // the multiplies push them back up, so one input bit flips ~half the output.
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;

    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const KEYS: usize = 100_000;

    fn ring_of(vnodes: usize, nodes: &[&str]) -> Ring {
        let mut ring = Ring::new(vnodes);
        for n in nodes {
            ring.add_node(n);
        }
        ring
    }

    // where every key lands, keyed by "key42" -> "node-b"
    fn placements(ring: &Ring) -> HashMap<String, String> {
        (0..KEYS)
            .map(|i| {
                let key = format!("key{}", i);
                let node = ring.get_node(&key).unwrap().to_string();
                (key, node)
            })
            .collect()
    }

    fn counts(ring: &Ring) -> HashMap<String, usize> {
        let mut counts = HashMap::new();
        for i in 0..KEYS {
            let node = ring.get_node(&format!("key{}", i)).unwrap();
            *counts.entry(node.to_string()).or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn empty_ring_has_nowhere_to_put_a_key() {
        let ring = Ring::new(100);
        assert_eq!(ring.get_node("anything"), None);
    }

    #[test]
    fn same_key_always_lands_on_the_same_node() {
        let ring = ring_of(100, &["node-a", "node-b", "node-c"]);
        for i in 0..1000 {
            let key = format!("key{}", i);
            assert_eq!(ring.get_node(&key), ring.get_node(&key));
        }
    }

    #[test]
    fn every_key_lands_somewhere_real() {
        let nodes = ["node-a", "node-b", "node-c"];
        let ring = ring_of(100, &nodes);
        for i in 0..1000 {
            let node = ring.get_node(&format!("key{}", i)).unwrap();
            // must be a physical name, never a "node-a#7" vnode label
            assert!(nodes.contains(&node), "got a non-physical node: {}", node);
        }
    }

    #[test]
    fn a_single_node_owns_the_whole_ring() {
        let ring = ring_of(100, &["only-node"]);
        // every position wraps around to the same node when there's only one
        for i in 0..1000 {
            assert_eq!(ring.get_node(&format!("key{}", i)), Some("only-node"));
        }
    }

    #[test]
    fn vnodes_split_the_load_evenly() {
        let ring = ring_of(150, &["node-a", "node-b", "node-c"]);
        let counts = counts(&ring);

        // expected spread is roughly 1/sqrt(vnodes) = ~8% at 150,
        // so 15% is a couple of sigma rather than an arbitrary round number
        let ideal = KEYS / 3;
        for (node, count) in &counts {
            let drift = (*count as f64 - ideal as f64).abs() / ideal as f64;
            assert!(
                drift < 0.15,
                "{} holds {} keys, {:.1}% off an even split",
                node,
                count,
                drift * 100.0
            );
        }
    }

    #[test]
    fn more_vnodes_means_a_tighter_split() {
        let worst_drift = |vnodes: usize| {
            let ring = ring_of(vnodes, &["node-a", "node-b", "node-c"]);
            let ideal = KEYS as f64 / 3.0;
            counts(&ring)
                .values()
                .map(|c| (*c as f64 - ideal).abs() / ideal)
                .fold(0.0, f64::max)
        };

        // the core claim of virtual nodes: more placements, more even split
        assert!(worst_drift(500) < worst_drift(10));
    }

    #[test]
    fn one_placement_per_node_splits_badly() {
        // the contrast that justifies vnodes: place each node once and the
        // arcs are whatever the hash felt like, wildly lopsided
        let ring = ring_of(1, &["node-a", "node-b", "node-c"]);
        let counts = counts(&ring);

        let ideal = KEYS / 3;
        let worst = counts
            .values()
            .map(|c| (*c as f64 - ideal as f64).abs() / ideal as f64)
            .fold(0.0, f64::max);

        assert!(
            worst > 0.10,
            "expected a lopsided split without vnodes, worst drift was {:.1}%",
            worst * 100.0
        );
    }

    #[test]
    fn adding_a_node_moves_only_its_share() {
        // this is the whole reason consistent hashing exists.
        // with hash(key) % n, going 3 -> 4 nodes moves ~75% of keys and
        // the cache goes cold all at once. here it should be ~1/4.
        let before = ring_of(150, &["node-a", "node-b", "node-c"]);
        let placed_before = placements(&before);

        let after = ring_of(150, &["node-a", "node-b", "node-c", "node-d"]);
        let placed_after = placements(&after);

        let moved = placed_before
            .iter()
            .filter(|(key, node)| placed_after[*key] != **node)
            .count();

        let fraction = moved as f64 / KEYS as f64;
        assert!(
            fraction > 0.15 && fraction < 0.35,
            "expected ~25% of keys to move, got {:.1}%",
            fraction * 100.0
        );

        // and every key that moved must have moved *onto* the new node —
        // adding a node should never shuffle keys between existing ones
        for (key, node) in &placed_before {
            let now = &placed_after[key];
            assert!(
                now == node || now == "node-d",
                "key {} moved from {} to {}, but only node-d was added",
                key,
                node,
                now
            );
        }
    }

    #[test]
    fn removing_a_node_leaves_everyone_else_alone() {
        let before = ring_of(150, &["node-a", "node-b", "node-c"]);
        let placed_before = placements(&before);

        let after = ring_of(150, &["node-a", "node-b"]);
        let placed_after = placements(&after);

        for (key, node) in &placed_before {
            if node != "node-c" {
                // keys that weren't on the departing node must not budge
                assert_eq!(&placed_after[key], node, "key {} moved unnecessarily", key);
            }
        }
    }

    #[test]
    fn remove_node_takes_every_vnode_with_it() {
        let mut ring = ring_of(150, &["node-a", "node-b", "node-c"]);
        ring.remove_node("node-b");

        for i in 0..KEYS {
            assert_ne!(ring.get_node(&format!("key{}", i)), Some("node-b"));
        }
    }

    #[test]
    fn removing_the_last_node_empties_the_ring() {
        let mut ring = ring_of(150, &["only-node"]);
        ring.remove_node("only-node");
        assert_eq!(ring.get_node("anything"), None);
    }
}
