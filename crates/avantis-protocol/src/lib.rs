mod client;
mod filename;
mod show;
mod wire;

pub use client::{backup_show, test_connection, BackupOutcome, BackupRequest};
pub use filename::{dated_archive_name, sanitise_archive_stem, usb_show_directory, AVANTIS_USB_ROOT, AVANTIS_USB_SHOWS};
pub use show::ShowKey;
pub use wire::{AhNetVersion, NetMessage};

use std::fmt;

#[derive(Debug)]
pub enum BackupError {
    InvalidInput(String),
    Io(std::io::Error),
    Protocol(String),
    Timeout(String),
    ShowNotFound { requested: String, available: Vec<String> },
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) | Self::Protocol(message) | Self::Timeout(message) => f.write_str(message),
            Self::Io(error) => write!(f, "{error}"),
            Self::ShowNotFound { requested, available } => {
                if available.is_empty() {
                    write!(f, "Show ‘{requested}’ was not found on the Avantis.")
                } else {
                    write!(
                        f,
                        "Show ‘{requested}’ was not found. Stored shows seen: {}.",
                        available.join(", ")
                    )
                }
            }
        }
    }
}

impl std::error::Error for BackupError {}

impl From<std::io::Error> for BackupError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub(crate) type Result<T> = std::result::Result<T, BackupError>;
