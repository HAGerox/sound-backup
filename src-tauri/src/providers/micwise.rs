use super::{
    http_client,
    mdns,
    net_scan,
    util::{dated_path, safe_stem},
};
use serde::Deserialize;
use std::{path::Path, time::Duration};

const SERVICE_TYPE: &str = "_micwise._tcp.local.";
const DISCOVERY_WINDOW: Duration = Duration::from_secs(2);
const HTTP_PORT: u16 = 8000;
const OUTPUT_FOLDER: &str = "Mic-Wise";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub endpoint: String,
    pub show_name: String,
    pub show_filename: String,
    pub version: String,
    pub local: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupOutcome {
    pub path: std::path::PathBuf,
    pub bytes: u64,
    pub show_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Health {
    #[serde(default)]
    app: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    show_name: String,
    #[serde(default)]
    show_filename: String,
}

fn normalise_endpoint(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    }
}

pub fn identity(endpoint: &str) -> Result<Instance, String> {
    let base = normalise_endpoint(endpoint);
    let health: Health = http_client::get_json(&format!("{base}/api/health"))?;
    if !health.app.is_empty() && health.app != "micwise" {
        return Err("That address is not a Mic-Wise instance.".to_string());
    }
    let show_name = if health.show_name.is_empty() {
        "Mic-Wise".to_string()
    } else {
        health.show_name
    };
    Ok(Instance {
        endpoint: base,
        show_name,
        show_filename: health.show_filename,
        version: health.version,
        local: is_local(&endpoint),
    })
}

pub fn test(endpoint: &str) -> Result<(), String> {
    identity(endpoint).map(|_| ())
}

pub fn discover(known: Option<&str>) -> Vec<Instance> {
    let mut found: Vec<Instance> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for service in mdns::browse(SERVICE_TYPE, DISCOVERY_WINDOW).unwrap_or_default() {
        match identity(&service.endpoint()) {
            Ok(instance) => {
                seen.insert(instance.endpoint.clone());
                found.push(instance);
            }
            Err(_) => continue,
        }
    }

    for address in net_scan::open_listeners(HTTP_PORT) {
        let endpoint = format!("http://{address}");
        if seen.contains(&endpoint) {
            continue;
        }
        if let Ok(instance) = identity(&endpoint) {
            seen.insert(instance.endpoint.clone());
            found.push(instance);
        }
    }

    if let Some(endpoint) = known.map(str::trim).filter(|value| !value.is_empty()) {
        let endpoint = normalise_endpoint(endpoint);
        if !seen.contains(&endpoint) {
            if let Ok(instance) = identity(&endpoint) {
                found.push(instance);
            }
        }
    }

    found
}

/// Pull the show archive and file it under `<backup folder>/Mic-Wise/`.
pub fn backup(
    endpoint: &str,
    destination: &Path,
    progress: &mut dyn FnMut(&str),
) -> Result<BackupOutcome, String> {
    progress("Reading the show archive");
    let instance = identity(endpoint)?;
    let output_folder = destination.join(OUTPUT_FOLDER);
    let name = if instance.show_name.is_empty() {
        "Mic-Wise Show".to_string()
    } else {
        instance.show_name.clone()
    };
    let output = dated_path(&output_folder, &name, "Mic-Wise Show", "micwise.zip");
    let url = format!("{}/api/showfile/export?format=archive", instance.endpoint);
    progress("Copying the show archive");
    let bytes = http_client::download_to(&url, &output)?;
    if bytes == 0 {
        let _ = std::fs::remove_file(&output);
        return Err("Mic-Wise returned an empty show archive.".to_string());
    }
    Ok(BackupOutcome {
        path: output,
        bytes,
        show_name: safe_stem(&name, "Mic-Wise Show"),
    })
}

fn is_local(endpoint: &str) -> bool {
    let host = endpoint
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default();
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// Serve one canned HTTP response so the real client path can be exercised.
    fn serve_once(body: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let payload = body.to_string();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    payload.len(),
                    payload
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn normalises_endpoints_with_and_without_a_scheme() {
        assert_eq!(normalise_endpoint("10.0.0.4:8000"), "http://10.0.0.4:8000");
        assert_eq!(
            normalise_endpoint("http://10.0.0.4:8000/"),
            "http://10.0.0.4:8000"
        );
    }

    #[test]
    fn recognises_local_endpoints() {
        assert!(is_local("http://127.0.0.1:8000"));
        assert!(!is_local("http://192.168.130.18:8000"));
    }

    #[test]
    fn names_archives_after_the_show() {
        let outcome = BackupOutcome {
            path: Path::new("/tmp/x").to_path_buf(),
            bytes: 1,
            show_name: safe_stem("Sunday Matinee", "Mic-Wise Show"),
        };
        assert_eq!(outcome.show_name, "Sunday Matinee");
        assert_eq!(safe_stem("  ", "Mic-Wise Show"), "Mic-Wise Show");
    }

    #[test]
    fn reads_the_show_identity_over_http() {
        let (endpoint, handle) = serve_once(
            r#"{"app":"micwise","status":"ok","version":"1.2.3","show_name":"Sunday","show_filename":"sunday.micwise","audio_engine_running":true}"#,
        );
        let instance = identity(&endpoint).unwrap();
        handle.join().unwrap();
        assert_eq!(instance.show_name, "Sunday");
        assert_eq!(instance.show_filename, "sunday.micwise");
        assert_eq!(instance.version, "1.2.3");
        assert!(instance.local);
    }

    #[test]
    fn refuses_another_application() {
        let (endpoint, handle) = serve_once(r#"{"app":"something-else","status":"ok"}"#);
        let error = identity(&endpoint).unwrap_err();
        handle.join().unwrap();
        assert!(error.contains("not a Mic-Wise"), "{error}");
    }

    #[test]
    fn pulls_the_show_archive_into_the_backup_folder() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            // First request: identity. Second: the show archive.
            let identity_body =
                r#"{"app":"micwise","show_name":"Sunday","show_filename":"s.micwise","version":"1"}"#;
            for reply in [
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    identity_body.len(),
                    identity_body
                ),
                "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: 4\r\nConnection: close\r\n\r\nPK\x03\x04"
                    .to_string(),
            ] {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buffer = [0u8; 2048];
                    let _ = stream.read(&mut buffer);
                    let _ = stream.write_all(reply.as_bytes());
                }
            }
        });

        let destination = std::env::temp_dir().join(format!("stage-backup-micwise-{}", std::process::id()));
        let mut notes = Vec::new();
        let outcome = backup(
            &format!("http://{address}"),
            &destination,
            &mut |detail| notes.push(detail.to_string()),
        )
        .unwrap();
        handle.join().unwrap();

        assert!(outcome.path.to_string_lossy().contains("Mic-Wise"));
        assert!(outcome.path.to_string_lossy().ends_with(".micwise.zip"));
        assert!(outcome.path.to_string_lossy().contains("Sunday"));
        assert!(outcome.bytes > 0);
        assert!(!notes.is_empty());
        let _ = std::fs::remove_dir_all(&destination);
    }
}
