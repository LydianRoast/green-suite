// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Read-only Wrist Reporter USB transport and independently implemented decoder.
//!
//! The format below was established by comparing captured USB transactions with
//! the corresponding user-owned CSV exports. No Evergreen assembly is loaded.
//! Device dates have no timezone; unit labels are preserved, not converted.

use anyhow::{Context, Result, anyhow, ensure};
use chrono::{NaiveDate, Utc};
use serde_json::json;
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::model::Reading;

const BAUD: u32 = 230_400;
const MAX_GROUPS: u32 = 10_000;
const MAX_RECORDS: u32 = 1_000_000;

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub port: String,
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub group_count: u32,
    pub record_count: u32,
}

pub struct DownloadResult {
    pub device: DeviceInfo,
    pub readings: Vec<Reading>,
    pub raw_log: String,
    pub warnings: Vec<String>,
    /// All inventoried records were received and their labelled numeric fields
    /// decoded. This does not imply verification of undocumented checksums.
    pub complete: bool,
}

pub fn list_ports() -> Result<Vec<String>> {
    // Include all enumerated ports: Windows does not always expose USB metadata
    // for a previously installed CP210x serial driver.
    let mut ports: Vec<_> = serialport::available_ports()?
        .into_iter()
        .map(|p| p.port_name)
        .collect();
    ports.sort();
    ports.dedup();
    Ok(ports)
}

pub fn probe(port: &str) -> Result<DeviceInfo> {
    Session::open(port)?.device_info(port)
}

/// Downloads using the device's read cursor only. Never erases records, changes
/// its clock, writes configuration, or enters the firmware-update protocol.
/// On an interrupted cursor operation, partial data and an explicit warning are
/// returned. Retrying a "next" command could silently skip a record, so we do not.
pub fn download(port: &str, progress: impl Fn(String)) -> Result<DownloadResult> {
    let mut session = Session::open(port)?;
    let device = session.device_info(port)?;
    let mut readings = Vec::new();
    let mut warnings = Vec::new();
    let result = download_records(
        &mut session,
        &device,
        &progress,
        &mut readings,
        &mut warnings,
    );
    let complete = match result {
        Ok(complete) => complete,
        Err(error) => {
            warnings.insert(0, format!(
                "INCOMPLETE DOWNLOAD: {error:#}. Partial readings and received bytes were retained. Reconnect and download again; the device's stored records were not changed."
            ));
            false
        }
    };
    if session.unverified_checksums > 0 {
        warnings.push(format!(
            "{} response(s) had an undocumented checksum. Their framing and response codes were checked, and original bytes are retained, but checksum integrity could not be verified.",
            session.unverified_checksums
        ));
    }
    session.raw_log.push_str(&serde_json::to_string(&json!({
        "Event": "DownloadSummary",
        "TimestampUtc": Utc::now().to_rfc3339(),
        "Serial": device.serial,
        "Complete": complete,
        "Measurements": readings.len(),
        "Warnings": warnings,
    }))?);
    session.raw_log.push('\n');
    Ok(DownloadResult {
        device,
        readings,
        raw_log: session.raw_log,
        warnings,
        complete,
    })
}

fn download_records(
    session: &mut Session,
    device: &DeviceInfo,
    progress: &impl Fn(String),
    readings: &mut Vec<Reading>,
    warnings: &mut Vec<String>,
) -> Result<bool> {
    ensure!(
        device.group_count <= MAX_GROUPS,
        "Implausible group count {}",
        device.group_count
    );
    let mut groups = Vec::new();
    for index in 0..device.group_count {
        progress(format!(
            "Reading group {}/{}",
            index + 1,
            device.group_count
        ));
        let query = if index == 0 {
            Query::FirstGroup
        } else {
            Query::NextGroup
        };
        let payload = session.query(
            query,
            &format!("Group inventory {}/{}", index + 1, device.group_count),
        )?;
        let group = decode_group(&payload)?;
        ensure!(
            !groups.iter().any(|g: &Group| g.name == group.name),
            "Duplicate group {} in inventory",
            group.name
        );
        groups.push(group);
    }
    let total = groups.iter().try_fold(0u32, |n, g| {
        n.checked_add(g.records)
            .ok_or_else(|| anyhow!("Record count overflow"))
    })?;
    ensure!(
        total <= MAX_RECORDS,
        "Implausible inventory: {total} records"
    );
    if total != device.record_count {
        warnings.push(format!(
            "The global counter reports {} records; group inventories contain {total}. Download uses the per-group counts.", device.record_count
        ));
    }
    let mut downloaded = 0u32;
    let mut skipped = 0usize;
    let mut unitless = 0usize;
    for group in groups {
        for index in 0..group.records {
            progress(format!(
                "Downloading {}: {}/{} · {} of {total} records",
                group.name,
                index + 1,
                group.records,
                downloaded
            ));
            let query = if index == 0 {
                Query::FirstRecord(group.name_bytes)
            } else {
                Query::NextRecord
            };
            let payload = session.query(
                query,
                &format!(
                    "Record {}/{} from group {}",
                    index + 1,
                    group.records,
                    group.name
                ),
            )?;
            match decode_record(&payload[2..], &device.serial, &group.name, index + 1) {
                Ok(decoded) => {
                    unitless += decoded.unitless;
                    readings.extend(decoded.readings);
                }
                Err(error) => {
                    skipped += 1;
                    if skipped <= 5 {
                        warnings.push(format!(
                            "Group {} record {} retained as raw bytes only: {error:#}",
                            group.name,
                            index + 1
                        ));
                    }
                }
            }
            downloaded += 1;
        }
    }
    if skipped > 5 {
        warnings.push(format!(
            "{skipped} records could not be decoded; all are retained in the raw USB log."
        ));
    }
    if unitless > 0 {
        warnings.push(format!("{unitless} fields without unit labels were preserved in the raw USB log and excluded from mapped measurements."));
    }
    progress(format!(
        "Downloaded {downloaded} records; decoded {} measurements",
        readings.len()
    ));
    Ok(skipped == 0)
}

#[derive(Debug)]
struct Group {
    name: String,
    name_bytes: [u8; 2],
    records: u32,
}

fn decode_group(payload: &[u8]) -> Result<Group> {
    ensure!(
        payload.len() == 10,
        "Unsupported group header length {}",
        payload.len()
    );
    ensure!(
        payload[0] == 0xb0 && matches!(payload[1], 0x32 | 0x33),
        "Unexpected group response"
    );
    let name_bytes = [payload[3], payload[4]];
    ensure!(
        name_bytes.iter().all(u8::is_ascii_alphanumeric) && payload[5] == 0,
        "Unsupported group name encoding"
    );
    let records = uint32(payload, 6)?;
    ensure!(
        records <= MAX_RECORDS,
        "Implausible group record count {records}"
    );
    Ok(Group {
        name: String::from_utf8(name_bytes.to_vec())?,
        name_bytes,
        records,
    })
}

// This closed enum is the command whitelist. There is no arbitrary command API.
#[derive(Clone, Copy)]
enum Query {
    Status,
    Firmware,
    Identity,
    GroupCount,
    RecordCount,
    FirstGroup,
    NextGroup,
    FirstRecord([u8; 2]),
    NextRecord,
}

impl Query {
    fn wire(self) -> (Vec<u8>, u8) {
        match self {
            Self::Status => (vec![0x30, 0x00], 0x01),
            Self::Firmware => (vec![0x30, 0x05], 0x52),
            Self::Identity => (vec![0x30, 0x15, 0x00, 0x01, 0x00, 0x00], 0x15),
            Self::GroupCount => (vec![0x30, 0x30], 0x30),
            Self::RecordCount => (vec![0x30, 0x31], 0x31),
            Self::FirstGroup => (vec![0x30, 0x32], 0x32),
            Self::NextGroup => (vec![0x30, 0x33], 0x33),
            Self::FirstRecord(name) => (vec![0x30, 0x34, name[0], name[1], 0x00], 0x34),
            Self::NextRecord => (vec![0x30, 0x35], 0x35),
        }
    }
}

struct Session {
    port: Box<dyn SerialPort>,
    raw_log: String,
    transactions: usize,
    unverified_checksums: usize,
}

impl Session {
    fn open(name: &str) -> Result<Self> {
        let mut port = serialport::new(name, BAUD)
            .data_bits(DataBits::Eight)
            .parity(Parity::None)
            .stop_bits(StopBits::One)
            .flow_control(FlowControl::None)
            .dtr_on_open(false)
            .timeout(Duration::from_millis(1500))
            .open()
            .with_context(|| format!("Opening {name}; close other applications using this port"))?;
        port.write_data_terminal_ready(false)
            .context("Disabling DTR")?;
        port.write_request_to_send(false).context("Disabling RTS")?;
        port.clear(ClearBuffer::Input)?;
        Ok(Self {
            port,
            raw_log: String::new(),
            transactions: 0,
            unverified_checksums: 0,
        })
    }

    fn query(&mut self, query: Query, label: &str) -> Result<Vec<u8>> {
        let (payload, expected) = query.wire();
        let tx = encode_frame(&payload)?;
        self.port
            .write_all(&tx)
            .with_context(|| format!("Sending {label}"))?;
        self.port.flush()?;
        let mut raw = Vec::new();
        let response = read_frame(&mut self.port, &mut raw);
        self.transactions += 1;
        let mut entry = json!({
            "Transaction": self.transactions,
            "TimestampUtc": Utc::now().to_rfc3339(),
            "Label": label,
            "TxHex": hex(&tx),
            "RxHex": hex(&raw),
        });
        match &response {
            Ok(frame) => {
                entry["PayloadHex"] = json!(hex(&frame.payload));
                entry["ChecksumMode"] = json!(frame.checksum_mode);
                if frame.checksum_mode == "unverified" {
                    self.unverified_checksums += 1;
                }
            }
            Err(error) => entry["Error"] = json!(format!("{error:#}")),
        }
        self.raw_log.push_str(&serde_json::to_string(&entry)?);
        self.raw_log.push('\n');
        let response = response
            .with_context(|| format!("Reading {label}; cursor commands are not retried"))?;
        ensure!(
            response.checksum_mode != "unverified"
                || matches!(query, Query::FirstRecord(_) | Query::NextRecord),
            "{label}: checksum could not be verified"
        );
        ensure!(
            response.payload.len() >= 2,
            "{label}: response shorter than two bytes"
        );
        ensure!(
            response.payload[0] == 0xb0 && response.payload[1] == expected,
            "{label}: unexpected response {:02X} {:02X}, expected B0 {expected:02X}",
            response.payload[0],
            response.payload[1]
        );
        Ok(response.payload)
    }

    fn device_info(&mut self, port: &str) -> Result<DeviceInfo> {
        self.query(Query::Status, "Status")?;
        let firmware = self.query(Query::Firmware, "Firmware revision")?;
        let identity = self.query(Query::Identity, "Model and serial EEPROM")?;
        let groups = self.query(Query::GroupCount, "Number of groups")?;
        let records = self.query(Query::RecordCount, "Total record count")?;
        ensure!(identity.len() >= 4, "Truncated device identity");
        let serial = ascii_terminated(&identity[3..])?;
        validate_serial(&serial)?;
        Ok(DeviceInfo {
            port: port.into(),
            model: format!("Wrist Reporter (model code {})", identity[2]),
            serial,
            // Firmware's response code is also the first 'R' in 'Rev.'.
            firmware: ascii_terminated(&firmware[1..])?,
            group_count: uint32(&groups, 2)?,
            record_count: uint32(&records, 2)?,
        })
    }
}

fn ascii_terminated(bytes: &[u8]) -> Result<String> {
    let end = bytes
        .iter()
        .position(|b| *b == 0 || *b == 0xff)
        .unwrap_or(bytes.len());
    ensure!(
        bytes[..end]
            .iter()
            .all(|b| b.is_ascii_graphic() || *b == b' '),
        "Invalid ASCII identity string"
    );
    Ok(String::from_utf8(bytes[..end].to_vec())?)
}

fn validate_serial(serial: &str) -> Result<()> {
    ensure!(
        !serial.is_empty()
            && serial.len() <= 64
            && serial
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "Unsupported device serial number format"
    );
    Ok(())
}

fn uint32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| anyhow!("Truncated integer"))?
            .try_into()?,
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn escape(output: &mut Vec<u8>, byte: u8) {
    if matches!(byte, 0x7e | 0x7d | 0x11 | 0x13) {
        output.extend([0x7d, byte ^ 0x20]);
    } else {
        output.push(byte);
    }
}

fn encode_frame(payload: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        !payload.is_empty() && payload.len() <= 255,
        "Invalid outgoing payload length"
    );
    let mut output = vec![0x7e, 0];
    escape(&mut output, payload.len() as u8);
    for byte in payload {
        escape(&mut output, *byte);
    }
    escape(
        &mut output,
        0xffu8.wrapping_sub(payload.iter().fold(0u8, |a, b| a.wrapping_add(*b))),
    );
    Ok(output)
}

#[derive(Debug)]
struct Frame {
    payload: Vec<u8>,
    checksum_mode: &'static str,
}

fn read_byte(reader: &mut impl Read, raw: &mut Vec<u8>, deadline: Instant) -> Result<u8> {
    ensure!(Instant::now() <= deadline, "USB frame deadline exceeded");
    let mut byte = [0u8];
    reader
        .read_exact(&mut byte)
        .context("Truncated or timed-out USB frame")?;
    raw.push(byte[0]);
    Ok(byte[0])
}

fn decoded_byte(reader: &mut impl Read, raw: &mut Vec<u8>, deadline: Instant) -> Result<u8> {
    let byte = read_byte(reader, raw, deadline)?;
    if byte == 0x7d {
        let next = read_byte(reader, raw, deadline)? ^ 0x20;
        ensure!(
            matches!(next, 0x7e | 0x7d | 0x11 | 0x13),
            "Invalid USB escape sequence"
        );
        Ok(next)
    } else {
        ensure!(byte != 0x7e, "Unexpected frame delimiter inside USB packet");
        Ok(byte)
    }
}

fn read_frame(reader: &mut impl Read, raw: &mut Vec<u8>) -> Result<Frame> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        ensure!(raw.len() < 1024, "No USB frame delimiter within 1024 bytes");
        if read_byte(reader, raw, deadline)? == 0x7e {
            break;
        }
    }
    ensure!(
        read_byte(reader, raw, deadline)? == 0,
        "Unsupported USB length prefix"
    );
    let length = usize::from(decoded_byte(reader, raw, deadline)?);
    ensure!(length >= 2, "USB response payload shorter than two bytes");
    let mut payload = Vec::with_capacity(length);
    for _ in 0..length {
        payload.push(decoded_byte(reader, raw, deadline)?);
    }
    let checksum = decoded_byte(reader, raw, deadline)?;
    let sum = payload.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    let checksum_mode = if checksum == 0xffu8.wrapping_sub(sum) {
        "ones-complement"
    } else if checksum == sum {
        "payload-sum"
    } else {
        "unverified"
    };
    Ok(Frame {
        payload,
        checksum_mode,
    })
}

struct DecodedRecord {
    readings: Vec<Reading>,
    unitless: usize,
}

fn bcd(byte: u8) -> Result<u32> {
    ensure!(byte & 0x0f <= 9 && byte >> 4 <= 9, "Invalid BCD timestamp");
    Ok(u32::from(byte >> 4) * 10 + u32::from(byte & 0x0f))
}

fn decode_record(data: &[u8], serial: &str, group: &str, index: u32) -> Result<DecodedRecord> {
    ensure!(
        data.len() >= 18 && data[0..2] == [0xda, 0xff],
        "Unsupported record header"
    );
    ensure!(
        usize::from(data[2]) == data.len(),
        "Record length does not match header"
    );
    let date =
        NaiveDate::from_ymd_opt(2000 + bcd(data[12])? as i32, bcd(data[10])?, bcd(data[11])?)
            .ok_or_else(|| anyhow!("Invalid device date"))?;
    let timestamp = date
        .and_hms_opt(bcd(data[13] & 0x3f)?, bcd(data[14])?, bcd(data[15])?)
        .ok_or_else(|| anyhow!("Invalid device time"))?
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string();
    let mut offset = 18usize;
    let mut readings = Vec::new();
    let mut unitless = 0;
    let raw = hex(data);
    for block_index in 0..usize::from(data[17]) {
        let header = data
            .get(offset..offset + 10)
            .ok_or_else(|| anyhow!("Truncated sensor block"))?;
        ensure!(
            header[0..2] == [0xdb, 0xff],
            "Unsupported sensor block marker"
        );
        let length = usize::from(header[2]);
        ensure!(length > 10, "Empty sensor block");
        let block = data
            .get(offset..offset + length)
            .ok_or_else(|| anyhow!("Truncated sensor value"))?;
        let text: String = block[10..]
            .iter()
            .map(|b| if *b == 0xdf { '°' } else { char::from(*b) })
            .collect();
        ensure!(
            text.chars()
                .all(|c| c == '°' || c.is_ascii_graphic() || c == ' '),
            "Unsupported measurement text encoding"
        );
        let text = text.trim();
        // Numbers are the final whitespace-delimited field, including negatives
        // and decimal values. A unitless constituent is not a physical reading.
        let (unit, number) = match text.rsplit_once(' ') {
            Some((unit, number)) => (unit.trim(), number.trim()),
            None => ("", text),
        };
        let value: f64 = number.parse().context("Measurement is not numeric")?;
        ensure!(value.is_finite(), "Non-finite measurement");
        if unit.is_empty() {
            unitless += 1;
        } else {
            readings.push(Reading {
                id: format!(
                    "wr:{serial}:{group}:{index}:{}:{}:{block_index}",
                    header[8], header[9]
                ),
                timestamp: timestamp.clone(),
                group: group.into(),
                sensor: format!("S-{} / channel {}", header[8], u16::from(header[9]) + 1),
                kind: measurement_kind(unit).into(),
                value,
                unit: unit.into(),
                source: format!("USB {serial} · record {index}"),
                raw: raw.clone(),
            });
        }
        offset += length;
    }
    ensure!(
        data[offset..].iter().all(|b| *b == 0 || *b == 0xff),
        "Unrecognized trailing record content"
    );
    Ok(DecodedRecord { readings, unitless })
}

fn measurement_kind(unit: &str) -> &'static str {
    match unit {
        "FPM" | "m/s" => "Velocity",
        "CFM" | "CF:c" | "L/s" | "m3/h" => "Airflow",
        "inW" | "PSI" | "Pa" | "kPa" | "inHg" => "Pressure",
        "°F" | "°C" | "F" | "C" => "Temperature",
        "°Fd" | "°Cd" => "Dry-bulb temperature",
        "°Fw" | "°Cw" => "Wet-bulb temperature",
        "%RH" => "Relative humidity",
        "DP" => "Dew point",
        "GPM" | "gpm" => "Water flow",
        _ => "Measurement",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn bytes(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    #[test]
    fn frame_escapes_reserved_payload_length_and_checksum_bytes() {
        let mut payload = vec![0x7e, 0x7d, 0x11, 0x13];
        payload.resize(19, 0x13);
        let encoded = encode_frame(&payload).unwrap();
        assert_eq!(&encoded[..4], &[0x7e, 0, 0x7d, 0x33]);
        let frame = read_frame(&mut Cursor::new(encoded), &mut Vec::new()).unwrap();
        assert_eq!(frame.payload, payload);
        assert_eq!(frame.checksum_mode, "ones-complement");
    }

    #[test]
    fn truncated_and_malformed_frames_are_rejected() {
        for raw in [
            vec![0x7e, 0, 4, 0xb0],
            vec![0x7e, 1, 2, 0xb0, 1],
            vec![0x7e, 0, 2, 0x7d, 0],
            vec![0x7e, 0, 1, 0],
        ] {
            assert!(read_frame(&mut Cursor::new(raw), &mut Vec::new()).is_err());
        }
        assert!(encode_frame(&vec![0; 256]).is_err());
    }

    #[test]
    fn device_identity_cannot_create_path_or_identifier_segments() {
        assert!(validate_serial("TEST-123_4").is_ok());
        for serial in ["", "../outside", "..\\outside", "two words", "other:part"] {
            assert!(validate_serial(serial).is_err());
        }
        assert!(validate_serial(&"A".repeat(65)).is_err());
    }

    #[test]
    fn unknown_checksum_is_explicit_and_never_marked_verified() {
        let frame = read_frame(
            &mut Cursor::new(vec![0x7e, 0, 2, 0xb0, 1, 0]),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(frame.checksum_mode, "unverified");
    }

    #[test]
    fn velocity_and_bcd_clock_decode_without_vendor_code() {
        let data = bytes(
            "DA FF 24 00 FF FF FF FF B0 08 01 02 26 89 15 30 80 01 DB FF 12 FF FF FF FF 01 01 00 46 50 4D 20 2D 35 30 30",
        );
        let decoded = decode_record(&data, "TEST", "AA", 1).unwrap();
        assert_eq!(decoded.readings.len(), 1);
        let reading = &decoded.readings[0];
        assert_eq!(reading.timestamp, "2026-01-02T09:15:30");
        assert_eq!(reading.value, -500.0);
        assert_eq!(reading.unit, "FPM");
        assert_eq!(reading.sensor, "S-1 / channel 1");
    }

    #[test]
    fn unitless_constituent_does_not_become_mapped_measurement() {
        let data = bytes(
            "DA FF 32 00 FF FF FF FF B0 08 05 04 26 92 19 24 80 02 DB FF 0E FF FF FF FF 01 00 00 20 31 32 33 DB FF 12 FF FF FF FF 00 00 00 43 46 3A 63 20 31 32 34",
        );
        let decoded = decode_record(&data, "TEST", "AA", 1).unwrap();
        assert_eq!(decoded.unitless, 1);
        assert_eq!(decoded.readings.len(), 1);
        assert_eq!(decoded.readings[0].value, 124.0);
        assert_eq!(decoded.readings[0].unit, "CF:c");
    }

    #[test]
    fn temperature_channels_preserve_sensor_identity_and_degree_glyph() {
        let data = bytes(
            "DA FF 34 00 FF FF FF FF B0 08 01 02 26 89 15 30 80 02 DB FF 11 FF FF FF FF 01 01 00 DF 46 20 37 32 2E 35 DB FF 11 FF FF FF FF 02 02 00 DF 46 20 37 37 2E 30",
        );
        let decoded = decode_record(&data, "TEST", "AA", 1).unwrap();
        assert_eq!(decoded.readings.len(), 2);
        assert_eq!(decoded.readings[0].value, 72.5);
        assert_eq!(decoded.readings[1].value, 77.0);
        assert_eq!(decoded.readings[0].unit, "°F");
        assert_eq!(decoded.readings[1].sensor, "S-2 / channel 1");
        assert_ne!(decoded.readings[0].id, decoded.readings[1].id);
    }

    #[test]
    fn malformed_record_does_not_yield_partial_measurements() {
        let mut data = bytes(
            "DA FF 24 00 FF FF FF FF B0 08 01 02 26 89 15 30 80 01 DB FF 12 FF FF FF FF 01 01 00 46 50 4D 20 2D 35 30 30",
        );
        data[10] = 0x19;
        assert!(decode_record(&data, "TEST", "AA", 1).is_err());
        data[10] = 0x05;
        data[20] = 0x30;
        assert!(decode_record(&data, "TEST", "AA", 1).is_err());
    }

    /// Optional local regression against a user's exported recovery, never
    /// copied into the source tree. Set GREEN_SUITE_RECOVERY to its directory.
    #[test]
    #[ignore]
    fn compare_private_recovery_with_native_csv() -> Result<()> {
        let directory = std::path::PathBuf::from(std::env::var("GREEN_SUITE_RECOVERY")?);
        let log = std::fs::read_to_string(directory.join("raw_protocol_records.ndjson"))?;
        let mut decoded_count = 0;
        let mut records = 0;
        let mut unverified_checksums = 0;
        let mut sensor_checks = 0;
        let mut csv_cache = std::collections::HashMap::new();
        for line in log.lines() {
            let value: serde_json::Value = serde_json::from_str(line)?;
            let label = value["Label"].as_str().unwrap_or("");
            if !label.starts_with("Record ") {
                continue;
            }
            let parts: Vec<_> = label.split_whitespace().collect();
            let group = parts[4];
            let index: usize = parts[1].split('/').next().unwrap().parse()?;
            let payload = bytes(value["PayloadHex"].as_str().unwrap());
            let frame = read_frame(
                &mut Cursor::new(bytes(value["RxHex"].as_str().unwrap())),
                &mut Vec::new(),
            )?;
            ensure!(
                frame.payload == payload,
                "{label}: decoded frame differs from captured payload"
            );
            if frame.checksum_mode == "unverified" {
                unverified_checksums += 1;
            }
            let decoded = decode_record(&payload[2..], "TEST", group, index as u32)
                .with_context(|| label.to_owned())?;
            if !csv_cache.contains_key(group) {
                let path = std::fs::read_dir(&directory)?
                    .filter_map(|f| f.ok())
                    .find(|f| {
                        f.file_name()
                            .to_string_lossy()
                            .starts_with(&format!("{group}-"))
                            && f.path().extension().is_some_and(|e| e == "csv")
                    })
                    .ok_or_else(|| anyhow!("No CSV for group {group}"))?
                    .path();
                let mut reader = csv::ReaderBuilder::new()
                    .has_headers(false)
                    .flexible(true)
                    .from_path(path)?;
                let mut expected = std::collections::HashMap::new();
                let mut sensor_columns: Vec<String> = Vec::new();
                for row in reader.records() {
                    let row = row?;
                    if sensor_columns.is_empty()
                        && row.get(0).unwrap_or("").is_empty()
                        && row.iter().any(|cell| cell.starts_with("S-"))
                    {
                        sensor_columns = row.iter().map(str::to_owned).collect();
                    }
                    let Ok(number) = row.get(0).unwrap_or("").parse::<usize>() else {
                        continue;
                    };
                    let mut values = Vec::new();
                    for i in (3..row.len()).step_by(2) {
                        if let Ok(value) = row.get(i).unwrap_or("").parse::<f64>() {
                            let sensor = sensor_columns
                                .get(i)
                                .filter(|s| s.starts_with("S-"))
                                .cloned();
                            values.push((value, row.get(i + 1).unwrap_or("").to_string(), sensor));
                        }
                    }
                    let timestamp = chrono::NaiveDateTime::parse_from_str(
                        &format!("{} {}", row.get(1).unwrap_or(""), row.get(2).unwrap_or("")),
                        "%m/%d/%Y %H:%M:%S",
                    )?
                    .format("%Y-%m-%dT%H:%M:%S")
                    .to_string();
                    expected.insert(number, (values, timestamp));
                }
                csv_cache.insert(group.to_string(), expected);
            }
            let (expected, timestamp) = &csv_cache[group][&index];
            ensure!(
                decoded.readings.len() == expected.len(),
                "{label}: measurement count differs, {} vs {}",
                decoded.readings.len(),
                expected.len()
            );
            // Native exports sort columns by sensor, while wire blocks may be
            // emitted in another order. Compare each record as a multiset.
            let mut actual: Vec<_> = decoded.readings.iter().collect();
            actual.sort_by(|a, b| {
                a.unit
                    .replace('°', "")
                    .cmp(&b.unit.replace('°', ""))
                    .then_with(|| a.value.total_cmp(&b.value))
                    .then_with(|| a.sensor.cmp(&b.sensor))
            });
            let mut expected = expected.clone();
            expected.sort_by(|a, b| {
                a.1.cmp(&b.1)
                    .then_with(|| a.0.total_cmp(&b.0))
                    .then_with(|| a.2.cmp(&b.2))
            });
            for (actual, (number, unit, sensor)) in actual.into_iter().zip(&expected) {
                ensure!(
                    actual.timestamp == *timestamp,
                    "{label}: timestamp {} != {timestamp}",
                    actual.timestamp
                );
                ensure!(
                    (actual.value - number).abs() < 1e-7,
                    "{label}: {} != {number}",
                    actual.value
                );
                ensure!(
                    actual.unit.replace('°', "") == *unit,
                    "{label}: unit {} != {unit}",
                    actual.unit
                );
                if let Some(sensor) = sensor {
                    ensure!(
                        actual.sensor.split(" / ").next() == Some(sensor.as_str()),
                        "{label}: sensor {} != {sensor}",
                        actual.sensor
                    );
                    sensor_checks += 1;
                }
            }
            decoded_count += decoded.readings.len();
            records += 1;
        }
        ensure!(records > 0, "Recovery log contains no records");
        println!(
            "Matched {decoded_count} measurements, units, and timestamps in {records} records against native CSV exports; checked {sensor_checks} native sensor labels; {unverified_checksums} packets have undocumented checksums."
        );
        Ok(())
    }
}
