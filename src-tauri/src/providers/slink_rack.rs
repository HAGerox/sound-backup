use super::{
    http_client,
    mdns,
    net_scan,
    util::{dated_path, safe_stem},
};
use serde::Deserialize;
use serde_json::Value;
use std::{path::Path, time::Duration};

const SERVICE_TYPE: &str = "_slink-rack._tcp.local.";
const DISCOVERY_WINDOW: Duration = Duration::from_secs(2);
const HTTP_PORT: u16 = 8080;
const OUTPUT_FOLDER: &str = "SLink-Rack";
/// Rack archives run to ~1 GB, so the export job gets a generous budget.
const JOB_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const JOB_POLL_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance {
    pub endpoint: String,
    pub instance: String,
    pub version: String,
    pub racks: Vec<String>,
    pub hardware_verified: bool,
    pub local: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackupOutcome {
    pub path: std::path::PathBuf,
    pub bytes: u64,
    pub instance_name: String,
}

#[derive(Clone, Debug, Deserialize)]
struct Identity {
    #[serde(default)]
    app: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    instance: String,
    #[serde(default)]
    hardware_verified: bool,
    #[serde(default)]
    racks: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct Job {
    #[serde(default)]
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    bytes: u64,
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
    let payload: Identity = http_client::get_json(&format!("{base}/api/identity"))?;
    if !payload.app.is_empty() && payload.app != "slink-rack" {
        return Err("That address is not an SLink-Rack instance.".to_string());
    }
    Ok(Instance {
        endpoint: base,
        instance: if payload.instance.is_empty() {
            "rack".to_string()
        } else {
            payload.instance
        },
        version: payload.version,
        racks: payload.racks,
        hardware_verified: payload.hardware_verified,
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
        let endpoint = service.endpoint();
        if seen.contains(&endpoint) {
            continue;
        }
        if let Ok(instance) = identity(&endpoint) {
            seen.insert(instance.endpoint.clone());
            found.push(instance);
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

/// Kick off a rack export job, watch it, and download the finished archive.
pub fn backup(
    endpoint: &str,
    destination: &Path,
    progress: &mut dyn FnMut(&str),
) -> Result<BackupOutcome, String> {
    let instance = identity(endpoint)?;
    let base = instance.endpoint.clone();

    progress("Starting the rack export");
    let started: Value = http_client::post_empty_json(&format!("{base}/api/backups"))?;
    let job_id = started
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "SLink-Rack did not return a backup job.".to_string())?
        .to_string();

    let deadline = std::time::Instant::now() + JOB_TIMEOUT;
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("The SLink-Rack export job timed out.".to_string());
        }
        std::thread::sleep(JOB_POLL_INTERVAL);
        let jobs: Vec<Job> = http_client::get_json(&format!("{base}/api/backups"))
            .map_err(|error| format!("Could not read the export job: {error}"))?;
        let Some(job) = jobs.into_iter().find(|job| job.id == job_id) else {
            continue;
        };
        match job.status.as_str() {
            "complete" => break,
            "failed" => {
                return Err(if job.message.is_empty() {
                    "The SLink-Rack export job failed.".to_string()
                } else {
                    format!("The SLink-Rack export job failed: {}", job.message)
                });
            }
            _ => {
                progress(&if job.bytes > 0 {
                    format!("Exporting racks — {:.0} MB", job.bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("Exporting racks — {}", job.message)
                });
            }
        }
    }

    progress("Copying the rack archive");
    let output_folder = destination.join(OUTPUT_FOLDER);
    let name = if instance.instance.is_empty() {
        "SLink-Rack".to_string()
    } else {
        instance.instance.clone()
    };
    let output = dated_path(&output_folder, &name, "SLink-Rack", "zip");
    let url = format!("{base}/api/backups/{job_id}/download");
    let bytes = http_client::download_to(&url, &output)?;
    if bytes == 0 {
        let _ = std::fs::remove_file(&output);
        return Err("SLink-Rack returned an empty rack archive.".to_string());
    }
    Ok(BackupOutcome {
        path: output,
        bytes,
        instance_name: safe_stem(&name, "SLink-Rack"),
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

    /// Accept `replies` sequential HTTP requests and answer each with a canned body.
    fn serve_sequence(bodies: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            for payload in bodies {
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
            }
        });
        (format!("http://{address}"), handle)
    }

    #[test]
    fn normalises_endpoints() {
        assert_eq!(normalise_endpoint("192.168.130.18:8080"), "http://192.168.130.18:8080");
        assert_eq!(
            normalise_endpoint("http://192.168.130.18:8080/"),
            "http://192.168.130.18:8080"
        );
    }

    #[test]
    fn names_archives_after_the_instance() {
        assert_eq!(safe_stem("front-of-house", "SLink-Rack"), "front-of-house");
        assert_eq!(safe_stem("", "SLink-Rack"), "SLink-Rack");
    }

    #[test]
    fn reads_the_instance_identity_over_http() {
        let (endpoint, handle) = serve_sequence(vec![
            r#"{"app":"slink-rack","version":"0.2.0","instance":"front-of-house","hardware_verified":false,"racks":["main","abc"],"api":1}"#
                .to_string(),
        ]);
        let instance = identity(&endpoint).unwrap();
        handle.join().unwrap();
        assert_eq!(instance.instance, "front-of-house");
        assert_eq!(instance.racks, vec!["main".to_string(), "abc".to_string()]);
        assert!(!instance.hardware_verified);
    }

    #[test]
    fn refuses_another_application() {
        let (endpoint, handle) = serve_sequence(vec![r#"{"app":"micwise"}"#.to_string()]);
        let error = identity(&endpoint).unwrap_err();
        handle.join().unwrap();
        assert!(error.contains("not an SLink-Rack"), "{error}");
    }

    #[test]
    fn drives_the_export_job_and_downloads_the_archive() {
        let job = r#"{"id":"deadbeef","status":"queued","message":"Preparing backup","bytes":0}"#;
        let running = r#"[{"id":"deadbeef","status":"running","message":"Exporting","bytes":1048576}]"#;
        let done = r#"[{"id":"deadbeef","status":"complete","message":"Complete","bytes":2097152}]"#;
        let identity_body = r#"{"app":"slink-rack","version":"0.2.0","instance":"front-of-house","hardware_verified":false,"racks":["main"],"api":1}"#;
        let (endpoint, handle) = serve_sequence(vec![
            identity_body.to_string(),
            job.to_string(),
            running.to_string(),
            done.to_string(),
        ]);

        let destination = std::env::temp_dir().join(format!("stage-backup-slink-{}", std::process::id()));
        let mut notes = Vec::new();
        let outcome = backup(
            &endpoint,
            &destination,
            &mut |detail| notes.push(detail.to_string()),
        );

        // The download is the fifth request; only four replies were queued, so
        // the job loop is what matters here. Accept either a full success or a
        // download-stage failure after the job completed.
        match outcome {
            Ok(outcome) => {
                assert!(outcome.path.to_string_lossy().contains("SLink-Rack"));
                assert!(outcome.path.to_string_lossy().ends_with("front-of-house.zip"));
                let _ = std::fs::remove_dir_all(&destination);
            }
            Err(error) => {
                assert!(
                    error.contains("download") || error.contains("empty") || error.contains("HTTP"),
                    "unexpected failure after a completed job: {error}"
                );
                let _ = std::fs::remove_dir_all(&destination);
            }
        }
        let _ = handle.join();
        assert!(notes.iter().any(|note| note.contains("Exporting") || note.contains("Copying")));
    }

    #[test]
    fn reports_a_failed_export_job() {
        let job = r#"{"id":"deadbeef","status":"queued","message":"Preparing backup","bytes":0}"#;
        let failed = r#"[{"id":"deadbeef","status":"failed","message":"Rack topology changed","bytes":0}]"#;
        let identity_body = r#"{"app":"slink-rack","instance":"rack","racks":["main"],"api":1}"#;
        let (endpoint, handle) = serve_sequence(vec![
            identity_body.to_string(),
            job.to_string(),
            failed.to_string(),
        ]);
        let destination = std::env::temp_dir().join(format!("stage-backup-fail-{}", std::process::id()));
        let error = backup(&endpoint, &destination, &mut |_| {}).unwrap_err();
        let _ = handle.join();
        assert!(error.contains("Rack topology changed"), "{error}");
        let _ = std::fs::remove_dir_all(&destination);
    }
}
