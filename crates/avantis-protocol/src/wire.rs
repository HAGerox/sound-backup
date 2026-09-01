use crate::{BackupError, Result};

pub const DEFAULT_AHNET_PORT: u16 = 51_321;
const UTIL_START: u8 = 0xE0;
const UTIL_END: u8 = 0xE7;
const NET_V1_START: u8 = 0xF0;
const NET_V1_END: u8 = 0xF7;
const NET_V2_START: u8 = 0xF1;
const NET_V2_END: u8 = 0xF8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhNetVersion {
    V1,
    V2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetMessage {
    pub connection: u16,
    pub target: u16,
    pub source: u16,
    pub function: u16,
    pub payload: Vec<u8>,
}

impl NetMessage {
    pub fn new(target: u16, source: u16, function: u16, payload: Vec<u8>) -> Self {
        Self {
            connection: 1,
            target,
            source,
            function,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WireFrame {
    Util(Vec<u8>),
    Net(NetMessage),
}

pub(crate) fn encode_util(body: &[u8]) -> Result<Vec<u8>> {
    let length: u16 = body
        .len()
        .try_into()
        .map_err(|_| BackupError::Protocol("AH-Net utility message is too large.".into()))?;
    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(UTIL_START);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(body);
    out.push(UTIL_END);
    Ok(out)
}

pub(crate) fn encode_net(version: AhNetVersion, message: &NetMessage) -> Result<Vec<u8>> {
    let length: u16 = message
        .payload
        .len()
        .try_into()
        .map_err(|_| BackupError::Protocol("AH-Net message is too large.".into()))?;

    let mut out = Vec::with_capacity(message.payload.len() + 20);
    match version {
        AhNetVersion::V1 => {
            out.push(NET_V1_START);
            for value in [
                message.connection,
                message.target,
                message.source,
                message.function,
            ] {
                out.extend_from_slice(&value.to_be_bytes());
            }
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(&message.payload);
            out.push(NET_V1_END);
        }
        AhNetVersion::V2 => {
            out.push(NET_V2_START);
            for value in [
                message.connection,
                message.target,
                message.source,
                message.function,
            ] {
                out.extend_from_slice(&value.to_be_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            out.extend_from_slice(&length.to_be_bytes());
            out.extend_from_slice(&message.payload);
            out.push(NET_V2_END);
        }
    }
    Ok(out)
}

#[derive(Default)]
pub(crate) struct WireDecoder {
    buffer: Vec<u8>,
}

impl WireDecoder {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    pub(crate) fn next(&mut self) -> Result<Option<WireFrame>> {
        loop {
            let Some(&start) = self.buffer.first() else {
                return Ok(None);
            };

            let parsed = match start {
                UTIL_START => self.try_util()?,
                NET_V1_START => self.try_net_v1()?,
                NET_V2_START => self.try_net_v2()?,
                _ => {
                    self.buffer.remove(0);
                    continue;
                }
            };
            return Ok(parsed);
        }
    }

    fn try_util(&mut self) -> Result<Option<WireFrame>> {
        if self.buffer.len() < 3 {
            return Ok(None);
        }
        let payload_len = u16::from_be_bytes([self.buffer[1], self.buffer[2]]) as usize;
        let total = payload_len + 4;
        if self.buffer.len() < total {
            return Ok(None);
        }
        if self.buffer[total - 1] != UTIL_END {
            self.buffer.remove(0);
            return Err(BackupError::Protocol(
                "Malformed AH-Net utility frame.".into(),
            ));
        }
        let body = self.buffer[3..3 + payload_len].to_vec();
        self.buffer.drain(..total);
        Ok(Some(WireFrame::Util(body)))
    }

    fn try_net_v1(&mut self) -> Result<Option<WireFrame>> {
        if self.buffer.len() < 11 {
            return Ok(None);
        }
        let payload_len = be_u16(&self.buffer[9..11]) as usize;
        let total = payload_len + 12;
        if self.buffer.len() < total {
            return Ok(None);
        }
        if self.buffer[total - 1] != NET_V1_END {
            self.buffer.remove(0);
            return Err(BackupError::Protocol("Malformed AH-Net v1 frame.".into()));
        }
        let message = NetMessage {
            connection: be_u16(&self.buffer[1..3]),
            target: be_u16(&self.buffer[3..5]),
            source: be_u16(&self.buffer[5..7]),
            function: be_u16(&self.buffer[7..9]),
            payload: self.buffer[11..11 + payload_len].to_vec(),
        };
        self.buffer.drain(..total);
        Ok(Some(WireFrame::Net(message)))
    }

    fn try_net_v2(&mut self) -> Result<Option<WireFrame>> {
        if self.buffer.len() < 19 {
            return Ok(None);
        }
        let payload_len = be_u16(&self.buffer[17..19]) as usize;
        let total = payload_len + 20;
        if self.buffer.len() < total {
            return Ok(None);
        }
        if self.buffer[total - 1] != NET_V2_END {
            self.buffer.remove(0);
            return Err(BackupError::Protocol("Malformed AH-Net v2 frame.".into()));
        }
        let message = NetMessage {
            connection: be_u16(&self.buffer[1..3]),
            target: be_u16(&self.buffer[5..7]),
            source: be_u16(&self.buffer[9..11]),
            function: be_u16(&self.buffer[13..15]),
            payload: self.buffer[19..19 + payload_len].to_vec(),
        };
        self.buffer.drain(..total);
        Ok(Some(WireFrame::Net(message)))
    }
}

pub(crate) fn decode_datagram(bytes: &[u8]) -> Result<Option<WireFrame>> {
    let mut decoder = WireDecoder::default();
    decoder.push(bytes);
    decoder.next()
}

pub(crate) fn be_u16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

pub(crate) fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_round_trip() {
        let message = NetMessage {
            connection: 0x1234,
            target: 0x2345,
            source: 0x3456,
            function: 0x118,
            payload: vec![1, 2, 3, 4],
        };
        let bytes = encode_net(AhNetVersion::V1, &message).unwrap();
        let mut decoder = WireDecoder::default();
        decoder.push(&bytes);
        assert_eq!(decoder.next().unwrap(), Some(WireFrame::Net(message)));
    }

    #[test]
    fn v2_round_trip() {
        let message = NetMessage {
            connection: 1,
            target: 0x0203,
            source: 0x7FFE,
            function: 0x100,
            payload: b"Show File Manager\0".to_vec(),
        };
        let bytes = encode_net(AhNetVersion::V2, &message).unwrap();
        let mut decoder = WireDecoder::default();
        for chunk in bytes.chunks(3) {
            decoder.push(chunk);
        }
        assert_eq!(decoder.next().unwrap(), Some(WireFrame::Net(message)));
    }

    #[test]
    fn util_round_trip() {
        let bytes = encode_util(&[1, 3, 0xCA, 0x79]).unwrap();
        let mut decoder = WireDecoder::default();
        decoder.push(&bytes);
        assert_eq!(
            decoder.next().unwrap(),
            Some(WireFrame::Util(vec![1, 3, 0xCA, 0x79]))
        );
    }
}
