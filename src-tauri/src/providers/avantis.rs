use avantis_protocol::{
    backup_shows, find_consoles, list_stored_shows, set_trace_sink, test_connection,
    BackupBatchOutcome, BackupBatchRequest, BackupError, FinderRoute, StoredShow, DISCOVERY_PORT,
};
use if_addrs::{get_if_addrs, IfAddr, IfOperStatus};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Director repeats its finder broadcast; a console normally answers the first one.
const DISCOVERY_WINDOW: Duration = Duration::from_millis(1600);
const TRACE_LOG_LIMIT: u64 = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredAvantis {
    pub endpoint: String,
    pub name: String,
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

/// Find consoles the way Director does: a UDP finder broadcast on every active IPv4 interface.
/// A saved address is also asked directly, so a console on a routed network that answers unicast
/// finders is still listed, and is otherwise checked with an AH-Net connection.
pub fn discover(known_address: Option<&str>) -> Result<Vec<DiscoveredAvantis>, BackupError> {
    let known_address = known_address
        .map(str::trim)
        .filter(|address| !address.is_empty());
    let known_ip = known_address.and_then(|address| address.parse::<Ipv4Addr>().ok());

    let mut routes = finder_routes();
    if let Some(ip) = known_ip {
        let target = SocketAddr::from((ip, DISCOVERY_PORT));
        match routes.first_mut() {
            Some(route) => route.targets.push(target),
            None => routes.push(FinderRoute {
                local: Ipv4Addr::UNSPECIFIED,
                targets: vec![target],
            }),
        }
    }

    let mut found: Vec<DiscoveredAvantis> = find_consoles(&routes, DISCOVERY_WINDOW)?
        .into_iter()
        .map(|console| DiscoveredAvantis {
            endpoint: console.address.to_string(),
            name: console.name,
        })
        .collect();

    if let Some(address) = known_address {
        let answered = found.iter().any(|device| same_host(&device.endpoint, address));
        if !answered && test_connection(address).is_ok() {
            found.push(DiscoveredAvantis {
                endpoint: address.to_string(),
                name: "Avantis".to_string(),
            });
        }
    }
    Ok(found)
}

/// Append protocol trace lines to `avantis.log` so a failed connection can be diagnosed.
pub fn install_trace_log(directory: PathBuf) {
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join("avantis.log");
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() > TRACE_LOG_LIMIT) {
        let _ = fs::rename(&path, directory.join("avantis.previous.log"));
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let file = Mutex::new(file);
    set_trace_sink(move |line| {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs_f64())
            .unwrap_or_default();
        if let Ok(mut file) = file.lock() {
            let _ = writeln!(file, "{seconds:.3} {line}");
        }
    });
}

fn finder_routes() -> Vec<FinderRoute> {
    let mut routes = Vec::new();
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
        let IfAddr::V4(address) = interface.addr else {
            continue;
        };
        let broadcast = address
            .broadcast
            .unwrap_or_else(|| subnet_broadcast(address.ip, address.prefixlen));
        routes.push(FinderRoute::broadcast(address.ip, broadcast));
    }
    routes
}

fn subnet_broadcast(ip: Ipv4Addr, prefix_len: u8) -> Ipv4Addr {
    let mask = u32::MAX.checked_shl(32 - u32::from(prefix_len.min(32))).unwrap_or(0);
    Ipv4Addr::from(u32::from(ip) | !mask)
}

fn same_host(endpoint: &str, address: &str) -> bool {
    let host = |value: &str| -> Option<IpAddr> {
        value
            .parse::<IpAddr>()
            .ok()
            .or_else(|| value.parse::<SocketAddr>().ok().map(|socket| socket.ip()))
    };
    match (host(endpoint), host(address)) {
        (Some(left), Some(right)) => left == right,
        _ => endpoint.eq_ignore_ascii_case(address),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_tests_reject_a_quiet_port() {
        // Bound so the address exists but never completes an AH-Net handshake.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(test(&address).is_err());
    }

    #[test]
    fn computes_subnet_broadcasts() {
        assert_eq!(
            subnet_broadcast(Ipv4Addr::new(192, 168, 1, 54), 24),
            Ipv4Addr::new(192, 168, 1, 255)
        );
        assert_eq!(
            subnet_broadcast(Ipv4Addr::new(10, 10, 160, 185), 16),
            Ipv4Addr::new(10, 10, 255, 255)
        );
    }

    #[test]
    fn matches_saved_addresses_with_or_without_a_port() {
        assert!(same_host("192.168.1.109", "192.168.1.109"));
        assert!(same_host("192.168.1.109", "192.168.1.109:51321"));
        assert!(!same_host("192.168.1.109", "192.168.1.110"));
    }
}
