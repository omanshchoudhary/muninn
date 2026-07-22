use muninn::{resp::handle, store::Store};
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;


#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Read CLI args: muninn [--port <PORT>] [--max-memory <bytes>]
    // no flag (or 0) = unlimited, same convention as redis `maxmemory 0`
    let args: Vec<String> = env::args().collect();
    let mut max_memory = 0;
    let mut port: u16 = 6379;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                port = match args.get(i + 1).and_then(|v| v.parse().ok()) {
                    Some(p) => p,
                    None => {
                        eprintln!("usage: muninn [--port <PORT>] [--max-memory <BYTES>]");
                        std::process::exit(1);
                    }
                };
                i += 2;
            }

            "--max-memory" => {
                max_memory = match args.get(i + 1).and_then(|v| v.parse().ok()) {
                    Some(m) => m,
                    None => {
                        eprintln!("usage: muninn [--port <PORT>] [--max-memory <BYTES>]");
                        std::process::exit(1);
                    }
                };
                i += 2;
            }

            _ => {
                eprintln!("unknown argument: {}", args[i]);
                eprintln!("usage: muninn [--port <PORT>] [--max-memory <BYTES>]");
                std::process::exit(1);
            }
        }
    }

    // Create Store
    let store = Arc::new(Store::new(max_memory));

    // Start TCP Listener
    let listener = TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    println!("muninn listening on 127.0.0.1:{}", port);

    loop {
        let (socket, _addr) = listener.accept().await?;
        let store_per_task = Arc::clone(&store);
        tokio::spawn(handle(socket, store_per_task));
    }
}
