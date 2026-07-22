// kills a node under load and reports exactly what breaks.
// spawns its own cluster so the whole experiment is one command:
//   cargo run --release --bin killnode
use muninn::client::{Reply, Router};
use std::process::{Child, Command};

const KEYS: usize = 3_000;
const VNODES: usize = 150;
const PORTS: [u16; 3] = [7379, 7380, 7381];

fn addr(port: u16) -> String {
    format!("127.0.0.1:{}", port)
}

fn spawn_nodes() -> Vec<Child> {
    let exe = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .join("muninn");

    PORTS
        .iter()
        .map(|p| {
            Command::new(&exe)
                .args(["--port", &p.to_string()])
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("could not spawn a node, run cargo build --release first")
        })
        .collect()
}

// counts hits, misses and hard errors across the whole keyspace
fn survey(r: &mut Router) -> (usize, usize, usize, Option<String>) {
    let (mut hits, mut misses, mut errors) = (0, 0, 0);
    let mut first_error = None;

    for i in 0..KEYS {
        match r.get(&format!("key{}", i)) {
            Ok(Reply::Value(_)) => hits += 1,
            Ok(_) => misses += 1,
            Err(e) => {
                if first_error.is_none() {
                    first_error = Some(e.to_string());
                }
                errors += 1;
            }
        }
    }
    (hits, misses, errors, first_error)
}

fn report(label: &str, r: &mut Router) {
    let (hits, misses, errors, first) = survey(r);
    let pct = |n: usize| n as f64 / KEYS as f64 * 100.0;
    println!("{}", label);
    println!(
        "  hits {:>5} ({:>5.1}%)   misses {:>5} ({:>5.1}%)   errors {:>5} ({:>5.1}%)",
        hits,
        pct(hits),
        misses,
        pct(misses),
        errors,
        pct(errors)
    );
    if let Some(e) = first {
        println!("  first error: {}", e);
    }
    println!();
}

fn main() -> std::io::Result<()> {
    let mut children = spawn_nodes();
    std::thread::sleep(std::time::Duration::from_millis(600));

    let nodes: Vec<String> = PORTS.iter().map(|p| addr(*p)).collect();
    let mut r = Router::new(nodes, VNODES)?;

    for i in 0..KEYS {
        r.set(&format!("key{}", i), &format!("val{}", i))?;
    }
    report("all three nodes up", &mut r);

    // how many keys does the victim own, before we kill it
    let victim = addr(PORTS[1]);
    let owned = (0..KEYS)
        .filter(|i| r.node_for(&format!("key{}", i)) == Some(victim.as_str()))
        .count();
    println!("killing {} which owns {} keys\n", victim, owned);

    children[1].kill()?;
    children[1].wait()?;
    std::thread::sleep(std::time::Duration::from_millis(300));

    report("after SIGKILL, router unchanged", &mut r);

    // the only remediation this design has: tell the client the node is gone
    r.remove_node(&victim);
    report("after remove_node, ring reshaped", &mut r);

    // and once the survivors are rewritten, service is whole again
    for i in 0..KEYS {
        r.set(&format!("key{}", i), &format!("val{}", i))?;
    }
    report("after rewriting the keyspace", &mut r);

    for c in children.iter_mut() {
        let _ = c.kill();
        let _ = c.wait();
    }
    Ok(())
}
