use serde::de::DeserializeOwned;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

/// Blocking HTTP client for LAN control planes that speak plain JSON over HTTP.
///
/// Mic-Wise and SLink-Rack are both local-network services with no transport
/// security, so this deliberately does plain HTTP with bounded timeouts and no
/// redirect following to unexpected hosts.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout(REQUEST_TIMEOUT)
        .redirects(0)
        .build()
}

/// Long-read agent for multi-hundred-megabyte rack archives.
fn download_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(3))
        .timeout(Duration::from_secs(600))
        .redirects(0)
        .build()
}

fn check(response: ureq::Response) -> Result<ureq::Response, String> {
    let status = response.status();
    if (200..300).contains(&status) {
        return Ok(response);
    }
    let detail = response
        .into_string()
        .unwrap_or_default()
        .chars()
        .take(200)
        .collect::<String>();
    Err(if detail.is_empty() {
        format!("The service returned HTTP {status}.")
    } else {
        format!("The service returned HTTP {status}: {detail}")
    })
}

pub fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, String> {
    let response = check(agent().get(url).call().map_err(|error| error.to_string())?)?;
    serde_json::from_reader(response.into_reader())
        .map_err(|error| format!("The service returned unreadable JSON: {error}"))
}

pub fn post_empty_json<R: DeserializeOwned>(url: &str) -> Result<R, String> {
    let response = check(
        agent()
            .post(url)
            .set("Content-Type", "application/json")
            .send_string("{}")
            .map_err(|error| error.to_string())?,
    )?;
    serde_json::from_reader(response.into_reader())
        .map_err(|error| format!("The service returned unreadable JSON: {error}"))
}

/// Stream `url` into `path`, returning the number of bytes written.
///
/// Used for multi-hundred-megabyte rack archives, which must never be held in
/// memory whole.
pub fn download_to(url: &str, path: &Path) -> Result<u64, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the backup folder: {error}"))?;
    }
    let response = check(
        download_agent()
            .get(url)
            .call()
            .map_err(|error| error.to_string())?,
    )?;
    let mut reader = response.into_reader();
    let mut file = File::create(path).map_err(|error| format!("Could not write the backup: {error}"))?;
    let mut written = 0u64;
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("Could not read the backup stream: {error}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .map_err(|error| format!("Could not write the backup: {error}"))?;
        written += read as u64;
    }
    file.flush()
        .map_err(|error| format!("Could not finish the backup: {error}"))?;
    Ok(written)
}
