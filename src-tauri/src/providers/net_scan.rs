use if_addrs::{get_if_addrs, IfAddr, IfOperStatus};
use std::{
    collections::{BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

/// Number of concurrent connection attempts during a subnet sweep.
const SCAN_WORKERS: usize = 32;
/// Per-address connect timeout. Stage gear answers immediately when present.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(90);
/// Cap so a mistyped prefix cannot turn into an unbounded sweep.
const MAX_CANDIDATES: usize = 1022;

/// Expand the local IPv4 subnets into unicast candidate sockets on `port`.
///
/// Prefixes are clamped to a `/24` (or tighter) so a wide network does not
/// produce a hostile scan.
pub fn candidate_addresses(port: u16) -> BTreeSet<SocketAddr> {
    let mut candidates = BTreeSet::new();
    for interface in get_if_addrs().unwrap_or_default() {
        if interface.is_loopback()
            || interface.is_p2p()
            || matches!(
                interface.oper_status,
                IfOperStatus::Down | IfOperStatus::NotPresent
            )
        {
            continue;
        }
        if let IfAddr::V4(address) = interface.addr {
            candidates.extend(scan_addresses(address.ip, address.prefixlen, port));
        }
    }
    candidates
}

/// Sweep the candidate set and return the addresses that accepted a connection.
pub fn open_listeners(port: u16) -> Vec<SocketAddr> {
    let queue = Arc::new(Mutex::new(VecDeque::from_iter(candidate_addresses(port))));
    let (sender, receiver) = mpsc::channel();
    let worker_count = queue.lock().map(|items| items.len().min(SCAN_WORKERS)).unwrap_or(0);
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let sender = sender.clone();
        workers.push(thread::spawn(move || loop {
            let address = queue.lock().ok().and_then(|mut items| items.pop_front());
            let Some(address) = address else {
                break;
            };
            if TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).is_ok() {
                let _ = sender.send(address);
            }
        }));
    }
    drop(sender);

    let mut possible = BTreeSet::new();
    for address in receiver {
        possible.insert(address);
    }
    for worker in workers {
        let _ = worker.join();
    }
    possible.into_iter().collect()
}

fn scan_addresses(ip: Ipv4Addr, prefix_len: u8, port: u16) -> Vec<SocketAddr> {
    let prefix_len = prefix_len.clamp(24, 30);
    let mask = u32::MAX << (32 - prefix_len);
    let ip_value = u32::from(ip);
    let network = ip_value & mask;
    let broadcast = network | !mask;
    (network + 1..broadcast)
        .filter(|candidate| *candidate != ip_value)
        .take(MAX_CANDIDATES)
        .map(|candidate| SocketAddr::from((Ipv4Addr::from(candidate), port)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_the_active_subnet_but_not_the_local_address() {
        let addresses = scan_addresses(Ipv4Addr::new(192, 168, 50, 12), 24, 51321);
        assert_eq!(addresses.len(), 253);
        assert!(addresses.contains(&SocketAddr::from((
            Ipv4Addr::new(192, 168, 50, 1),
            51321
        ))));
        assert!(!addresses.contains(&SocketAddr::from((
            Ipv4Addr::new(192, 168, 50, 12),
            51321
        ))));
    }

    #[test]
    fn caps_wide_networks_to_the_local_slash_24() {
        let addresses = scan_addresses(Ipv4Addr::new(10, 24, 7, 8), 16, 8080);
        assert!(!addresses.is_empty());
        assert!(addresses.iter().all(|address| match address.ip() {
            std::net::IpAddr::V4(ip) => ip.octets()[..3] == [10, 24, 7] && address.port() == 8080,
            std::net::IpAddr::V6(_) => false,
        }));
    }
}
