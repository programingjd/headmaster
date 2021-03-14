use crate::conf::{BindAddress, Conf};
use crate::errors::Error;
use std::net::SocketAddr;
use tokio::net::tcp::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

pub async fn connect<C: Conf + Sync>(config: &'static C) -> Result<(), Error> {
    let listener = config.bind_address().bind().await?;
    loop {
        if let Ok((mut client_stream, remote_address)) = listener.accept().await {
            tokio::spawn(async move {
                if let Some(trace) = config.accept(&remote_address) {
                    if let Some(backend_address) = config.select(&remote_address, trace) {
                        match TcpStream::connect(backend_address).await {
                            Ok(mut backend_stream) => {
                                let (mut client_stream_read, mut client_stream_write) =
                                    client_stream.split();
                                let (mut backend_stream_read, mut backend_stream_write) =
                                    backend_stream.split();
                                let read_request = async {
                                    client_stream_read
                                        .copy_to(&mut backend_stream_write)
                                        .await
                                        .map_err(|e| CopyError::ReadFailure(e))
                                };
                                let write_response = async {
                                    client_stream_write
                                        .copy_from(&mut backend_stream_read)
                                        .await
                                        .map_err(|e| CopyError::WriteFailure(e))
                                };
                                match tokio::try_join!(read_request, write_response) {
                                    Ok((request_size, response_size)) => config.record_success(
                                        &remote_address,
                                        backend_address,
                                        request_size,
                                        response_size,
                                        trace,
                                    ),
                                    Err(e) => match e {
                                        CopyError::ReadFailure(e) => {
                                            config.record_read_failure(
                                                &remote_address,
                                                backend_address,
                                                e,
                                                trace,
                                            );
                                        }
                                        CopyError::WriteFailure(e) => {
                                            config.record_write_failure(
                                                &remote_address,
                                                backend_address,
                                                e,
                                                trace,
                                            );
                                        }
                                    },
                                }
                            }
                            Err(e) => config.record_connection_failure(
                                &remote_address,
                                backend_address,
                                e,
                                trace,
                            ),
                        }
                    }
                }
            });
        }
    }
}

enum CopyError {
    ReadFailure(std::io::Error),
    WriteFailure(std::io::Error),
}

impl BindAddress {
    async fn bind(&self) -> Result<SocketListener, std::io::Error> {
        match self {
            Self::TcpSocket(address) => TcpListener::bind(address)
                .await
                .map(|it| SocketListener::Tcp(it)),
            #[cfg(target_os = "unix")]
            Self::UnixSocket(fd) => {
                let listener = std::os::unix::net::UnixListener::from_raw_fd(fd);
                UnixListener::from_std(listener)
            }
        }
    }
}

pub enum SocketListener {
    #[cfg(target_os = "unix")]
    Unix(tokio::net::UnixListener),
    Tcp(TcpListener),
}

impl SocketListener {
    async fn accept(&self) -> Result<(AcceptStream, SocketAddr), std::io::Error> {
        match self {
            Self::Tcp(listener) => listener
                .accept()
                .await
                .map(|(stream, remote_address)| (AcceptStream::Tcp(stream), remote_address)),
            #[cfg(target_os = "unix")]
            Self::Unix(listener) => listener
                .accept()
                .await
                .map(|(stream, remote_address)| (AcceptStream::Unix(stream), remote_address)),
        }
    }
}

pub enum AcceptStream {
    #[cfg(target_os = "unix")]
    Unix(tokio::net::UnixStream),
    Tcp(TcpStream),
}

impl AcceptStream {
    fn split(&mut self) -> (Read, Write) {
        match self {
            Self::Tcp(stream) => {
                let (read, write) = stream.split();
                (Read::Tcp(read), Write::Tcp(write))
            }
            #[cfg(target_os = "unix")]
            Self::Unix(stream) => {
                let (read, write) = stream.split();
                (Read::Unix(read), Write::Unix(write))
            }
        }
    }
}

pub enum Read<'a> {
    #[cfg(target_os = "unix")]
    Unix(tokio::net::unix::ReadHalf<'a>),
    Tcp(ReadHalf<'a>),
}

pub enum Write<'a> {
    #[cfg(target_os = "unix")]
    Unix(tokio::net::unix::WriteHalf<'a>),
    Tcp(WriteHalf<'a>),
}

impl<'a> Read<'a> {
    async fn copy_to(&'a mut self, write: &'a mut WriteHalf<'a>) -> Result<u64, std::io::Error> {
        match self {
            Self::Tcp(read) => tokio::io::copy(read, write).await,
            #[cfg(target_os = "unix")]
            Self::Unix(read) => tokio::io::copy(read, write).await,
        }
    }
}
impl<'a> Write<'a> {
    async fn copy_from(&'a mut self, read: &'a mut ReadHalf<'a>) -> Result<u64, std::io::Error> {
        match self {
            Self::Tcp(write) => tokio::io::copy(read, write).await,
            #[cfg(target_os = "unix")]
            Self::Unix(write) => tokio::io::copy(read, write).await,
        }
    }
}
