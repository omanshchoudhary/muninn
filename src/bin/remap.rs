// measures what adding and removing a node does to a live cluster.
// start nodes on 6379-6382 first, then run this.
use muninn::client::{Reply, Router};

const KEYS: usize = 10_000;
const VNODES: usize = 150;

fn key(i: usize) -> String {
    format!("key{}", i)
}

// how many keys the router can still find
fn hits(r: &mut Router) -> usize {
    (0..KEYS)
        .filter(|i| matches!(r.get(&key(*i)), Ok(Reply::Value(_))))
        .count()
}

// where every key currently routes
fn placements(r: &Router) -> Vec<String> {
    (0..KEYS)
        .map(|i| r.node_for(&key(i)).unwrap().to_string())
        .collect()
}

fn pct(n: usize) -> f64 {
    n as f64 / KEYS as f64 * 100.0
}

fn main() -> std::io::Result<()> {
    let three: Vec<String> = ["127.0.0.1:6379", "127.0.0.1:6380", "127.0.0.1:6381"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let fourth = "127.0.0.1:6382";

    let mut r = Router::new(three, VNODES)?;

    for i in 0..KEYS {
        r.set(&key(i), &format!("val{}", i))?;
    }
    println!("wrote {} keys across 3 nodes", KEYS);
    println!("baseline hit rate: {:.1}%\n", pct(hits(&mut r)));

    let before = placements(&r);

    r.add_node(fourth)?;
    let after = placements(&r);

    let moved = before.iter().zip(&after).filter(|(a, b)| a != b).count();
    let onto_new = after.iter().filter(|n| *n == fourth).count();

    println!("--- added a 4th node ---");
    println!("keys that changed owner: {:.1}%", pct(moved));
    println!("keys now on the new node: {:.1}%", pct(onto_new));
    println!("hit rate now: {:.1}%", pct(hits(&mut r)));
    println!("(modulo hashing would have moved ~75% here)\n");

    // every key that moved must have moved onto the new node, never between old ones
    let shuffled = before
        .iter()
        .zip(&after)
        .filter(|(a, b)| a != b && *b != fourth)
        .count();
    println!("keys shuffled between existing nodes: {}", shuffled);

    r.remove_node(fourth);
    println!("\n--- removed it again ---");
    println!("hit rate: {:.1}%", pct(hits(&mut r)));
    println!("(original owners still held their copies, nothing was migrated)");

    Ok(())
}
