use avantis_protocol::{backup_show, test_connection, BackupError, BackupOutcome, BackupRequest};
use std::path::PathBuf;

pub fn test(console_address: &str) -> Result<(), BackupError> {
    test_connection(console_address)
}

pub fn backup(
    console_address: &str,
    show_name: &str,
    destination: PathBuf,
) -> Result<BackupOutcome, BackupError> {
    backup_show(&BackupRequest {
        endpoint: console_address.to_string(),
        show_name: show_name.to_string(),
        destination,
    })
}
