use super::{
    remote::{self, Connection},
    util::{dated_path, shell_quote, temporary_remote_path},
};
use if_addrs::get_if_addrs;
use keyring::Entry;
use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};
use rosc::{decoder, encoder, OscMessage, OscPacket, OscType};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs},
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

const QLAB_PASSCODE_SERVICE: &str = "uk.stagebackup.qlab-osc";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub host: String,
    pub address: String,
    pub hostname: String,
    pub name: String,
    pub osc_port: u16,
    pub local: bool,
    pub workspace_names: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub port: u16,
    pub version: String,
}

#[derive(Clone, Debug)]
pub struct BackupOutcome {
    pub path: PathBuf,
    pub bytes: u64,
    pub workspace_name: String,
}

pub fn discover() -> Result<Vec<Instance>, String> {
    let daemon =
        ServiceDaemon::new().map_err(|error| format!("Bonjour could not start: {error}"))?;
    let receiver = daemon
        .browse("_qlab._tcp.local.")
        .map_err(|error| format!("QLab discovery could not start: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut found = BTreeMap::new();
    while Instant::now() < deadline {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250));
        if let Ok(ServiceEvent::ServiceResolved(info)) = receiver.recv_timeout(timeout) {
            let Some(address) = preferred_address(info.get_addresses()) else {
                continue;
            };
            let port = info.get_port();
            let local = is_local_address(&address);
            let host = if local {
                "127.0.0.1".to_string()
            } else {
                address.clone()
            };
            let hostname = clean_hostname(info.get_hostname());
            found.insert(
                if local {
                    format!("local:{port}")
                } else {
                    format!("{host}:{port}")
                },
                Instance {
                    host,
                    address,
                    hostname,
                    name: service_name(info.get_fullname()),
                    osc_port: port,
                    local,
                    workspace_names: Vec::new(),
                },
            );
        }
    }
    let _ = daemon.stop_browse("_qlab._tcp.local.");
    let _ = daemon.shutdown();

    if found.is_empty() {
        if let Ok(workspaces) = list_workspaces("127.0.0.1", 53000) {
            if !workspaces.is_empty() {
                found.insert(
                    "127.0.0.1:53000".to_string(),
                    Instance {
                        host: "127.0.0.1".to_string(),
                        address: preferred_local_address(),
                        hostname: computer_hostname(),
                        name: "QLab on this Mac".to_string(),
                        osc_port: 53000,
                        local: true,
                        workspace_names: workspaces
                            .iter()
                            .map(|workspace| workspace.name.clone())
                            .collect(),
                    },
                );
            }
        }
    }
    let mut instances: Vec<_> = found.into_values().collect();
    for instance in &mut instances {
        if instance.workspace_names.is_empty() {
            instance.workspace_names = list_workspaces(&instance.host, instance.osc_port)
                .unwrap_or_default()
                .into_iter()
                .map(|workspace| workspace.name)
                .collect();
        }
    }
    Ok(instances)
}

pub fn list_workspaces(host: &str, port: u16) -> Result<Vec<Workspace>, String> {
    let mut client = OscClient::new(host)?;
    let reply = client.query(port_or_default(port), "/workspaces", Vec::new())?;
    let data = reply_data(reply)?;
    let entries = data
        .as_array()
        .ok_or_else(|| "QLab returned an unexpected workspace list.".to_string())?;
    Ok(entries
        .iter()
        .filter_map(|entry| {
            Some(Workspace {
                id: entry.get("uniqueID")?.as_str()?.to_string(),
                name: entry.get("displayName")?.as_str()?.to_string(),
                port: entry
                    .get("port")
                    .and_then(Value::as_u64)
                    .and_then(|port| u16::try_from(port).ok())
                    .unwrap_or(port_or_default(port)),
                version: entry
                    .get("version")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })
        })
        .collect())
}

pub fn authorise(
    instance: &Instance,
    workspace: &Workspace,
    passcode: Option<&str>,
) -> Result<String, String> {
    let mut client = OscClient::new(&instance.host)?;
    let supplied = passcode.filter(|passcode| !passcode.is_empty());
    let stored = if supplied.is_none() {
        load_passcode(instance, workspace)?
    } else {
        None
    };
    let passcode = supplied.or(stored.as_deref());
    connect_workspace(&mut client, workspace, passcode)?;
    let base_path = workspace_base_path(&mut client, workspace)?;
    if let Some(passcode) = supplied {
        save_passcode(instance, workspace, passcode)?;
    }
    Ok(base_path)
}

pub fn backup(
    instance: &Instance,
    connection: &Connection,
    workspaces: &[Workspace],
    destination: &Path,
) -> Result<Vec<BackupOutcome>, String> {
    if workspaces.is_empty() {
        return Err("Choose at least one open QLab workspace.".to_string());
    }
    let output_folder = destination.join("QLab");
    fs::create_dir_all(&output_folder)
        .map_err(|error| format!("Could not create the QLab backup folder: {error}"))?;
    let remote_session = if instance.local {
        None
    } else {
        Some(remote::connect(connection, None)?.session)
    };
    let mut client = OscClient::new(&instance.host)?;
    let mut outcomes = Vec::with_capacity(workspaces.len());

    for workspace in workspaces {
        let passcode = load_passcode(instance, workspace)?;
        connect_workspace(&mut client, workspace, passcode.as_deref())?;
        let base_path = workspace_base_path(&mut client, workspace)?;
        validate_base_path(&base_path)?;
        client.send(
            workspace.port,
            &format!("/workspace/{}/save", workspace.id),
            Vec::new(),
        )?;
        thread::sleep(Duration::from_millis(600));

        let project_name = workspace
            .name
            .strip_suffix(".qlab5")
            .unwrap_or(&workspace.name);
        let output = dated_path(&output_folder, project_name, "QLab Workspace", "zip");
        let bytes = if let Some(session) = remote_session.as_ref() {
            remote_bundle(session, &base_path, &output)?
        } else {
            local_bundle(&base_path, &output)?
        };
        outcomes.push(BackupOutcome {
            path: output,
            bytes,
            workspace_name: workspace.name.clone(),
        });
    }
    Ok(outcomes)
}

fn connect_workspace(
    client: &mut OscClient,
    workspace: &Workspace,
    passcode: Option<&str>,
) -> Result<(), String> {
    let arguments = passcode
        .filter(|passcode| !passcode.is_empty())
        .map(|passcode| vec![OscType::String(passcode.to_string())])
        .unwrap_or_default();
    let reply = client.query(
        workspace.port,
        &format!("/workspace/{}/connect", workspace.id),
        arguments,
    )?;
    let status = reply.get("status").and_then(Value::as_str).unwrap_or("ok");
    let data = reply
        .get("data")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if status == "denied" || data == "badpass" {
        return Err(
            "QLab needs its OSC passcode. Enter it in Settings, then try again.".to_string(),
        );
    }
    if status == "error" {
        return Err("QLab refused the connection to this workspace.".to_string());
    }
    Ok(())
}

fn workspace_base_path(client: &mut OscClient, workspace: &Workspace) -> Result<String, String> {
    let reply = client.query(
        workspace.port,
        &format!("/workspace/{}/basePath", workspace.id),
        Vec::new(),
    )?;
    let data = reply_data(reply)?;
    let path = data.as_str().unwrap_or_default().trim().to_string();
    if path.is_empty() {
        Err("Save this QLab workspace once, then scan again.".to_string())
    } else {
        Ok(path)
    }
}

fn reply_data(reply: Value) -> Result<Value, String> {
    match reply.get("status").and_then(Value::as_str) {
        Some("denied") => {
            Err("QLab needs its OSC passcode. Enter it in Settings, then try again.".to_string())
        }
        Some("error") => Err(reply
            .get("data")
            .and_then(Value::as_str)
            .unwrap_or("QLab could not complete that request.")
            .to_string()),
        _ => Ok(reply.get("data").cloned().unwrap_or(Value::Null)),
    }
}

fn local_bundle(base_path: &str, output: &Path) -> Result<u64, String> {
    let status = Command::new("/usr/bin/ditto")
        .args(["-c", "-k", "--sequesterRsrc", "--keepParent", base_path])
        .arg(output)
        .status()
        .map_err(|error| format!("Could not start the QLab bundle: {error}"))?;
    if !status.success() {
        let _ = fs::remove_file(output);
        return Err("QLab’s project folder could not be bundled.".to_string());
    }
    fs::metadata(output)
        .map(|metadata| metadata.len())
        .map_err(|error| format!("Could not read the completed QLab backup: {error}"))
}

fn remote_bundle(session: &ssh2::Session, base_path: &str, output: &Path) -> Result<u64, String> {
    let remote_archive = temporary_remote_path("qlab", "zip");
    let command = format!(
        "/usr/bin/ditto -c -k --sequesterRsrc --keepParent {} {}",
        shell_quote(base_path),
        shell_quote(&remote_archive)
    );
    remote::run(session, &command)
        .map_err(|error| format!("QLab’s project folder could not be bundled: {error}"))?;
    let transfer = remote::download(session, &remote_archive, output);
    let _ = remote::run(
        session,
        &format!("/bin/rm -f -- {}", shell_quote(&remote_archive)),
    );
    match transfer {
        Ok(bytes) => Ok(bytes),
        Err(error) => {
            let _ = fs::remove_file(output);
            Err(error)
        }
    }
}

fn validate_base_path(path: &str) -> Result<(), String> {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        Err("QLab reported an unsafe project folder. Save the workspace into its own project folder first.".to_string())
    } else {
        Ok(())
    }
}

fn passcode_account(instance: &Instance, workspace: &Workspace) -> String {
    format!(
        "{}:{}:{}",
        instance.host.to_ascii_lowercase(),
        instance.osc_port,
        workspace.id
    )
}

fn save_passcode(instance: &Instance, workspace: &Workspace, passcode: &str) -> Result<(), String> {
    Entry::new(
        QLAB_PASSCODE_SERVICE,
        &passcode_account(instance, workspace),
    )
    .map_err(|error| format!("Could not open Keychain: {error}"))?
    .set_password(passcode)
    .map_err(|error| format!("Could not save the QLab passcode in Keychain: {error}"))
}

fn load_passcode(instance: &Instance, workspace: &Workspace) -> Result<Option<String>, String> {
    let entry = Entry::new(
        QLAB_PASSCODE_SERVICE,
        &passcode_account(instance, workspace),
    )
    .map_err(|error| format!("Could not open Keychain: {error}"))?;
    match entry.get_password() {
        Ok(passcode) => Ok(Some(passcode)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "Could not read the QLab passcode from Keychain: {error}"
        )),
    }
}

struct OscClient {
    host: String,
    streams: BTreeMap<u16, TcpStream>,
}

impl OscClient {
    fn new(host: &str) -> Result<Self, String> {
        Ok(Self {
            host: host.to_string(),
            streams: BTreeMap::new(),
        })
    }

    fn send(&mut self, port: u16, address: &str, args: Vec<OscType>) -> Result<(), String> {
        let packet = OscPacket::Message(OscMessage {
            addr: address.to_string(),
            args,
        });
        let encoded = encoder::encode(&packet)
            .map_err(|error| format!("Could not encode the QLab request: {error}"))?;
        let framed = slip_encode(&encoded);
        self.stream(port)?
            .write_all(&framed)
            .map_err(|error| format!("Could not send the request to QLab: {error}"))?;
        Ok(())
    }

    fn query(&mut self, port: u16, address: &str, args: Vec<OscType>) -> Result<Value, String> {
        self.send(port, address, args)?;
        let frame = slip_read(self.stream(port)?)?;
        let (_, packet) = decoder::decode_udp(&frame)
            .map_err(|error| format!("QLab returned an unreadable OSC packet: {error}"))?;
        if let Some(json) = packet_json(packet) {
            return serde_json::from_str(&json)
                .map_err(|error| format!("QLab returned an unreadable response: {error}"));
        }
        Err("QLab returned a response Stage Backup did not recognise.".to_string())
    }

    fn stream(&mut self, port: u16) -> Result<&mut TcpStream, String> {
        let port = port_or_default(port);
        if !self.streams.contains_key(&port) {
            let target = resolve_tcp(&self.host, port)?;
            let stream = TcpStream::connect_timeout(&target, Duration::from_secs(2))
                .map_err(|error| format!("QLab did not respond: {error}"))?;
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .map_err(|error| format!("Could not configure the QLab connection: {error}"))?;
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .map_err(|error| format!("Could not configure the QLab connection: {error}"))?;
            self.streams.insert(port, stream);
        }
        self.streams
            .get_mut(&port)
            .ok_or_else(|| "Could not retain the QLab connection.".to_string())
    }
}

fn packet_json(packet: OscPacket) -> Option<String> {
    match packet {
        OscPacket::Message(message) if message.addr.starts_with("/reply") => message
            .args
            .into_iter()
            .find_map(|argument| match argument {
                OscType::String(value) => Some(value),
                _ => None,
            }),
        OscPacket::Bundle(bundle) => bundle.content.into_iter().find_map(packet_json),
        _ => None,
    }
}

fn resolve_tcp(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("Could not resolve {host}: {error}"))?
        .find(|address| address.is_ipv4())
        .ok_or_else(|| format!("Could not find an IPv4 address for {host}."))
}

fn slip_encode(packet: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(packet.len() + 2);
    framed.push(0xc0);
    for byte in packet {
        match byte {
            0xc0 => framed.extend_from_slice(&[0xdb, 0xdc]),
            0xdb => framed.extend_from_slice(&[0xdb, 0xdd]),
            byte => framed.push(*byte),
        }
    }
    framed.push(0xc0);
    framed
}

fn slip_read(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    let mut frame = Vec::new();
    let mut started = false;
    let mut escaped = false;
    let mut byte = [0_u8; 1];
    loop {
        stream
            .read_exact(&mut byte)
            .map_err(|error| format!("Could not read QLab’s response: {error}"))?;
        match byte[0] {
            0xc0 if started && !frame.is_empty() => return Ok(frame),
            0xc0 => started = true,
            _ if !started => {}
            0xdc if escaped => {
                frame.push(0xc0);
                escaped = false;
            }
            0xdd if escaped => {
                frame.push(0xdb);
                escaped = false;
            }
            0xdb => escaped = true,
            value => {
                if escaped {
                    frame.push(0xdb);
                    escaped = false;
                }
                frame.push(value);
            }
        }
    }
}

fn port_or_default(port: u16) -> u16 {
    if port == 0 {
        53000
    } else {
        port
    }
}

fn preferred_address(addresses: &HashSet<ScopedIp>) -> Option<String> {
    addresses
        .iter()
        .find(|address| address.is_ipv4() && !address.is_loopback())
        .or_else(|| addresses.iter().find(|address| !address.is_loopback()))
        .map(ToString::to_string)
}

fn is_local_address(host: &str) -> bool {
    let Ok(address) = host.split('%').next().unwrap_or(host).parse::<IpAddr>() else {
        return false;
    };
    address.is_loopback()
        || get_if_addrs()
            .map(|interfaces| {
                interfaces
                    .into_iter()
                    .any(|interface| interface.ip() == address)
            })
            .unwrap_or(false)
}

fn preferred_local_address() -> String {
    get_if_addrs()
        .ok()
        .and_then(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| interface.ip())
                .find(|address| address.is_ipv4() && !address.is_loopback())
        })
        .map(|address| address.to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

fn service_name(fullname: &str) -> String {
    fullname
        .strip_suffix("._qlab._tcp.local.")
        .unwrap_or(fullname)
        .trim_end_matches('.')
        .replace("\\032", " ")
}

fn clean_hostname(hostname: &str) -> String {
    hostname.trim().trim_end_matches('.').to_string()
}

fn computer_hostname() -> String {
    Command::new("/bin/hostname")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| clean_hostname(&String::from_utf8_lossy(&output.stdout)))
        .filter(|hostname| !hostname.is_empty())
        .unwrap_or_else(|| "This Mac".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_qlab_reply_json() {
        let packet = OscPacket::Message(OscMessage {
            addr: "/reply/workspaces".to_string(),
            args: vec![OscType::String(
                "{\"status\":\"ok\",\"data\":[]}".to_string(),
            )],
        });
        assert_eq!(
            packet_json(packet).as_deref(),
            Some("{\"status\":\"ok\",\"data\":[]}")
        );
    }

    #[test]
    fn slip_round_trips_osc_bytes() {
        let bytes = [0x2f, 0xc0, 0xdb, 0x00];
        assert_eq!(
            slip_encode(&bytes),
            [0xc0, 0x2f, 0xdb, 0xdc, 0xdb, 0xdd, 0x00, 0xc0]
        );
    }

    #[test]
    fn refuses_root_as_a_project_folder() {
        assert!(validate_base_path("/").is_err());
        assert!(validate_base_path("/Users/finn/Show").is_ok());
    }

    #[test]
    #[ignore = "requires QLab to be open on this Mac"]
    fn discovers_local_qlab_by_open_workspace_name() {
        let workspaces = list_workspaces("127.0.0.1", 53000).unwrap();
        assert!(!workspaces.is_empty());
        assert!(workspaces.iter().all(|workspace| !workspace.id.is_empty()));

        let names: HashSet<_> = workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect();
        let instances = discover().unwrap();

        let instance = instances
            .iter()
            .find(|instance| {
                instance
                    .workspace_names
                    .iter()
                    .any(|name| names.contains(name.as_str()))
            })
            .expect("the local QLab instance should include its open workspace names");
        assert!(instance.local);
        assert_eq!(instance.host, "127.0.0.1");
        assert!(!instance.address.is_empty());
        assert!(!instance.hostname.is_empty());
    }
}
