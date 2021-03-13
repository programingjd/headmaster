use crate::conf;
use crate::conf::Conf;
use crate::errors::Error;
use tokio::net::{TcpListener, TcpStream};

pub async fn connect<C: Conf + Sync>(config: &'static C) -> Result<(), Error> {
    match config.bind_address() {
        conf::BindAddress::TcpSocket { address } => {
            let listener = TcpListener::bind(address).await?;
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
                                        let write_request = async {
                                            tokio::io::copy(
                                                &mut client_stream_read,
                                                &mut backend_stream_write,
                                            )
                                            .await
                                            .map_err(|e| CopyError::ReadFailure(e))
                                        };
                                        let write_response = async {
                                            tokio::io::copy(
                                                &mut backend_stream_read,
                                                &mut client_stream_write,
                                            )
                                            .await
                                            .map_err(|e| CopyError::WriteFailure(e))
                                        };
                                        match tokio::try_join!(write_request, write_response) {
                                            Ok((request_size, response_size)) => config
                                                .record_success(
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
        #[cfg(target_os = "unix")]
        _ => unimplemented!(),
    };
}

enum CopyError {
    ReadFailure(std::io::Error),
    WriteFailure(std::io::Error),
}
