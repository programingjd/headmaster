use headmaster::errors::Error;
use headmaster::tcp::*;
use headmaster::{BindAddress, Conf, ConfBuilder, ConfImpl};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;
use tokio::time::timeout;

#[macro_use]
extern crate lazy_static;

const ID1: &'static [u8] = b"1";
const ID2: &'static [u8] = b"2";

lazy_static! {
    static ref ADDRESS: SocketAddr = SocketAddr::from(([127, 0, 0, 1], 0));
    static ref CONFIG: ConfImpl = ConfBuilder::new(BindAddress::TcpSocket(*ADDRESS)).build();
}

#[test]
fn test1() {
    let (address1, backend1) = start_backend(1, ID1);
    let (address2, backend2) = start_backend(1, ID2);

    let runtime = runtime(2);
    let listener = runtime.block_on(bind(&*CONFIG)).unwrap();
    let port = match listener {
        SocketListener::Tcp(ref listener) => listener.local_addr().unwrap().port(),
        _ => panic!(),
    };
    println!("port: {}", port);
    let balancer = runtime.spawn(accept_loop(&*CONFIG, listener));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    assert!(runtime.block_on(request(port)).is_err());
    CONFIG.add_backend(address1);
    assert_eq!([runtime.block_on(request(port)).unwrap()], ID1);
    assert!(false);

    CONFIG.add_backend(address2);

    runtime.block_on(async move {
        let address: &SocketAddr = &ADDRESS;
        let mut stream = TcpStream::connect(&address).await.unwrap();
        let (mut r, _) = stream.split();
        let k = r.read_u8().await.unwrap();
        println!("{}", k);
    });
    //backend1
    balancer.abort();
}

async fn request(port: u16) -> Result<u8, Error> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect(&address).await?;
    let (mut r, _) = stream.split();
    let k = r.read_u8().await?;
    Ok(k)
}

fn start_backend(thread_count: usize, id: &'static [u8]) -> (SocketAddr, JoinHandle<()>) {
    let runtime = runtime(thread_count);
    let listener = runtime.block_on(listen());
    let port = listener.local_addr().unwrap().port();
    println!("port: {}", port);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let backend = runtime.spawn(accept(listener, id));
    (address, backend)
}

fn runtime(thread_count: usize) -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(thread_count)
        .enable_all()
        .build()
        .unwrap()
}

async fn listen() -> TcpListener {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap()
}

async fn accept(listener: TcpListener, k: &[u8]) {
    while let Ok((mut stream, _)) = listener.accept().await {
        let (mut read, mut write) = stream.split();
        println!("accepted");
        let (request, _) = tokio::try_join!(
            async move {
                let mut vec = Vec::new();
                let mut buf = vec![0u8; 1024];
                loop {
                    match timeout(Duration::from_millis(100), read.read(&mut buf)).await {
                        Err(_) => {
                            println!("Timeout");
                            break;
                        }
                        Ok(Err(e)) => {
                            println!("{:?}", e);
                            break;
                        }
                        Ok(Ok(0)) => {
                            println!("Empty");
                            break;
                        }
                        Ok(Ok(n)) => {
                            println!("copying {} bytes", n);
                            vec.extend_from_slice(&buf[0..n]);
                        }
                    }
                }
                Ok(String::from_utf8_lossy(&vec).to_string())
            },
            async move { timeout(Duration::from_millis(1000), write.write_all(k)).await }
        )
        .unwrap();
        //println!("request:\n{}", request);
        //if request.starts_with("GET /stop ") {
        //    return;
        //}
    }
}
