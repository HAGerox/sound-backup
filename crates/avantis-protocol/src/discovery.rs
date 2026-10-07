use crate::Result;
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    time::{Duration, Instant},
};

/// UDP port Allen & Heath units listen on for finder broadcasts.
pub const DISCOVERY_PORT: u16 = 51_320;

/// Finder requests Director V2.01 broadcasts for mixing consoles. Director appends the `V2`
/// flag to every non-legacy finder, so a console answers with its unit name.
const FIND_REQUESTS: [&[u8]; 2] = [b"TLD Find\0V2\0", b"Squirrel Find\0V2\0"];
const RESEND_INTERVAL: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredConsole {
    pub address: IpAddr,
    pub name: String,
}

/// One local IPv4 interface to search from, and where its finder datagrams are sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinderRoute {
    pub local: Ipv4Addr,
    pub targets: Vec<SocketAddr>,
}

impl FinderRoute {
    /// Broadcast on the interface's subnet, as Director does.
    pub fn broadcast(local: Ipv4Addr, broadcast: Ipv4Addr) -> Self {
        Self {
            local,
            targets: vec![SocketAddr::from((broadcast, DISCOVERY_PORT))],
        }
    }
}

/// Broadcast Director's console finder requests and collect the units that answer.
///
/// Units reply to the sending socket with their unit name as a NUL-terminated string; the
/// datagram source is the console address.
pub fn find_consoles(routes: &[FinderRoute], duration: Duration) -> Result<Vec<DiscoveredConsole>> {
    let mut sockets = Vec::new();
    for route in routes.iter().filter(|route| !route.targets.is_empty()) {
        // An interface can disappear between enumeration and bind; search the others.
        let Ok(socket) = UdpSocket::bind((route.local, 0)) else {
            continue;
        };
        socket.set_broadcast(true)?;
        socket.set_nonblocking(true)?;
        sockets.push((socket, &route.targets));
    }

    let mut found = BTreeMap::new();
    let deadline = Instant::now() + duration;
    let mut next_send = Instant::now();
    while Instant::now() < deadline {
        if Instant::now() >= next_send {
            for (socket, targets) in &sockets {
                for target in targets.iter() {
                    for request in FIND_REQUESTS {
                        // Routes without a usable broadcast path just stay quiet.
                        let _ = socket.send_to(request, target);
                    }
                }
            }
            next_send = Instant::now() + RESEND_INTERVAL;
        }

        let mut received = false;
        for (socket, _) in &sockets {
            let mut buffer = [0u8; 512];
            while let Ok((length, source)) = socket.recv_from(&mut buffer) {
                received = true;
                if let Some(name) = parse_reply(&buffer[..length]) {
                    found.entry(source.ip()).or_insert(name);
                }
            }
        }
        if !received {
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    Ok(found
        .into_iter()
        .map(|(address, name)| DiscoveredConsole { address, name })
        .collect())
}

fn parse_reply(datagram: &[u8]) -> Option<String> {
    // Finder requests from other Director instances share the port range; never treat them as
    // a unit answering.
    if datagram.windows(5).any(|window| window == b" Find") {
        return None;
    }
    let end = datagram
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(datagram.len());
    let name: String = datagram[..end]
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_control())
        .map(char::from)
        .collect();
    let name = name.trim();
    Some(if name.is_empty() { "Avantis" } else { name }.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_unit_name_replies() {
        assert_eq!(parse_reply(b"FOH Avantis\0").as_deref(), Some("FOH Avantis"));
        assert_eq!(parse_reply(b"\0").as_deref(), Some("Avantis"));
        assert_eq!(parse_reply(b"Bridge Find\0V2\0"), None);
    }

    #[test]
    fn finds_a_unit_that_answers_the_finder() {
        let unit = UdpSocket::bind("127.0.0.1:0").unwrap();
        let unit_address = unit.local_addr().unwrap();
        let responder = std::thread::spawn(move || {
            let mut buffer = [0u8; 64];
            let (length, client) = unit.recv_from(&mut buffer).unwrap();
            assert!(FIND_REQUESTS.contains(&&buffer[..length]));
            unit.send_to(b"Stage Left\0", client).unwrap();
        });

        let consoles = find_consoles(
            &[FinderRoute {
                local: Ipv4Addr::LOCALHOST,
                targets: vec![unit_address],
            }],
            Duration::from_millis(500),
        )
        .unwrap();
        responder.join().unwrap();
        assert_eq!(
            consoles,
            vec![DiscoveredConsole {
                address: IpAddr::V4(Ipv4Addr::LOCALHOST),
                name: "Stage Left".into(),
            }]
        );
    }
}
