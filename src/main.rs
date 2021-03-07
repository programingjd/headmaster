use crate::errors::Error;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

mod config;
mod errors;
mod tcp;

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
    runtime.block_on(tcp::forward(config::Config {}));
    Ok(())
}
