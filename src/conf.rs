use crossbeam::sync::ShardedLock;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub enum BindAddress {
    #[cfg(target_os = "unix")]
    UnixSocket(std::os::unix::io::RawFd),
    TcpSocket(SocketAddr),
}

pub trait ToSocketAddr {
    fn address(&self) -> &SocketAddr;
}

pub trait Conf<T: ToSocketAddr> {
    type Trace: Copy + Send;
    fn bind_address(&self) -> &BindAddress;
    fn admin_address(&self) -> &BindAddress;
    fn accept(&self, remote_address: &SocketAddr) -> Option<Self::Trace>;
    fn select(&self, remote_address: &SocketAddr, trace: Self::Trace) -> Option<T>;
    fn connection_timeout(&self) -> Option<Duration>;
    fn read_timeout(&self) -> Option<Duration>;
    fn write_timeout(&self) -> Option<Duration>;
    fn add_backend(&self, backend_address: SocketAddr);
    fn remove_backend(&self, backend_address: SocketAddr);
    fn record_success(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        request_size: u64,
        response_size: u64,
        trace: Self::Trace,
    );
    fn record_connection_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
    fn record_connection_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
    fn record_read_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
    fn record_read_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
    fn record_write_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
    fn record_write_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: T,
        error: std::io::Error,
        trace: Self::Trace,
    );
}

pub struct ConfBuilder {
    bind_address: BindAddress,
    admin_address: BindAddress,
    connection_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    blacklist: HashSet<SocketAddr>,
}

impl ConfBuilder {
    pub fn new(bind_address: BindAddress) -> Self {
        ConfBuilder {
            admin_address: Self::admin_address_from(&bind_address),
            bind_address,
            connection_timeout: Some(Duration::from_millis(5_000)),
            read_timeout: Some(Duration::from_millis(30_000)),
            write_timeout: Some(Duration::from_millis(120_000)),
            blacklist: HashSet::new(),
        }
    }
    #[allow(dead_code)]
    pub fn admin_address(&mut self, admin_address: BindAddress) -> &mut Self {
        self.admin_address = admin_address;
        self
    }
    #[allow(dead_code)]
    pub fn no_connection_timeout(&mut self) -> &mut Self {
        self.connection_timeout = None;
        self
    }
    #[allow(dead_code)]
    pub fn connection_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.connection_timeout = Some(timeout);
        self
    }
    #[allow(dead_code)]
    pub fn no_read_timeout(&mut self) -> &mut Self {
        self.read_timeout = None;
        self
    }
    #[allow(dead_code)]
    pub fn read_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.read_timeout = Some(timeout);
        self
    }
    #[allow(dead_code)]
    pub fn no_write_timeout(&mut self) -> &mut Self {
        self.write_timeout = None;
        self
    }
    #[allow(dead_code)]
    pub fn write_timeout(&mut self, timeout: Duration) -> &mut Self {
        self.write_timeout = Some(timeout);
        self
    }
    #[allow(dead_code)]
    pub fn blacklist(&mut self, remote_address: SocketAddr) -> &mut Self {
        self.blacklist.insert(remote_address);
        self
    }
    pub fn build(&self) -> ConfImpl {
        ConfImpl {
            bind_address: self.bind_address.clone(),
            admin_address: self.admin_address.clone(),
            connection_timeout: self.connection_timeout,
            read_timeout: self.read_timeout,
            write_timeout: self.write_timeout,
            backends: ShardedLock::new(vec![]),
            blacklist: self.blacklist.iter().map(|it| it.clone()).collect(),
        }
    }

    fn admin_address_from(bind_address: &BindAddress) -> BindAddress {
        match bind_address {
            BindAddress::TcpSocket(address) => BindAddress::TcpSocket(SocketAddr::from((
                address.ip(),
                if address.port() == 8000 { 8001 } else { 8000 },
            ))),
            #[cfg(target_os = "unix")]
            UnixSocket(fd) => BindAddress::TcpSocket {
                address: SocketAddr::from((0, 0, 0, 0), 8000),
            },
        }
    }
}

pub struct ConfImpl {
    bind_address: BindAddress,
    admin_address: BindAddress,
    connection_timeout: Option<Duration>,
    read_timeout: Option<Duration>,
    write_timeout: Option<Duration>,
    backends: ShardedLock<Vec<Arc<Backend>>>,
    blacklist: HashSet<SocketAddr>,
}

impl ConfImpl {
    fn backend(backends: &Vec<Arc<Backend>>) -> Option<&Arc<Backend>> {
        backends.first()
    }
}

impl Conf<Arc<Backend>> for ConfImpl {
    type Trace = Instant;
    fn bind_address(&self) -> &BindAddress {
        &self.bind_address
    }
    fn admin_address(&self) -> &BindAddress {
        &self.admin_address
    }
    fn accept(&self, remote_address: &SocketAddr) -> Option<Self::Trace> {
        let start_time = Instant::now();
        if self.blacklist.contains(remote_address) {
            None
        } else {
            Some(start_time)
        }
    }

    fn select(&self, remote_address: &SocketAddr, _trace: Self::Trace) -> Option<Arc<Backend>> {
        let selected = if let Ok(backends) = self.backends.read() {
            if let Some(first) = Self::backend(&*backends) {
                let backend = first.clone();
                Some(backend)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(ref backend) = selected {
            backend.active_counter.fetch_add(1, Ordering::Relaxed);
        } else {
            eprintln!("{} => NO BACKEND", remote_address);
        }
        selected
    }
    fn connection_timeout(&self) -> Option<Duration> {
        self.connection_timeout
    }
    fn read_timeout(&self) -> Option<Duration> {
        self.read_timeout
    }
    fn write_timeout(&self) -> Option<Duration> {
        self.write_timeout
    }
    fn add_backend(&self, backend_address: SocketAddr) {
        let mut backends = self.backends.write().unwrap();
        if backends
            .iter()
            .enumerate()
            .find(|it| it.1.address == backend_address)
            .is_none()
        {
            backends.push(Arc::new(Backend::init(backend_address)));
        }
    }
    fn remove_backend(&self, backend_address: SocketAddr) {
        let mut backends = self.backends.write().unwrap();
        if let Some((pos, _)) = backends
            .iter()
            .enumerate()
            .find(|it| it.1.address == backend_address)
        {
            backends.remove(pos);
        }
    }
    fn record_success(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        request_size: u64,
        response_size: u64,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address.last_failure.store(0, Ordering::Relaxed);
        println!(
            "{} [{}] => {} [{}] ({}ms)",
            remote_address,
            request_size,
            backend_address.address,
            response_size,
            time.as_millis()
        );
    }
    fn record_connection_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} => {} FAILURE ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
    fn record_connection_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} => {} TIMEOUT ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
    fn record_read_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} [FAILURE] => {} ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
    fn record_read_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} [TIMEOUT] => {} ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
    fn record_write_failure(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} [] => {} [FAILURE] ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
    fn record_write_timeout(
        &self,
        remote_address: &SocketAddr,
        backend_address: Arc<Backend>,
        error: std::io::Error,
        trace: Self::Trace,
    ) {
        let time = Instant::now().duration_since(trace);
        backend_address
            .active_counter
            .fetch_sub(1, Ordering::Relaxed);
        backend_address
            .last_failure
            .store(time.as_secs(), Ordering::Relaxed);
        eprintln!(
            "{} [] => {} [TIMEOUT] ({}ms)\n{}",
            remote_address,
            backend_address.address,
            time.as_millis(),
            error
        );
    }
}

pub struct Backend {
    address: SocketAddr,
    active_counter: AtomicI32,
    last_failure: AtomicU64, // secs
    unavailable: AtomicBool,
}

impl ToSocketAddr for Arc<Backend> {
    fn address(&self) -> &SocketAddr {
        &self.address
    }
}

impl Backend {
    fn init(address: SocketAddr) -> Self {
        Self {
            address,
            active_counter: AtomicI32::new(0),
            last_failure: AtomicU64::new(0),
            unavailable: AtomicBool::new(false),
        }
    }
}
