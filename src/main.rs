use crate::conf::{BindAddress, ConfBuilder, ConfImpl};
use crate::errors::Error;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
#[macro_use]
extern crate lazy_static;

mod conf;
mod errors;
mod tcp;

lazy_static! {
    static ref CONFIG: ConfImpl =
        ConfBuilder::new(BindAddress::TcpSocket(SocketAddr::from(([0, 0, 0, 0], 80)))).build();
}

fn main() -> Result<(), Error> {
    let worker_thread_count = std::cmp::max(1, num_cpus::get() - 1);
    let name = std::env::current_exe()
        .ok()
        .and_then(|it| {
            it.file_stem()
                .and_then(|it| it.to_str().map(|it| it.to_string()))
        })
        .unwrap_or("headmaster".to_string());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_thread_count)
        .enable_all()
        .thread_name_fn(move || {
            static THREAD_COUNTER: AtomicU64 = AtomicU64::new(0);
            let id = THREAD_COUNTER.fetch_add(1, Ordering::SeqCst);
            format!("{}-worker-{}", name, id)
        })
        .build()?;
    let conf: &ConfImpl = &CONFIG;
    runtime.block_on(tcp::connect(conf))?;
    Ok(())
}
