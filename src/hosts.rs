use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinSock::*;
struct Cached {
    label: Option<String>,
    until: Instant,
}
/// DNS work never runs on the capture or UI threads. Queues and cache are bounded.
pub struct Resolver {
    tx: Option<SyncSender<IpAddr>>,
    rx: Receiver<(IpAddr, Option<String>)>,
    cache: HashMap<IpAddr, Cached>,
    pending: HashSet<IpAddr>,
    stop: Arc<AtomicBool>,
}
impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}
impl Resolver {
    pub fn new() -> Self {
        let (tx, jobs) = mpsc::sync_channel::<IpAddr>(128);
        let jobs = Arc::new(Mutex::new(jobs));
        let (results, rx) = mpsc::sync_channel(128);
        let stop = Arc::new(AtomicBool::new(false));
        for n in 0..2 {
            let jobs = jobs.clone();
            let results = results.clone();
            let stop = stop.clone();
            let _ = std::thread::Builder::new()
                .name(format!("host-names-{n}"))
                .spawn(move || {
                    let mut data = WSADATA::default();
                    // SAFETY: data is a valid writable WSADATA, and version 2.2 is supported.
                    if unsafe { WSAStartup(0x0202, &mut data) } != 0 {
                        return;
                    }
                    struct Winsock;
                    impl Drop for Winsock {
                        fn drop(&mut self) {
                            // SAFETY: balances this thread's successful WSAStartup.
                            unsafe {
                                WSACleanup();
                            }
                        }
                    }
                    let _winsock = Winsock;
                    while !stop.load(Ordering::Relaxed) {
                        let ip = match jobs.lock() {
                            Ok(rx) => match rx.recv() {
                                Ok(ip) => ip,
                                Err(_) => break,
                            },
                            Err(_) => break,
                        };
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let name = lookup(ip);
                        let _ = results.send((ip, name));
                    }
                });
        }
        Self {
            tx: Some(tx),
            rx,
            cache: HashMap::new(),
            pending: HashSet::new(),
            stop,
        }
    }
    pub fn request(&mut self, addresses: &[String]) {
        for address in addresses.iter().take(128) {
            let Ok(ip) = address.parse::<IpAddr>() else {
                continue;
            };
            if self.pending.contains(&ip)
                || self
                    .cache
                    .get(&ip)
                    .is_some_and(|r| r.until > Instant::now())
            {
                continue;
            }
            if self.tx.as_ref().is_some_and(|tx| tx.try_send(ip).is_ok()) {
                self.pending.insert(ip);
            }
        }
    }
    pub fn poll(&mut self) -> bool {
        let before = self.cache.len();
        self.cache.retain(|_, c| c.until > Instant::now());
        let mut changed = self.cache.len() != before;
        while let Ok((ip, label)) = self.rx.try_recv() {
            self.pending.remove(&ip);
            if self.cache.len() >= 2048 && !self.cache.contains_key(&ip) {
                self.cache.retain(|_, r| r.until > Instant::now());
                if self.cache.len() >= 2048
                    && let Some(old) = self.cache.keys().next().copied()
                {
                    self.cache.remove(&old);
                }
            }
            let ttl = if label.is_some() { 900 } else { 120 };
            self.cache.insert(
                ip,
                Cached {
                    label,
                    until: Instant::now() + Duration::from_secs(ttl),
                },
            );
            changed = true;
        }
        changed
    }
    pub fn labels(&self) -> HashMap<String, String> {
        self.cache
            .iter()
            .filter_map(|(ip, c)| c.label.as_ref().map(|s| (ip.to_string(), s.clone())))
            .collect()
    }
}
impl Drop for Resolver {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.tx.take();
    }
}
fn lookup(ip: IpAddr) -> Option<String> {
    if ip.is_loopback() {
        return Some("localhost".into());
    }
    if ip.is_unspecified() || ip.is_multicast() {
        return None;
    }
    let mut host = [0u16; 1025];
    let result = match ip {
        IpAddr::V4(v) => {
            let mut addr = SOCKADDR_IN {
                sin_family: AF_INET,
                ..Default::default()
            };
            addr.sin_addr.S_un.S_addr = u32::from_ne_bytes(v.octets());
            // SAFETY: the IPv4 address and output buffer remain valid for the synchronous call.
            unsafe {
                GetNameInfoW(
                    (&addr as *const SOCKADDR_IN).cast(),
                    std::mem::size_of::<SOCKADDR_IN>() as i32,
                    host.as_mut_ptr(),
                    host.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    NI_NAMEREQD as i32,
                )
            }
        }
        IpAddr::V6(v) => {
            let mut addr = SOCKADDR_IN6 {
                sin6_family: AF_INET6,
                ..Default::default()
            };
            addr.sin6_addr.u.Byte = v.octets();
            // SAFETY: the IPv6 address and output buffer remain valid for the synchronous call.
            unsafe {
                GetNameInfoW(
                    (&addr as *const SOCKADDR_IN6).cast(),
                    std::mem::size_of::<SOCKADDR_IN6>() as i32,
                    host.as_mut_ptr(),
                    host.len() as u32,
                    std::ptr::null_mut(),
                    0,
                    NI_NAMEREQD as i32,
                )
            }
        }
    };
    if result != 0 {
        return None;
    }
    let len = host.iter().position(|c| *c == 0).unwrap_or(host.len());
    let label = String::from_utf16_lossy(&host[..len])
        .trim_end_matches('.')
        .to_lowercase();
    if label.is_empty() || label.parse::<IpAddr>().is_ok() {
        None
    } else {
        Some(label)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_names_need_no_network_query() {
        assert_eq!(
            lookup("127.0.0.1".parse().unwrap()).as_deref(),
            Some("localhost")
        );
        assert!(lookup("0.0.0.0".parse().unwrap()).is_none());
    }
    #[test]
    fn background_resolver_returns_local_names_without_blocking_the_caller() {
        let mut resolver = Resolver::new();
        resolver.request(&["127.0.0.1".into()]);
        let start = Instant::now();
        while resolver.labels().is_empty() && start.elapsed() < Duration::from_secs(2) {
            resolver.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            resolver.labels().get("127.0.0.1").map(String::as_str),
            Some("localhost")
        );
    }
    #[test]
    #[ignore = "Queries two public addresses through the current Windows DNS resolver"]
    fn native_dns_resolution_probe() {
        let mut resolver = Resolver::new();
        resolver.request(&["1.1.1.1".into(), "2606:4700:4700::1111".into()]);
        let start = Instant::now();
        while !resolver.pending.is_empty() && start.elapsed() < Duration::from_secs(10) {
            resolver.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            resolver.pending.is_empty(),
            "Native DNS queries did not complete in the probe window"
        );
        eprintln!("Native resolver names: {:?}", resolver.labels());
        assert!(
            resolver.labels().contains_key("1.1.1.1"),
            "The configured DNS resolver returned no IPv4 host name"
        );
    }
}
