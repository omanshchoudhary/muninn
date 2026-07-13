mod resp;
mod store;

use resp::handle;
use std::sync::Arc;

use tokio::net::TcpListener;

use crate::store::Store;
#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;
    let store = Arc::new(Store::new());

    loop {
        let (socket, _addr) = listener.accept().await?;
        let store_per_task = Arc::clone(&store);
        tokio::spawn(handle(socket, store_per_task));
    }
}
