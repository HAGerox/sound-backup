use crate::{BackupError, Result};

pub(crate) const SHOW_KEY_BYTES: usize = 42;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShowKey {
    raw: [u8; SHOW_KEY_BYTES],
    name: String,
    location: u8,
}

impl ShowKey {
    pub fn parse(payload: &[u8]) -> Result<Self> {
        if payload.len() < SHOW_KEY_BYTES {
            return Err(BackupError::Protocol(
                "Avantis returned a truncated Show key.".into(),
            ));
        }
        let mut raw = [0u8; SHOW_KEY_BYTES];
        raw.copy_from_slice(&payload[..SHOW_KEY_BYTES]);
        let name = latin1_c_string(&raw[..17]);
        if name.is_empty() {
            return Err(BackupError::Protocol(
                "Avantis returned a Show with no name.".into(),
            ));
        }
        Ok(Self {
            location: raw[17],
            raw,
            name,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn location(&self) -> u8 {
        self.location
    }

    pub(crate) fn download_payload(&self) -> Vec<u8> {
        let mut bytes = self.raw;
        // Director passes false for the legacy flag when asking the console to upload a stored Show.
        bytes[41] = 0;
        bytes.to_vec()
    }

    #[cfg(test)]
    pub(crate) fn fixture(name: &str, location: u8) -> Self {
        let mut raw = [0u8; SHOW_KEY_BYTES];
        let bytes = name.as_bytes();
        let count = bytes.len().min(16);
        raw[..count].copy_from_slice(&bytes[..count]);
        raw[17] = location;
        raw[18..20].copy_from_slice(&0x1234u16.to_be_bytes());
        raw[20..23].copy_from_slice(b"aux");
        raw[41] = 1;
        Self::parse(&raw).unwrap()
    }
}

fn latin1_c_string(bytes: &[u8]) -> String {
    bytes
        .iter()
        .copied()
        .take_while(|byte| *byte != 0)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn show_key_is_exactly_reused_except_legacy_flag() {
        let key = ShowKey::fixture("Sunday", 4);
        assert_eq!(key.name(), "Sunday");
        assert_eq!(key.location(), 4);
        let payload = key.download_payload();
        assert_eq!(payload.len(), 42);
        assert_eq!(&payload[18..20], &0x1234u16.to_be_bytes());
        assert_eq!(&payload[20..23], b"aux");
        assert_eq!(payload[41], 0);
    }
}
