use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[test]
fn test() {
    println!("creating runtime1");
    let runtime1 = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let listener1 = runtime1.block_on(async move { listen1().await });
    let port1 = listener1.local_addr().unwrap().port();
    println!("port1: {}", port1);
    let backend1 = SocketAddr::from(([127, 0, 0, 1], port1));
    runtime1.block_on(async move { accept1(listener1).await });
}

async fn listen1() -> TcpListener {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap()
}

async fn accept1(listener: TcpListener) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let (mut read, mut write) = stream.split();
        println!("accepted1");
        let (request, _) = tokio::try_join!(
            async move {
                let mut request = [0u8, 100];
                read.read_exact(&mut request).await.map(|it| request)
            },
            async move { write.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await }
        )
        .unwrap();
        println!("{:?}", String::from_utf8_lossy(&request));
    }
}
