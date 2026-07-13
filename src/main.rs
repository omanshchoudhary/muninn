
use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::main]
async fn main()-> std::io::Result<()>{
    let listener = TcpListener::bind("127.0.0.1:6379").await?;

    loop {
        let (mut socket, _addr) = listener.accept().await?;

        tokio::spawn(async move{
            let mut buf = [0u8; 1024];
            loop {
                match socket.read(&mut buf).await {
                    Ok(0)=> break, // client hung up
                    Ok(n) => {socket.write_all(&mut buf[..n]).await.unwrap();}, // echoing the bytes back
                    Err(_) => break,
                }
            }
        });
    }

}