mod resp;

use resp::handle;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (socket, _addr) = listener.accept().await?;

        tokio::spawn(handle(socket));
    }
}
