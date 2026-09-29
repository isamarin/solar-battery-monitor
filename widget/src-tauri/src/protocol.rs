//! JBD smart BMS frame protocol (read-only subset: commands 0x03, 0x04, 0x05).
//! Request:  DD A5 <cmd> <len> <data..> <crc hi> <crc lo> 77
//! Response: DD <cmd> <status> <len> <data..> <crc hi> <crc lo> 77
//! crc = 0x10000 - sum(bytes from <cmd>/<status> through <data>)

use serde::Serialize;

pub const CMD_BASIC: u8 = 0x03;
pub const CMD_CELLS: u8 = 0x04;
pub const CMD_HW: u8 = 0x05;

const HEAD: u8 = 0xDD;
const TAIL: u8 = 0x77;

fn crc(bytes: &[u8]) -> u16 {
    0u16.wrapping_sub(bytes.iter().map(|&b| b as u16).fold(0, u16::wrapping_add))
}

pub fn read_request(cmd: u8) -> Vec<u8> {
    let c = crc(&[cmd, 0]).to_be_bytes();
    vec![HEAD, 0xA5, cmd, 0x00, c[0], c[1], TAIL]
}

/// Accumulates BLE notification chunks into one response frame.
#[derive(Default)]
pub struct FrameAssembler {
    buf: Vec<u8>,
}

impl FrameAssembler {
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Feeds a chunk; returns the payload once a valid frame for `want` is complete.
    pub fn push(&mut self, chunk: &[u8], want: u8) -> Option<Result<Vec<u8>, String>> {
        // a chunk that looks like a frame start begins a new frame
        if chunk.len() >= 3 && chunk[0] == HEAD && matches!(chunk[1], 0x03..=0x05) && chunk[2] & 0x7F == 0 {
            self.buf.clear();
        }
        self.buf.extend_from_slice(chunk);
        let b = &self.buf;
        if b.len() < 7 || b[0] != HEAD {
            return None;
        }
        let len = b[3] as usize;
        if b.len() < len + 7 {
            return None;
        }
        let end = len + 6;
        let res = if b[end] != TAIL {
            Some(Err("bad frame tail".into()))
        } else if crc(&b[2..end - 2]) != u16::from_be_bytes([b[end - 2], b[end - 1]]) {
            Some(Err("bad checksum".into()))
        } else if b[1] != want {
            None // stale reply to an earlier request
        } else if b[2] != 0 {
            Some(Err(format!("BMS error status 0x{:02x}", b[2])))
        } else {
            Some(Ok(b[4..4 + len].to_vec()))
        };
        self.buf.clear();
        res
    }
}

pub const PROTECTION_FLAGS: [&str; 13] = [
    "Cell overvoltage",
    "Cell undervoltage",
    "Pack overvoltage",
    "Pack undervoltage",
    "Charge overtemp",
    "Charge undertemp",
    "Discharge overtemp",
    "Discharge undertemp",
    "Charge overcurrent",
    "Discharge overcurrent",
    "Short circuit",
    "AFE error",
    "MOS software lock",
];

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Basic {
    pub voltage: f64,
    pub current: f64,
    pub remaining_ah: f64,
    pub nominal_ah: f64,
    pub cycles: u16,
    pub production_date: String,
    /// bit n set = cell n+1 balancing
    pub balancing: u32,
    pub protection: u16,
    pub protections: Vec<String>,
    pub sw_version: String,
    pub soc: u8,
    pub charge_fet: bool,
    pub discharge_fet: bool,
    pub cell_count: u8,
    pub temps: Vec<f64>,
}

pub fn parse_basic(p: &[u8]) -> Result<Basic, String> {
    if p.len() < 23 {
        return Err(format!("basic info too short ({} bytes)", p.len()));
    }
    let u16_at = |i: usize| u16::from_be_bytes([p[i], p[i + 1]]);
    let ntc = p[22] as usize;
    if p.len() < 23 + 2 * ntc {
        return Err("basic info truncated in temperatures".into());
    }
    let date = u16_at(10);
    let protection = u16_at(16);
    Ok(Basic {
        voltage: u16_at(0) as f64 / 100.0,
        current: u16_at(2) as i16 as f64 / 100.0,
        remaining_ah: u16_at(4) as f64 / 100.0,
        nominal_ah: u16_at(6) as f64 / 100.0,
        cycles: u16_at(8),
        production_date: format!("{}-{:02}-{:02}", 2000 + (date >> 9), (date >> 5) & 0x0F, date & 0x1F),
        balancing: u16_at(12) as u32 | (u16_at(14) as u32) << 16,
        protection,
        protections: PROTECTION_FLAGS
            .iter()
            .enumerate()
            .filter(|(i, _)| protection & (1 << i) != 0)
            .map(|(_, s)| s.to_string())
            .collect(),
        sw_version: format!("{}.{}", p[18] >> 4, p[18] & 0x0F),
        soc: p[19],
        charge_fet: p[20] & 1 != 0,
        discharge_fet: p[20] & 2 != 0,
        cell_count: p[21],
        temps: (0..ntc)
            .map(|k| (u16_at(23 + 2 * k) as f64 - 2731.0) / 10.0)
            .collect(),
    })
}

pub fn parse_cells(p: &[u8]) -> Vec<f64> {
    p.chunks_exact(2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]) as f64 / 1000.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_frames_match_known_bytes() {
        assert_eq!(read_request(0x03), [0xDD, 0xA5, 0x03, 0x00, 0xFF, 0xFD, 0x77]);
        assert_eq!(read_request(0x04), [0xDD, 0xA5, 0x04, 0x00, 0xFF, 0xFC, 0x77]);
    }

    fn response(cmd: u8, data: &[u8]) -> Vec<u8> {
        let mut body = vec![0x00, data.len() as u8];
        body.extend_from_slice(data);
        let c = crc(&body).to_be_bytes();
        let mut f = vec![HEAD, cmd];
        f.extend(body);
        f.extend([c[0], c[1], TAIL]);
        f
    }

    #[test]
    fn assembles_split_cell_frame() {
        let f = response(0x04, &[0x0A, 0x36, 0x09, 0xE6, 0x09, 0xD7, 0x09, 0xCC]);
        let mut a = FrameAssembler::default();
        assert!(a.push(&f[..5], 0x04).is_none());
        let payload = a.push(&f[5..], 0x04).unwrap().unwrap();
        assert_eq!(parse_cells(&payload), [2.614, 2.534, 2.519, 2.508]);
    }

    #[test]
    fn ignores_reply_to_other_command() {
        let mut a = FrameAssembler::default();
        assert!(a.push(&response(0x03, &[0; 23]), 0x04).is_none());
        assert!(a.push(&response(0x04, &[0x0A, 0x36]), 0x04).unwrap().is_ok());
    }

    #[test]
    fn rejects_bad_checksum() {
        let mut f = response(0x04, &[0x0A, 0x36]);
        f[4] ^= 1;
        assert!(FrameAssembler::default().push(&f, 0x04).unwrap().is_err());
    }

    #[test]
    fn parses_basic_info_from_real_board() {
        // 11.95 V, 0 A, 0/100 Ah, pack undervoltage, fw 0x29, 4 cells, 1 NTC at 25.8 C
        let mut p = vec![0u8; 25];
        p[0..2].copy_from_slice(&1195u16.to_be_bytes());
        p[6..8].copy_from_slice(&10000u16.to_be_bytes());
        p[16..18].copy_from_slice(&0x0008u16.to_be_bytes());
        p[18] = 0x29;
        p[20] = 0x01;
        p[21] = 4;
        p[22] = 1;
        p[23..25].copy_from_slice(&(2731u16 + 258).to_be_bytes());
        let b = parse_basic(&p).unwrap();
        assert_eq!(b.voltage, 11.95);
        assert_eq!(b.nominal_ah, 100.0);
        assert_eq!(b.protections, ["Pack undervoltage"]);
        assert_eq!(b.sw_version, "2.9");
        assert!(b.charge_fet && !b.discharge_fet);
        assert_eq!(b.temps, [25.8]);
    }
}
