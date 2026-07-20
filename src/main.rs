mod lru;
mod resp;
mod store;

use resp::handle;
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;

use crate::store::Store;
#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Read CLI args: muninn [--max-memory <bytes>]
    // no flag (or 0) = unlimited, same convention as redis `maxmemory 0`
    let args: Vec<String> = env::args().collect();
    let max_memory: usize = match args.get(1).map(String::as_str) {
        None => 0,
        Some("--max-memory") => match args.get(2).and_then(|v| v.parse().ok()) {
            Some(n) => n,
            None => {
                eprintln!("usage: muninn [--max-memory <bytes>]");
                std::process::exit(1);
            }
        },
        Some(_) => {
            eprintln!("usage: muninn [--max-memory <bytes>]");
            std::process::exit(1);
        }
    };

    // Create Store
    let store = Arc::new(Store::new(max_memory));

    // Start TCP Listener
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (socket, _addr) = listener.accept().await?;
        let store_per_task = Arc::clone(&store);
        tokio::spawn(handle(socket, store_per_task));
    }
}
