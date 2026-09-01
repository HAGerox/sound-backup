use crate::{
    filename::{dated_archive_name, usb_show_directory},
    show::ShowKey,
    wire::{
        be_u16, be_u32, decode_datagram, encode_net, encode_util, AhNetVersion, NetMessage,
        WireDecoder, WireFrame, DEFAULT_AHNET_PORT,
    },
    BackupError, Result,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

const LOCAL_OBJECT: u16 = 0x7FFE;
const REGISTRY_FIND_OBJECT: u16 = 4;
const REGISTRY_FOUND_OBJECT: u16 = 2;
const REGISTRY_OBJECT_NOT_FOUND: u16 = 3;
const SHOW_MANAGER_SYNC: u16 = 0x100;
const SHOW_ADDED: u16 = 0x1001;
const SHOW_SYNC_READY: u16 = 0x100C;
const SHOW_UPLOAD_TO_CONNECTION: u16 = 0x118;
const FILE_BODY: u16 = 0x112;
const FILE_HEADER: u16 = 0x113;
const FILE_ACK: u16 = 2;
const FILE_ERROR: u16 = 3;
const MAX_SHOW_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct BackupRequest {
    pub endpoint: String,
    pub show_name: String,
    pub destination: PathBuf,
}

#[derive(Clone, Debug)]
pub struct BackupBatchRequest {
    pub endpoint: String,
    pub show_names: Vec<String>,
    pub destination: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupOutcome {
    pub path: PathBuf,
    pub bytes: u64,
    pub show_name: String,
    pub source_file_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupBatchOutcome {
    pub files: Vec<BackupOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredShow {
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Transport {
    Tcp,
    Udp,
}

#[derive(Debug)]
struct ReceivedNet {
    message: NetMessage,
    transport: Transport,
}

pub fn test_connection(endpoint: &str) -> Result<()> {
    let mut session = Session::connect(endpoint)?;
    session.discover_show_file_manager()?;
    Ok(())
}

pub fn list_stored_shows(endpoint: &str) -> Result<Vec<StoredShow>> {
    validate_endpoint(endpoint)?;
    let mut session = Session::connect(endpoint)?;
    let show_manager = session.discover_show_file_manager()?;
    Ok(session
        .list_stored_show_keys(show_manager)?
        .into_iter()
        .map(|key| StoredShow {
            name: key.name().to_string(),
        })
        .collect())
}

pub fn backup_show(request: &BackupRequest) -> Result<BackupOutcome> {
    let outcome = backup_shows(&BackupBatchRequest {
        endpoint: request.endpoint.clone(),
        show_names: vec![request.show_name.clone()],
        destination: request.destination.clone(),
    })?;
    outcome.files.into_iter().next().ok_or_else(|| {
        BackupError::Protocol("The Avantis backup completed without producing a file.".into())
    })
}

pub fn backup_shows(request: &BackupBatchRequest) -> Result<BackupBatchOutcome> {
    validate_batch_request(request)?;
    let mut session = Session::connect(&request.endpoint)?;
    let show_manager = session.discover_show_file_manager()?;
    let keys = session.list_stored_show_keys(show_manager)?;
    let available: Vec<String> = keys.iter().map(|key| key.name().to_string()).collect();
    let mut selected = Vec::new();
    let mut seen = BTreeSet::new();

    for requested in &request.show_names {
        let requested = requested.trim();
        let normalised = requested.to_ascii_lowercase();
        if !seen.insert(normalised) {
            continue;
        }
        let Some(key) = keys
            .iter()
            .find(|key| key.name().eq_ignore_ascii_case(requested))
        else {
            return Err(BackupError::ShowNotFound {
                requested: requested.to_string(),
                available,
            });
        };
        selected.push(key.clone());
    }

    let mut files = Vec::with_capacity(selected.len());
    for key in selected {
        files.push(session.download_show(show_manager, &key, &request.destination)?);
    }
    Ok(BackupBatchOutcome { files })
}

fn validate_endpoint(endpoint: &str) -> Result<()> {
    if endpoint.trim().is_empty() {
        return Err(BackupError::InvalidInput(
            "Enter the Avantis address.".into(),
        ));
    }
    Ok(())
}

fn validate_show_name(show: &str) -> Result<()> {
    let show = show.trim();
    if show.is_empty() {
        return Err(BackupError::InvalidInput(
            "Choose at least one stored Show.".into(),
        ));
    }
    if show.len() > 16 {
        return Err(BackupError::InvalidInput(
            "Avantis Show names are limited to 16 bytes.".into(),
        ));
    }
    if show.chars().any(|ch| ch.is_control()) {
        return Err(BackupError::InvalidInput(
            "The Show name contains a control character.".into(),
        ));
    }
    Ok(())
}

fn validate_batch_request(request: &BackupBatchRequest) -> Result<()> {
    validate_endpoint(&request.endpoint)?;
    if request.show_names.is_empty() {
        return Err(BackupError::InvalidInput(
            "Choose at least one stored Show.".into(),
        ));
    }
    for show in &request.show_names {
        validate_show_name(show)?;
    }
    if request.destination.as_os_str().is_empty() {
        return Err(BackupError::InvalidInput("Choose a backup folder.".into()));
    }
    Ok(())
}

struct Session {
    tcp: TcpStream,
    udp: UdpSocket,
    decoder: WireDecoder,
    queued: VecDeque<WireFrame>,
    version: AhNetVersion,
}

impl Session {
    fn connect(endpoint: &str) -> Result<Self> {
        let address = resolve_endpoint(endpoint)?;
        trace(format!("connect {address}"));
        let tcp =
            TcpStream::connect_timeout(&address, Duration::from_secs(3)).map_err(|error| {
                BackupError::Protocol(format!(
                    "Could not connect to Avantis at {address}: {error}"
                ))
            })?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(Duration::from_millis(80)))?;
        tcp.set_write_timeout(Some(Duration::from_secs(2)))?;

        let udp_bind = match address.ip() {
            IpAddr::V4(_) => "0.0.0.0:0",
            IpAddr::V6(_) => "[::]:0",
        };
        let udp = UdpSocket::bind(udp_bind)?;
        udp.set_read_timeout(Some(Duration::from_millis(20)))?;
        let local_udp_port = udp.local_addr()?.port();

        let mut session = Self {
            tcp,
            udp,
            decoder: WireDecoder::default(),
            queued: VecDeque::new(),
            version: AhNetVersion::V1,
        };

        session.send_util(&[
            0x01,
            0x03,
            (local_udp_port >> 8) as u8,
            local_udp_port as u8,
        ])?;
        let hello_deadline = Instant::now() + Duration::from_secs(2);
        let hello = session.wait_for_util(hello_deadline, |body| {
            body.len() >= 4 && body[0] == 0x02 && body[1] == 0x03
        })?;
        let remote_udp_port = be_u16(&hello[2..4]);
        if remote_udp_port != 0 {
            let remote_udp = SocketAddr::new(address.ip(), remote_udp_port);
            let _ = session.udp.connect(remote_udp);
            trace(format!("remote UDP {remote_udp}"));
        }

        // Director negotiates AH-Net v2 after the base hello. If the console does not answer,
        // continue in v1 so older firmware is still reachable.
        session.send_util(&[0x04, 0x03])?;
        let v2_deadline = Instant::now() + Duration::from_millis(650);
        match session.wait_for_util_optional(v2_deadline, |body| {
            body.len() >= 2 && body[0] == 0x05 && body[1] == 0x03
        })? {
            Some(_) => {
                session.version = AhNetVersion::V2;
                trace("AH-Net v2".into());
            }
            None => trace("AH-Net v1 fallback".into()),
        }
        Ok(session)
    }

    fn discover_show_file_manager(&mut self) -> Result<u16> {
        self.send_net_tcp(&NetMessage::new(
            0,
            LOCAL_OBJECT,
            REGISTRY_FIND_OBJECT,
            b"Show File Manager\0".to_vec(),
        ))?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let received = match self.recv_net_until(deadline) {
                Ok(received) => received,
                Err(BackupError::Timeout(_)) => break,
                Err(error) => return Err(error),
            };
            match received.message.function {
                REGISTRY_FOUND_OBJECT if received.message.payload.len() >= 2 => {
                    let object = if received.message.payload.len() >= 4 {
                        (be_u32(&received.message.payload[..4]) & 0xFFFF) as u16
                    } else {
                        be_u16(&received.message.payload[..2])
                    };
                    if object == 0 {
                        return Err(BackupError::Protocol(
                            "Avantis returned an invalid Show File Manager handle.".into(),
                        ));
                    }
                    trace(format!("Show File Manager object 0x{object:04X}"));
                    return Ok(object);
                }
                REGISTRY_OBJECT_NOT_FOUND => {
                    return Err(BackupError::Protocol(
                        "The Avantis did not expose its Show File Manager.".into(),
                    ));
                }
                _ => {}
            }
        }
        Err(BackupError::Timeout(
            "Timed out while finding the Avantis Show File Manager.".into(),
        ))
    }

    fn list_stored_show_keys(&mut self, show_manager: u16) -> Result<Vec<ShowKey>> {
        self.send_net_tcp(&NetMessage::new(
            show_manager,
            LOCAL_OBJECT,
            SHOW_MANAGER_SYNC,
            vec![],
        ))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut shows: BTreeMap<String, ShowKey> = BTreeMap::new();
        let mut catalogue_settle_deadline: Option<Instant> = None;
        let mut sync_retried = false;

        while Instant::now() < deadline {
            let receive_deadline = catalogue_settle_deadline
                .map(|settle| settle.min(deadline))
                .unwrap_or(deadline);
            let received = match self.recv_net_until(receive_deadline) {
                Ok(received) => received,
                Err(BackupError::Timeout(_)) => break,
                Err(error) => return Err(error),
            };
            if received.message.function == SHOW_SYNC_READY && !sync_retried {
                // Some firmware advertises readiness before it emits the catalogue.
                self.send_net_tcp(&NetMessage::new(
                    show_manager,
                    LOCAL_OBJECT,
                    SHOW_MANAGER_SYNC,
                    vec![],
                ))?;
                sync_retried = true;
                continue;
            }
            if received.message.function != SHOW_ADDED {
                continue;
            }
            if received.message.source != show_manager {
                continue;
            }
            let key = ShowKey::parse(&received.message.payload)?;
            trace(format!(
                "show location={} name={}",
                key.location(),
                key.name()
            ));

            // Factory and USB locations are not console-stored User Shows. Director V2.01 maps
            // User Show storage to location 4; another non-removable location is retained as a
            // compatibility fallback, but an explicit location-4 copy always wins.
            if !matches!(key.location(), 0 | 1) {
                let normalised = key.name().to_ascii_lowercase();
                let replace = shows
                    .get(&normalised)
                    .is_none_or(|current| current.location() != 4 || key.location() == 4);
                if replace {
                    shows.insert(normalised, key);
                }
                catalogue_settle_deadline = Some(Instant::now() + Duration::from_millis(300));
            }
        }

        let mut shows: Vec<ShowKey> = shows.into_values().collect();
        shows.sort_by(|left, right| {
            left.name()
                .to_ascii_lowercase()
                .cmp(&right.name().to_ascii_lowercase())
        });
        Ok(shows)
    }

    fn download_show(
        &mut self,
        show_manager: u16,
        key: &ShowKey,
        destination: &Path,
    ) -> Result<BackupOutcome> {
        fs::create_dir_all(destination)?;
        let usb_directory = usb_show_directory(destination);
        fs::create_dir_all(&usb_directory)?;

        self.send_net_tcp(&NetMessage::new(
            show_manager,
            LOCAL_OBJECT,
            SHOW_UPLOAD_TO_CONNECTION,
            key.download_payload(),
        ))?;

        let mut temp_path: Option<PathBuf> = None;
        let mut temp_file: Option<File> = None;
        let mut source_name = String::new();
        let mut expected_packets = 0u16;
        let mut packets = 0u16;
        let mut expected_bytes = 0u64;
        let mut bytes_written = 0u64;
        let mut file_sender: Option<u16> = None;
        let deadline = Instant::now() + Duration::from_secs(120);

        let transfer_result = (|| -> Result<()> {
            while Instant::now() < deadline {
                let received = self.recv_net_until(deadline)?;
                if !matches!(received.message.function, FILE_HEADER | FILE_BODY) {
                    continue;
                }
                if file_sender.is_some_and(|source| source != received.message.source) {
                    continue;
                }

                let packet_result = if received.message.function == FILE_HEADER {
                    if temp_file.is_some() {
                        Err(BackupError::Protocol(
                            "Avantis sent more than one Show file header.".into(),
                        ))
                    } else {
                        let header = FileHeader::parse(&received.message.payload)?;
                        if header.file_size > MAX_SHOW_BYTES {
                            Err(BackupError::Protocol(
                                "The Show archive reported an unexpected size.".into(),
                            ))
                        } else if header.packet_count == 0 {
                            Err(BackupError::Protocol(
                                "The Show archive reported zero transfer packets.".into(),
                            ))
                        } else {
                            source_name = header.file_name;
                            file_sender = Some(received.message.source);
                            expected_packets = header.packet_count;
                            expected_bytes = header.file_size;
                            if header.initial_data.len() as u64 > expected_bytes {
                                return Err(BackupError::Protocol(
                                    "The first Show packet exceeded the archive size declared."
                                        .into(),
                                ));
                            }
                            let (path, mut file) = create_partial_file(&usb_directory)?;
                            file.write_all(header.initial_data)?;
                            bytes_written += header.initial_data.len() as u64;
                            temp_path = Some(path);
                            temp_file = Some(file);
                            packets = 1;
                            Ok(())
                        }
                    }
                } else {
                    let Some(file) = temp_file.as_mut() else {
                        self.send_transfer_reply(&received, FILE_ERROR)?;
                        return Err(BackupError::Protocol(
                            "Avantis sent Show data before its file header.".into(),
                        ));
                    };
                    let remaining = expected_bytes.saturating_sub(bytes_written);
                    if received.message.payload.len() as u64 > remaining {
                        Err(BackupError::Protocol(
                            "Avantis sent more Show data than the archive size declared.".into(),
                        ))
                    } else {
                        file.write_all(&received.message.payload)?;
                        bytes_written += received.message.payload.len() as u64;
                        packets = packets.saturating_add(1);
                        Ok(())
                    }
                };

                match packet_result {
                    Ok(()) => self.send_transfer_reply(&received, FILE_ACK)?,
                    Err(error) => {
                        let _ = self.send_transfer_reply(&received, FILE_ERROR);
                        return Err(error);
                    }
                }

                if temp_file.is_some() && packets >= expected_packets {
                    break;
                }
            }

            if temp_file.is_none() {
                return Err(BackupError::Timeout(
                    "Timed out waiting for the Avantis to send the Show archive.".into(),
                ));
            }
            if packets != expected_packets || bytes_written != expected_bytes {
                return Err(BackupError::Protocol(format!(
                    "Show transfer ended early ({bytes_written}/{expected_bytes} bytes, {packets}/{expected_packets} packets)."
                )));
            }
            if let Some(file) = temp_file.as_mut() {
                file.flush()?;
                file.sync_all()?;
            }
            Ok(())
        })();

        if let Err(error) = transfer_result {
            if let Some(path) = temp_path.as_ref() {
                let _ = fs::remove_file(path);
            }
            return Err(error);
        }
        drop(temp_file);
        let temp_path = temp_path.ok_or_else(|| {
            BackupError::Protocol("The Show transfer completed without a temporary file.".into())
        })?;

        // Show archives observed in Director are gzip-compressed tar files. Check the signature,
        // but do not unpack/repack or otherwise alter the bytes received from the console.
        let mut signature = [0u8; 2];
        if let Err(error) =
            File::open(&temp_path).and_then(|mut file| file.read_exact(&mut signature))
        {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }
        if signature != [0x1F, 0x8B] {
            let _ = fs::remove_file(&temp_path);
            return Err(BackupError::Protocol(
                "The downloaded Show did not have the expected gzip archive signature.".into(),
            ));
        }

        let mut final_path = usb_directory.join(dated_archive_name(
            if source_name.trim().is_empty() {
                key.name()
            } else {
                &source_name
            },
            SystemTime::now(),
        ));
        final_path = unique_path(final_path);
        if let Err(error) = fs::rename(&temp_path, &final_path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error.into());
        }

        Ok(BackupOutcome {
            path: final_path,
            bytes: bytes_written,
            show_name: key.name().to_string(),
            source_file_name: source_name,
        })
    }

    fn send_transfer_reply(&mut self, received: &ReceivedNet, function: u16) -> Result<()> {
        let reply = NetMessage::new(received.message.source, LOCAL_OBJECT, function, vec![]);
        match received.transport {
            Transport::Tcp => self.send_net_tcp(&reply),
            Transport::Udp => self.send_net_udp(&reply),
        }
    }

    fn send_util(&mut self, body: &[u8]) -> Result<()> {
        let bytes = encode_util(body)?;
        self.tcp.write_all(&bytes)?;
        Ok(())
    }

    fn send_net_tcp(&mut self, message: &NetMessage) -> Result<()> {
        trace(format!(
            "tx tcp fn=0x{:04X} target=0x{:04X} bytes={}",
            message.function,
            message.target,
            message.payload.len()
        ));
        let bytes = encode_net(self.version, message)?;
        self.tcp.write_all(&bytes)?;
        Ok(())
    }

    fn send_net_udp(&mut self, message: &NetMessage) -> Result<()> {
        trace(format!(
            "tx udp fn=0x{:04X} target=0x{:04X} bytes={}",
            message.function,
            message.target,
            message.payload.len()
        ));
        let bytes = encode_net(self.version, message)?;
        self.udp.send(&bytes)?;
        Ok(())
    }

    fn wait_for_util<F>(&mut self, deadline: Instant, predicate: F) -> Result<Vec<u8>>
    where
        F: Fn(&[u8]) -> bool,
    {
        self.wait_for_util_optional(deadline, predicate)?
            .ok_or_else(|| {
                BackupError::Timeout("Timed out during the AH-Net connection handshake.".into())
            })
    }

    fn wait_for_util_optional<F>(
        &mut self,
        deadline: Instant,
        predicate: F,
    ) -> Result<Option<Vec<u8>>>
    where
        F: Fn(&[u8]) -> bool,
    {
        while Instant::now() < deadline {
            if let Some(frame) = self.next_tcp_frame(deadline)? {
                match frame {
                    WireFrame::Util(body) if predicate(&body) => return Ok(Some(body)),
                    other => self.queued.push_back(other),
                }
            }
        }
        Ok(None)
    }

    fn recv_net_until(&mut self, deadline: Instant) -> Result<ReceivedNet> {
        while Instant::now() < deadline {
            if let Some(index) = self
                .queued
                .iter()
                .position(|frame| matches!(frame, WireFrame::Net(_)))
            {
                if let Some(WireFrame::Net(message)) = self.queued.remove(index) {
                    trace(format!(
                        "rx queued fn=0x{:04X} source=0x{:04X} bytes={}",
                        message.function,
                        message.source,
                        message.payload.len()
                    ));
                    return Ok(ReceivedNet {
                        message,
                        transport: Transport::Tcp,
                    });
                }
            }

            if let Some(frame) = self.next_tcp_frame(
                Instant::now()
                    + Duration::from_millis(80)
                        .min(deadline.saturating_duration_since(Instant::now())),
            )? {
                match frame {
                    WireFrame::Net(message) => {
                        trace(format!(
                            "rx tcp fn=0x{:04X} source=0x{:04X} bytes={}",
                            message.function,
                            message.source,
                            message.payload.len()
                        ));
                        return Ok(ReceivedNet {
                            message,
                            transport: Transport::Tcp,
                        });
                    }
                    util => self.queued.push_back(util),
                }
            }

            let mut datagram = [0u8; 65_535];
            match self.udp.recv(&mut datagram) {
                Ok(length) => {
                    if let Some(WireFrame::Net(message)) = decode_datagram(&datagram[..length])? {
                        trace(format!(
                            "rx udp fn=0x{:04X} source=0x{:04X} bytes={}",
                            message.function,
                            message.source,
                            message.payload.len()
                        ));
                        return Ok(ReceivedNet {
                            message,
                            transport: Transport::Udp,
                        });
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotConnected => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(BackupError::Timeout(
            "Timed out waiting for the Avantis.".into(),
        ))
    }

    fn next_tcp_frame(&mut self, deadline: Instant) -> Result<Option<WireFrame>> {
        if let Some(frame) = self.decoder.next()? {
            return Ok(Some(frame));
        }
        while Instant::now() < deadline {
            let mut buffer = [0u8; 16_384];
            match self.tcp.read(&mut buffer) {
                Ok(0) => {
                    return Err(BackupError::Protocol(
                        "The Avantis closed the network connection.".into(),
                    ))
                }
                Ok(length) => {
                    self.decoder.push(&buffer[..length]);
                    if let Some(frame) = self.decoder.next()? {
                        return Ok(Some(frame));
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(None)
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(None)
    }
}

struct FileHeader<'a> {
    packet_count: u16,
    file_size: u64,
    file_name: String,
    initial_data: &'a [u8],
}

impl<'a> FileHeader<'a> {
    fn parse(payload: &'a [u8]) -> Result<Self> {
        if payload.len() < 9 {
            return Err(BackupError::Protocol(
                "Avantis sent a truncated Show file header.".into(),
            ));
        }
        let header_len = be_u16(&payload[0..2]) as usize;
        let packet_count = be_u16(&payload[2..4]);
        let file_size = be_u32(&payload[4..8]) as u64;
        if header_len < 9 || header_len > payload.len() {
            return Err(BackupError::Protocol(
                "Avantis sent an invalid Show file header length.".into(),
            ));
        }
        let name_area = &payload[8..header_len];
        let nul = name_area
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(name_area.len());
        let file_name: String = name_area[..nul].iter().copied().map(char::from).collect();
        if file_name.contains('/') || file_name.contains('\\') {
            return Err(BackupError::Protocol(
                "Avantis sent an unsafe Show archive name.".into(),
            ));
        }
        Ok(Self {
            packet_count,
            file_size,
            file_name,
            initial_data: &payload[header_len..],
        })
    }
}

fn resolve_endpoint(endpoint: &str) -> Result<SocketAddr> {
    let endpoint = endpoint.trim();
    if let Ok(address) = endpoint.parse::<SocketAddr>() {
        return Ok(address);
    }
    if let Ok(ip) = endpoint.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, DEFAULT_AHNET_PORT));
    }
    if let Some(bracketed_ip) = endpoint
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
    {
        if let Ok(ip) = bracketed_ip.parse::<IpAddr>() {
            return Ok(SocketAddr::new(ip, DEFAULT_AHNET_PORT));
        }
    }

    let with_port = if endpoint
        .rsplit_once(':')
        .is_some_and(|(_, port)| port.parse::<u16>().is_ok())
    {
        endpoint.to_string()
    } else {
        format!("{endpoint}:{DEFAULT_AHNET_PORT}")
    };
    with_port
        .to_socket_addrs()
        .map_err(BackupError::Io)?
        .next()
        .ok_or_else(|| {
            BackupError::InvalidInput("The Avantis address could not be resolved.".into())
        })
}

fn create_partial_file(directory: &Path) -> Result<(PathBuf, File)> {
    let base = format!(".stage-backup-{}-{}", std::process::id(), monotonic_tag());
    for index in 1..=9999 {
        let suffix = if index == 1 {
            String::new()
        } else {
            format!("-{index}")
        };
        let path = directory.join(format!("{base}{suffix}.partial"));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(BackupError::Io(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Could not allocate a unique temporary backup file.",
    )))
}

fn unique_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("Show.tar.gz");
    let stem = file_name.strip_suffix(".tar.gz").unwrap_or(file_name);
    for index in 2..=9999 {
        let candidate = parent.join(format!("{stem}_{index}.tar.gz"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path
}

fn monotonic_tag() -> u128 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn trace(message: String) {
    if std::env::var_os("STAGE_BACKUP_TRACE").is_some() {
        eprintln!("[avantis] {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_director_file_header_layout() {
        let name = b"Sunday.tar.gz\0";
        let header_len = 8 + name.len();
        let mut payload = Vec::new();
        payload.extend_from_slice(&(header_len as u16).to_be_bytes());
        payload.extend_from_slice(&2u16.to_be_bytes());
        payload.extend_from_slice(&1234u32.to_be_bytes());
        payload.extend_from_slice(name);
        payload.extend_from_slice(&[0x1f, 0x8b, 1, 2]);
        let parsed = FileHeader::parse(&payload).unwrap();
        assert_eq!(parsed.packet_count, 2);
        assert_eq!(parsed.file_size, 1234);
        assert_eq!(parsed.file_name, "Sunday.tar.gz");
        assert_eq!(parsed.initial_data, &[0x1f, 0x8b, 1, 2]);
    }

    #[test]
    fn resolves_ip_addresses_with_the_default_port() {
        assert_eq!(
            resolve_endpoint("127.0.0.1").unwrap(),
            SocketAddr::new(IpAddr::from([127, 0, 0, 1]), DEFAULT_AHNET_PORT)
        );
        assert_eq!(
            resolve_endpoint("::1").unwrap(),
            SocketAddr::new(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]), DEFAULT_AHNET_PORT)
        );
        assert_eq!(
            resolve_endpoint("[::1]").unwrap(),
            SocketAddr::new(IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]), DEFAULT_AHNET_PORT)
        );
    }
}
