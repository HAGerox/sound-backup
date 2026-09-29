use avantis_protocol::{
    backup_shows, list_stored_shows, test_connection, BackupBatchOutcome, BackupBatchRequest,
    BackupError, StoredShow, DEFAULT_AHNET_PORT,
};
use std::{collections::BTreeSet, path::PathBuf};

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
    let mut possible = BTreeSet::new();
    for address in super::net_scan::open_listeners(DEFAULT_AHNET_PORT) {
        possible.insert(address.to_string());
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
}
