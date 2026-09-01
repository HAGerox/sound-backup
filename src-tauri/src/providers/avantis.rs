use avantis_protocol::{
    backup_shows, list_stored_shows, test_connection, BackupBatchOutcome, BackupBatchRequest,
    BackupError, StoredShow, DEFAULT_AHNET_PORT,
};
use if_addrs::{get_if_addrs, IfAddr, IfOperStatus};
use std::{
    collections::{BTreeSet, VecDeque},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredAvantis {
    pub endpoint: String,
}

pub fn test(console_address: &str) -> Result<(), BackupError> {
    test_connection(console_address)
}

pub fn shows(console_address: &str) -> Result<Vec<StoredShow>, BackupError> {
    list_stored_shows(console_address)
}

pub fn backup(
    console_address: &str,
    show_names: Vec<String>,
    destination: PathBuf,
) -> Result<BackupBatchOutcome, BackupError> {
    backup_shows(&BackupBatchRequest {
        endpoint: console_address.to_string(),
        show_names,
        destination,
    })
}

pub fn discover(known_address: Option<&str>) -> Result<Vec<DiscoveredAvantis>, BackupError> {
    let mut candidates = BTreeSet::new();
    for interface in get_if_addrs()? {
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
            candidates.extend(scan_addresses(address.ip, address.prefixlen));
        }
    }

    let queue = Arc::new(Mutex::new(VecDeque::from_iter(candidates)));
    let (sender, receiver) = mpsc::channel();
    let worker_count = queue.lock().map(|items| items.len().min(32)).unwrap_or(0);
    let mut workers = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let sender = sender.clone();
        workers.push(thread::spawn(move || loop {
            let address = queue.lock().ok().and_then(|mut items| items.pop_front());
            let Some(address) = address else {
                break;
            };
            if TcpStream::connect_timeout(&address, Duration::from_millis(90)).is_ok() {
                let _ = sender.send(address);
            }
        }));
    }
    drop(sender);

    let mut possible = BTreeSet::new();
    for address in receiver {
        possible.insert(address.to_string());
    }
    for worker in workers {
        let _ = worker.join();
    }
    if let Some(address) = known_address
        .map(str::trim)
        .filter(|address| !address.is_empty())
    {
        possible.insert(address.to_string());
    }

    let mut found = Vec::new();
    for endpoint in possible {
        if test_connection(&endpoint).is_ok() {
            found.push(DiscoveredAvantis { endpoint });
        }
    }
    Ok(found)
}

fn scan_addresses(ip: Ipv4Addr, prefix_len: u8) -> Vec<SocketAddr> {
    let prefix_len = prefix_len.clamp(24, 30);
    let mask = u32::MAX << (32 - prefix_len);
    let ip_value = u32::from(ip);
    let network = ip_value & mask;
    let broadcast = network | !mask;
    (network + 1..broadcast)
        .filter(|candidate| *candidate != ip_value)
        .take(1022)
        .map(|candidate| SocketAddr::from((Ipv4Addr::from(candidate), DEFAULT_AHNET_PORT)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_the_active_subnet_but_not_the_local_address() {
        let addresses = scan_addresses(Ipv4Addr::new(192, 168, 50, 12), 24);
        assert_eq!(addresses.len(), 253);
        assert!(addresses.contains(&SocketAddr::from((
            Ipv4Addr::new(192, 168, 50, 1),
            DEFAULT_AHNET_PORT
        ))));
        assert!(!addresses.contains(&SocketAddr::from((
            Ipv4Addr::new(192, 168, 50, 12),
            DEFAULT_AHNET_PORT
        ))));
    }

    #[test]
    fn caps_wide_networks_to_the_local_slash_24() {
        let addresses = scan_addresses(Ipv4Addr::new(10, 24, 7, 8), 16);
        assert!(addresses.iter().all(|address| match address.ip() {
            std::net::IpAddr::V4(ip) => ip.octets()[..3] == [10, 24, 7],
            std::net::IpAddr::V6(_) => false,
        }));
    }
}
