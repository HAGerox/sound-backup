use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::{
    collections::BTreeMap,
    net::Ipv4Addr,
    time::{Duration, Instant},
};

/// A resolved mDNS service that fronts an HTTP control plane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpService {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub local: bool,
    pub properties: BTreeMap<String, String>,
}

impl HttpService {
    /// Base URL for the service's HTTP API.
    pub fn endpoint(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// Browse a service type for `window`, resolving each service as it appears.
pub fn browse(service_type: &str, window: Duration) -> Result<Vec<HttpService>, String> {
    let daemon =
        ServiceDaemon::new().map_err(|error| format!("Bonjour could not start: {error}"))?;
    let receiver = daemon
        .browse(service_type)
        .map_err(|error| format!("Discovery could not start: {error}"))?;
    let deadline = Instant::now() + window;
    let mut found = BTreeMap::new();
    while Instant::now() < deadline {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        if let Ok(ServiceEvent::ServiceResolved(info)) = receiver.recv_timeout(timeout) {
            let Some(address) = preferred_address(info.get_addresses_v4()) else {
                continue;
            };
            let port = info.get_port();
            let local = is_local_address(&address);
            let name = service_name(info.get_fullname(), service_type);
            let properties = info
                .get_properties()
                .iter()
                .map(|property| (property.key().to_string(), property.val_str().to_string()))
                .collect();
            found.insert(
                format!("{address}:{port}"),
                HttpService {
                    name,
                    host: address,
                    port,
                    local,
                    properties,
                },
            );
        }
    }
    let _ = daemon.stop_browse(service_type);
    let _ = daemon.shutdown();
    Ok(found.into_values().collect())
}

fn service_name(fullname: &str, service_type: &str) -> String {
    fullname
        .strip_suffix(&format!(".{service_type}"))
        .or_else(|| fullname.strip_suffix(service_type))
        .unwrap_or(fullname)
        .to_string()
}

fn preferred_address(addresses: std::collections::HashSet<Ipv4Addr>) -> Option<String> {
    let mut candidates: Vec<String> = addresses
        .iter()
        .filter(|ip| !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified())
        .map(|ip| ip.to_string())
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .or_else(|| addresses.iter().map(|ip| ip.to_string()).min())
}

fn is_local_address(address: &str) -> bool {
    if address == "127.0.0.1" || address == "::1" {
        return true;
    }
    let Ok(parsed) = address.parse::<Ipv4Addr>() else {
        return false;
    };
    if parsed.is_loopback() {
        return true;
    }
    if_addrs::get_if_addrs().unwrap_or_default().iter().any(|interface| {
        matches!(&interface.addr, if_addrs::IfAddr::V4(ip) if ip.ip == parsed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_http_endpoint() {
        let service = HttpService {
            name: "front-of-house".to_string(),
            host: "192.168.130.18".to_string(),
            port: 8080,
            local: false,
            properties: BTreeMap::new(),
        };
        assert_eq!(service.endpoint(), "http://192.168.130.18:8080");
    }

    #[test]
    fn strips_the_service_type_suffix_from_names() {
        assert_eq!(
            service_name("front-of-house._slink-rack._tcp.local.", "_slink-rack._tcp.local."),
            "front-of-house"
        );
        assert_eq!(service_name("Sunday._micwise._tcp.local.", "_micwise._tcp.local."), "Sunday");
    }

    #[test]
    fn recognises_loopback_as_local() {
        assert!(is_local_address("127.0.0.1"));
        assert!(!is_local_address("192.168.130.18"));
    }

    #[test]
    fn prefers_a_global_unicast_address() {
        let mut addresses = std::collections::HashSet::new();
        addresses.insert(Ipv4Addr::new(169, 254, 4, 5));
        addresses.insert(Ipv4Addr::new(192, 168, 130, 18));
        assert_eq!(
            preferred_address(addresses),
            Some("192.168.130.18".to_string())
        );
    }
}
