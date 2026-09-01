use avantis_protocol::{
    backup_show, backup_shows, list_stored_shows, usb_show_directory, BackupBatchRequest,
    BackupRequest,
};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const LOCAL_CLIENT_OBJECT: u16 = 0x7FFE;
const SERVER_SHOW_MANAGER: u16 = 0x2201;
const SERVER_FILE_SENDER: u16 = 0x2202;
static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
struct Msg {
    connection: u16,
    target: u16,
    source: u16,
    function: u16,
    payload: Vec<u8>,
}

#[test]
fn backs_up_selected_user_show_end_to_end() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || mock_console(listener, 4));

    let base = temp_dir();
    fs::create_dir_all(&base).unwrap();
    let outcome = backup_show(&BackupRequest {
        endpoint: format!("127.0.0.1:{port}"),
        show_name: "Sunday".into(),
        destination: base.clone(),
    })
    .unwrap();

    assert_eq!(outcome.show_name, "Sunday");
    assert_eq!(outcome.source_file_name, "Sunday.tar.gz");
    assert!(outcome.path.starts_with(usb_show_directory(&base)));
    let filename = outcome.path.file_name().unwrap().to_string_lossy();
    assert!(filename.starts_with("20"));
    assert!(filename.ends_with("_Sunday.tar.gz"));
    assert_eq!(fs::read(&outcome.path).unwrap(), fixture_archive());

    server.join().unwrap();
    let _ = fs::remove_dir_all(base);
}

#[test]
fn backs_up_compatibility_location_after_catalogue_settles() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || mock_console(listener, 2));

    let base = temp_dir();
    fs::create_dir_all(&base).unwrap();
    let outcome = backup_show(&BackupRequest {
        endpoint: format!("127.0.0.1:{port}"),
        show_name: "Sunday".into(),
        destination: base.clone(),
    })
    .unwrap();

    assert_eq!(outcome.show_name, "Sunday");
    assert_eq!(fs::read(&outcome.path).unwrap(), fixture_archive());

    server.join().unwrap();
    let _ = fs::remove_dir_all(base);
}

#[test]
fn lists_only_stored_user_shows_and_prefers_the_native_location() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || mock_console_catalogue(listener));

    let shows = list_stored_shows(&format!("127.0.0.1:{port}")).unwrap();

    assert_eq!(
        shows.into_iter().map(|show| show.name).collect::<Vec<_>>(),
        vec!["Festival", "Sunday"]
    );
    server.join().unwrap();
}

#[test]
fn backs_up_multiple_selected_shows_in_one_session() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || mock_console_batch(listener));

    let base = temp_dir();
    fs::create_dir_all(&base).unwrap();
    let outcome = backup_shows(&BackupBatchRequest {
        endpoint: format!("127.0.0.1:{port}"),
        show_names: vec!["Sunday".into(), "festival".into(), "SUNDAY".into()],
        destination: base.clone(),
    })
    .unwrap();

    assert_eq!(outcome.files.len(), 2);
    assert_eq!(outcome.files[0].show_name, "Sunday");
    assert_eq!(outcome.files[1].show_name, "Festival");
    for file in &outcome.files {
        assert_eq!(fs::read(&file.path).unwrap(), fixture_archive());
    }

    server.join().unwrap();
    let _ = fs::remove_dir_all(base);
}

fn connect_console(listener: TcpListener) -> TcpStream {
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let udp_port = udp.local_addr().unwrap().port();
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();

    let hello = read_frame(&mut stream);
    assert_eq!(hello[0], 0xE0);
    write_util(
        &mut stream,
        &[0x02, 0x03, (udp_port >> 8) as u8, udp_port as u8],
    );
    let v2 = read_frame(&mut stream);
    assert_eq!(&v2[3..5], &[0x04, 0x03]);
    write_util(&mut stream, &[0x05, 0x03]);

    let find = decode_net(&read_frame(&mut stream));
    assert_eq!(find.function, 4);
    assert_eq!(find.target, 0);
    assert_eq!(find.source, LOCAL_CLIENT_OBJECT);
    assert_eq!(find.payload, b"Show File Manager\0");
    write_net(
        &mut stream,
        Msg {
            connection: 1,
            target: LOCAL_CLIENT_OBJECT,
            source: 0,
            function: 2,
            payload: (SERVER_SHOW_MANAGER as u32).to_be_bytes().to_vec(),
        },
    );
    stream
}

fn sync_catalogue(stream: &mut TcpStream, messages: impl IntoIterator<Item = Msg>) {
    let sync = decode_net(&read_frame(stream));
    assert_eq!(sync.function, 0x100);
    assert_eq!(sync.target, SERVER_SHOW_MANAGER);
    for message in messages {
        write_net(stream, message);
    }
}

fn mock_console_catalogue(listener: TcpListener) {
    let mut stream = connect_console(listener);
    sync_catalogue(
        &mut stream,
        [
            show_added("Factory", 0),
            show_added("On USB", 1),
            show_added("Sunday", 2),
            show_added("Festival", 4),
            show_added("Sunday", 4),
        ],
    );
    thread::sleep(Duration::from_millis(400));
}

fn mock_console_batch(listener: TcpListener) {
    let mut stream = connect_console(listener);
    sync_catalogue(
        &mut stream,
        [
            show_added("Factory", 0),
            show_added("Sunday", 4),
            show_added("Festival", 4),
        ],
    );

    for expected_name in ["Sunday", "Festival"] {
        let request = decode_net(&read_frame(&mut stream));
        assert_eq!(request.function, 0x118);
        assert_eq!(request.target, SERVER_SHOW_MANAGER);
        assert_eq!(cstring(&request.payload[..16]), expected_name);
        send_archive(&mut stream, expected_name);
    }
}

fn send_archive(stream: &mut TcpStream, show_name: &str) {
    let archive = fixture_archive();
    let name = format!("{show_name}.tar.gz\0");
    let header_len = 8 + name.len();
    let split = 12usize;
    let mut header = Vec::new();
    header.extend_from_slice(&(header_len as u16).to_be_bytes());
    header.extend_from_slice(&2u16.to_be_bytes());
    header.extend_from_slice(&(archive.len() as u32).to_be_bytes());
    header.extend_from_slice(name.as_bytes());
    header.extend_from_slice(&archive[..split]);
    write_net(stream, file_msg(0x113, header));
    let ack1 = decode_net(&read_frame(stream));
    assert_eq!(ack1.function, 2);
    assert_eq!(ack1.target, SERVER_FILE_SENDER);

    write_net(stream, file_msg(0x112, archive[split..].to_vec()));
    let ack2 = decode_net(&read_frame(stream));
    assert_eq!(ack2.function, 2);
    assert_eq!(ack2.target, SERVER_FILE_SENDER);
}

fn cstring(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn mock_console(listener: TcpListener, show_location: u8) {
    let mut stream = connect_console(listener);
    sync_catalogue(
        &mut stream,
        [
            show_added("Factory", 0),
            show_added("Sunday", show_location),
        ],
    );

    let request = decode_net(&read_frame(&mut stream));
    assert_eq!(request.function, 0x118);
    assert_eq!(request.target, SERVER_SHOW_MANAGER);
    assert_eq!(request.payload[17], show_location);
    assert_eq!(request.payload[41], 0);

    send_archive(&mut stream, "Sunday");
}

fn show_added(name: &str, location: u8) -> Msg {
    let mut key = [0u8; 42];
    let bytes = name.as_bytes();
    key[..bytes.len().min(16)].copy_from_slice(&bytes[..bytes.len().min(16)]);
    key[17] = location;
    key[18..20].copy_from_slice(&0x0102u16.to_be_bytes());
    key[20..24].copy_from_slice(b"test");
    Msg {
        connection: 1,
        target: LOCAL_CLIENT_OBJECT,
        source: SERVER_SHOW_MANAGER,
        function: 0x1001,
        payload: key.to_vec(),
    }
}

fn file_msg(function: u16, payload: Vec<u8>) -> Msg {
    Msg {
        connection: 1,
        target: LOCAL_CLIENT_OBJECT,
        source: SERVER_FILE_SENDER,
        function,
        payload,
    }
}

fn fixture_archive() -> Vec<u8> {
    // A tiny deterministic byte string with a valid gzip signature. The production code preserves
    // received bytes exactly rather than decompressing/recompressing them.
    let mut bytes = vec![0x1F, 0x8B, 0x08, 0x00];
    bytes.extend((0u8..96).collect::<Vec<_>>());
    bytes
}

fn write_util(stream: &mut TcpStream, body: &[u8]) {
    let mut bytes = vec![0xE0];
    bytes.extend_from_slice(&(body.len() as u16).to_be_bytes());
    bytes.extend_from_slice(body);
    bytes.push(0xE7);
    stream.write_all(&bytes).unwrap();
}

fn write_net(stream: &mut TcpStream, message: Msg) {
    let mut bytes = vec![0xF1];
    for value in [
        message.connection,
        message.target,
        message.source,
        message.function,
    ] {
        bytes.extend_from_slice(&value.to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
    }
    bytes.extend_from_slice(&(message.payload.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&message.payload);
    bytes.push(0xF8);
    stream.write_all(&bytes).unwrap();
}

fn decode_net(bytes: &[u8]) -> Msg {
    assert_eq!(bytes[0], 0xF1);
    Msg {
        connection: u16::from_be_bytes([bytes[1], bytes[2]]),
        target: u16::from_be_bytes([bytes[5], bytes[6]]),
        source: u16::from_be_bytes([bytes[9], bytes[10]]),
        function: u16::from_be_bytes([bytes[13], bytes[14]]),
        payload: bytes[19..bytes.len() - 1].to_vec(),
    }
}

fn read_frame(stream: &mut TcpStream) -> Vec<u8> {
    let mut first = [0u8; 1];
    stream.read_exact(&mut first).unwrap();
    match first[0] {
        0xE0 => {
            let mut len = [0u8; 2];
            stream.read_exact(&mut len).unwrap();
            let length = u16::from_be_bytes(len) as usize;
            let mut rest = vec![0u8; length + 1];
            stream.read_exact(&mut rest).unwrap();
            let mut out = vec![0xE0, len[0], len[1]];
            out.extend(rest);
            out
        }
        0xF1 => {
            let mut fixed = [0u8; 18];
            stream.read_exact(&mut fixed).unwrap();
            let length = u16::from_be_bytes([fixed[16], fixed[17]]) as usize;
            let mut rest = vec![0u8; length + 1];
            stream.read_exact(&mut rest).unwrap();
            let mut out = vec![0xF1];
            out.extend(fixed);
            out.extend(rest);
            out
        }
        other => panic!("unexpected frame 0x{other:02X}"),
    }
}

fn temp_dir() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "stage-backup-test-{}-{unique}-{sequence}",
        std::process::id()
    ))
}
