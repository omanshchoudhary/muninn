// benchmark harness: throughput and latency percentiles under concurrent load.
// start the nodes first, then:
//   cargo run --release --bin bench -- --nodes 127.0.0.1:6379,127.0.0.1:6380 --threads 8
use muninn::client::{Reply, Router};
use std::time::Instant;

const VNODES: usize = 150;

struct Config {
    nodes: Vec<String>,
    threads: usize,
    ops: usize, // per thread
    keys: usize,
    value_size: usize,
    read_ratio: f64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            nodes: vec!["127.0.0.1:6379".to_string()],
            threads: 8,
            ops: 10_000,
            keys: 10_000,
            value_size: 64,
            read_ratio: 0.9,
        }
    }
}

// tiny xorshift, keeps the harness dependency free
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: bench [--nodes a:1,b:2] [--threads N] [--ops N] \
         [--keys N] [--value-size N] [--read-ratio 0.0-1.0]"
    );
    std::process::exit(1);
}

fn parse_args() -> Config {
    let args: Vec<String> = std::env::args().collect();
    let mut cfg = Config::default();

    let mut i = 1;
    while i < args.len() {
        let value = || args.get(i + 1).cloned().unwrap_or_else(|| usage());
        match args[i].as_str() {
            "--nodes" => cfg.nodes = value().split(',').map(|s| s.trim().to_string()).collect(),
            "--threads" => cfg.threads = value().parse().unwrap_or_else(|_| usage()),
            "--ops" => cfg.ops = value().parse().unwrap_or_else(|_| usage()),
            "--keys" => cfg.keys = value().parse().unwrap_or_else(|_| usage()),
            "--value-size" => cfg.value_size = value().parse().unwrap_or_else(|_| usage()),
            "--read-ratio" => cfg.read_ratio = value().parse().unwrap_or_else(|_| usage()),
            _ => usage(),
        }
        i += 2;
    }
    cfg
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

fn row(label: &str, mut lat: Vec<u64>) {
    if lat.is_empty() {
        println!("  {:<6} {:>28}", label, "no samples");
        return;
    }
    lat.sort_unstable();
    let mean = lat.iter().sum::<u64>() as f64 / lat.len() as f64;
    println!(
        "  {:<6} {:>9} {:>8} {:>8} {:>8} {:>8} {:>9}",
        label,
        lat.len(),
        percentile(&lat, 0.50),
        percentile(&lat, 0.95),
        percentile(&lat, 0.99),
        percentile(&lat, 0.999),
        format!("{:.0}", mean),
    );
}

fn main() -> std::io::Result<()> {
    let cfg = parse_args();
    let value = "x".repeat(cfg.value_size);

    // warm the cache so reads actually hit
    let mut warm = Router::new(cfg.nodes.clone(), VNODES)?;
    for i in 0..cfg.keys {
        warm.set(&format!("key{}", i), &value)?;
    }

    println!("muninn benchmark");
    println!("  nodes        {} ({})", cfg.nodes.len(), cfg.nodes.join(", "));
    println!("  threads      {}", cfg.threads);
    println!("  ops/thread   {}", cfg.ops);
    println!("  total ops    {}", cfg.threads * cfg.ops);
    println!("  keyspace     {}", cfg.keys);
    println!("  value size   {} B", cfg.value_size);
    println!(
        "  workload     {:.0}% GET / {:.0}% SET\n",
        cfg.read_ratio * 100.0,
        (1.0 - cfg.read_ratio) * 100.0
    );

    let read_cutoff = (cfg.read_ratio * u64::MAX as f64) as u64;
    let started = Instant::now();

    let mut handles = Vec::new();
    for t in 0..cfg.threads {
        // every thread gets its own router, so its own sockets
        let nodes = cfg.nodes.clone();
        let value = value.clone();
        let (ops, keys) = (cfg.ops, cfg.keys);

        handles.push(std::thread::spawn(move || -> std::io::Result<_> {
            let mut r = Router::new(nodes, VNODES)?;
            let mut rng = Rng(0x9e3779b97f4a7c15 ^ (t as u64 + 1));
            let mut gets: Vec<u64> = Vec::with_capacity(ops);
            let mut sets: Vec<u64> = Vec::with_capacity(ops);
            let mut misses = 0usize;

            for _ in 0..ops {
                let key = format!("key{}", rng.next() as usize % keys);
                let read = rng.next() < read_cutoff;

                let at = Instant::now();
                let reply = if read {
                    r.get(&key)?
                } else {
                    r.set(&key, &value)?
                };
                let took = at.elapsed().as_micros() as u64;

                if read {
                    if !matches!(reply, Reply::Value(_)) {
                        misses += 1;
                    }
                    gets.push(took);
                } else {
                    sets.push(took);
                }
            }
            Ok((gets, sets, misses))
        }));
    }

    let (mut gets, mut sets, mut misses) = (Vec::new(), Vec::new(), 0usize);
    for h in handles {
        let (g, s, m) = h.join().expect("worker panicked")?;
        gets.extend(g);
        sets.extend(s);
        misses += m;
    }

    let elapsed = started.elapsed();
    let total = gets.len() + sets.len();
    let per_sec = total as f64 / elapsed.as_secs_f64();

    println!("throughput");
    println!(
        "  {} ops in {:.3}s  ->  {:.0} ops/sec\n",
        total,
        elapsed.as_secs_f64(),
        per_sec
    );

    println!("latency (microseconds)");
    println!(
        "  {:<6} {:>9} {:>8} {:>8} {:>8} {:>8} {:>9}",
        "", "samples", "p50", "p95", "p99", "p99.9", "mean"
    );
    row("GET", gets);
    row("SET", sets);

    if misses > 0 {
        println!("\n  {} read misses", misses);
    }
    Ok(())
}
