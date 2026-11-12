// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Offline stored-USB capture inspection. Never opens ports or sends bytes.
//! Reports omit identifiers, labels, raw packets, timestamps and measurement
//! values. Repeated frames are counted, never discarded as duplicate readings.

use super::*;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader, Cursor},
    path::Path,
};

const MAX_CAPTURE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 32 * 1024;

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub format: &'static str,
    pub lines: usize,
    pub transactions: usize,
    pub accepted_frames: usize,
    pub rejected_transactions: usize,
    pub invalid_lines: usize,
    pub unverified_record_checksums: usize,
    pub repeated_exchanges: usize,
    pub decoded_records: usize,
    pub decoded_measurements: usize,
    pub unitless_fields: usize,
    pub capture_reported_complete: Option<bool>,
    pub issues: Vec<Issue>,
    pub issue_count: usize,
}

#[derive(Debug, Serialize)]
pub struct Issue {
    pub line: usize,
    pub code: &'static str,
}

impl Report {
    fn issue(&mut self, code: &'static str) {
        self.issue_count += 1;
        if self.issues.len() < 100 {
            self.issues.push(Issue {
                line: self.lines,
                code,
            });
        }
    }
}

pub fn inspect(path: &Path) -> Result<Report> {
    let file = File::open(path).context("Opening private replay capture")?;
    anyhow::ensure!(
        file.metadata()?.len() <= MAX_CAPTURE_BYTES,
        "Replay capture exceeds 128 MiB"
    );
    inspect_reader(BufReader::new(file.take(MAX_CAPTURE_BYTES + 1)))
}

fn inspect_reader(mut reader: impl BufRead) -> Result<Report> {
    let mut report = Report {
        format: "green-suite-stored-usb-replay-v1",
        ..Default::default()
    };
    let mut consumed = 0usize;
    let mut seen = HashSet::new();
    let mut group = None;
    let mut index = 0u32;
    loop {
        // Bound each allocation as well as the whole stream, including a file
        // that grows after the metadata check or omits newline delimiters.
        let mut line = Vec::new();
        let count = reader
            .by_ref()
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            break;
        }
        consumed += count;
        ensure!(
            consumed as u64 <= MAX_CAPTURE_BYTES,
            "Replay capture exceeds 128 MiB"
        );
        ensure!(
            count <= MAX_LINE_BYTES,
            "Replay capture line exceeds 32 KiB"
        );
        report.lines += 1;
        let entry: serde_json::Value = match serde_json::from_slice(&line) {
            Ok(entry) => entry,
            Err(_) => {
                report.invalid_lines += 1;
                report.issue("invalid-json");
                group = None;
                continue;
            }
        };
        if entry["Event"] == "DownloadSummary" {
            report.capture_reported_complete = entry["Complete"].as_bool();
            continue;
        }
        if entry.get("TxHex").is_none() && entry.get("RxHex").is_none() {
            continue;
        }
        report.transactions += 1;
        let result = inspect_exchange(&entry, &mut report, &mut group, &mut index, &mut seen);
        if let Err(code) = result {
            report.rejected_transactions += 1;
            report.issue(code);
            // A failed cursor operation invalidates subsequent positional
            // interpretation. Only an explicit FirstRecord can restore it.
            group = None;
        }
    }
    Ok(report)
}

fn parse_hex(value: &serde_json::Value) -> std::result::Result<Vec<u8>, &'static str> {
    let text = value.as_str().ok_or("missing-hex")?;
    text.split_whitespace()
        .map(|byte| {
            if byte.len() != 2 {
                return Err("invalid-hex");
            }
            u8::from_str_radix(byte, 16).map_err(|_| "invalid-hex")
        })
        .collect()
}

fn packet(bytes: &[u8]) -> std::result::Result<Frame, &'static str> {
    let mut cursor = Cursor::new(bytes);
    let frame =
        read_frame(&mut cursor, &mut Vec::new()).map_err(|_| "malformed-or-truncated-frame")?;
    if cursor.position() != bytes.len() as u64 {
        return Err("trailing-frame-bytes");
    }
    Ok(frame)
}

fn inspect_exchange(
    entry: &serde_json::Value,
    report: &mut Report,
    group: &mut Option<String>,
    index: &mut u32,
    seen: &mut HashSet<(Vec<u8>, Vec<u8>)>,
) -> std::result::Result<(), &'static str> {
    let tx = parse_hex(&entry["TxHex"])?;
    let rx = parse_hex(&entry["RxHex"])?;
    let request = packet(&tx)?;
    if request.checksum_mode == "unverified" {
        return Err("request-checksum-unverified");
    }
    let first = match request.payload.as_slice() {
        [0x30, 0x34, a, b, 0] if a.is_ascii_alphanumeric() && b.is_ascii_alphanumeric() => {
            Some([*a, *b])
        }
        _ => None,
    };
    let expected = if first.is_some() {
        0x34
    } else {
        match request.payload.as_slice() {
            [0x30, 0] => 1,
            [0x30, 5] => 0x52,
            [0x30, 0x15, 0, 1, 0, 0] => 0x15,
            [0x30, code @ (0x30 | 0x31 | 0x32 | 0x33 | 0x35)] => *code,
            _ => return Err("unsupported-request"),
        }
    };
    let frame = packet(&rx)?;
    if frame.payload.get(..2) != Some(&[0xb0, expected]) {
        return Err("mismatched-response");
    }
    let is_record = matches!(expected, 0x34 | 0x35);
    if frame.checksum_mode == "unverified" {
        if !is_record {
            return Err("metadata-checksum-unverified");
        }
        report.unverified_record_checksums += 1;
        report.issue("record-checksum-unverified");
    }
    if let Some(logged) = entry.get("PayloadHex")
        && parse_hex(logged)? != frame.payload
    {
        return Err("logged-payload-mismatch");
    }
    if !seen.insert((tx, rx)) {
        report.repeated_exchanges += 1;
    }
    match expected {
        0x52 => {
            ascii_terminated(&frame.payload[1..]).map_err(|_| "invalid-firmware")?;
        }
        0x15 => {
            let serial = frame.payload.get(3..).ok_or("truncated-identity")?;
            let serial = ascii_terminated(serial).map_err(|_| "invalid-identity")?;
            validate_serial(&serial).map_err(|_| "invalid-identity")?;
        }
        0x30 | 0x31 => {
            uint32(&frame.payload, 2).map_err(|_| "truncated-inventory")?;
        }
        0x32 | 0x33 => {
            decode_group(&frame.payload).map_err(|_| "invalid-group")?;
        }
        0x34 | 0x35 => {
            if let Some(name) = first {
                *group = Some(String::from_utf8_lossy(&name).into_owned());
                *index = 0;
            }
            let name = group.as_deref().ok_or("record-cursor-context-missing")?;
            *index = index.checked_add(1).ok_or("record-index-overflow")?;
            let decoded = decode_record(&frame.payload[2..], "REPLAY", name, *index)
                .map_err(|_| "invalid-record")?;
            report.decoded_records += 1;
            report.decoded_measurements += decoded.readings.len();
            report.unitless_fields += decoded.unitless;
        }
        _ => {}
    }
    report.accepted_frames += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(query: Query, response: &[u8]) -> String {
        json!({"TxHex":hex(&encode_frame(&query.wire().0).unwrap()),
            "RxHex":hex(&encode_frame(response).unwrap())})
        .to_string()
            + "\n"
    }

    fn record() -> Vec<u8> {
        "B0 34 DA FF 24 00 FF FF FF FF B0 08 01 02 26 89 15 30 80 01 DB FF 12 FF FF FF FF 01 01 00 46 50 4D 20 2D 35 30 30"
            .split_whitespace().map(|b| u8::from_str_radix(b,16).unwrap()).collect()
    }

    #[test]
    fn replays_shared_record_decoder_and_counts_repeats_without_discarding() {
        let entry = exchange(Query::FirstRecord(*b"AA"), &record());
        let report = inspect_reader(Cursor::new(entry.repeat(2))).unwrap();
        assert_eq!(report.decoded_records, 2);
        assert_eq!(report.decoded_measurements, 2);
        assert_eq!(report.repeated_exchanges, 1);
        assert_eq!(report.rejected_transactions, 0);
        let output = serde_json::to_string(&report).unwrap();
        assert!(!output.contains("-500"));
        assert!(!output.contains("AA"));
    }

    #[test]
    fn rejects_mismatches_truncation_bad_json_and_unknown_cursor_context() {
        let log = String::from("not-json\n")
            + &exchange(Query::Status, &[0xb0, 0x31])
            + &json!({"TxHex":hex(&encode_frame(&[0x30,0]).unwrap()),"RxHex":"7E 00 02 B0"})
                .to_string()
            + "\n"
            + &exchange(Query::NextRecord, &[0xb0, 0x35]);
        let report = inspect_reader(Cursor::new(log)).unwrap();
        assert_eq!(report.rejected_transactions, 3);
        assert_eq!(report.issue_count, 4);
        assert_eq!(report.accepted_frames, 0);
    }

    #[test]
    fn line_limits_apply_before_unbounded_allocations() {
        assert!(inspect_reader(Cursor::new(vec![b' '; MAX_LINE_BYTES + 1])).is_err());
    }

    #[test]
    fn checksum_corruption_remains_explicit_and_metadata_is_rejected() {
        let entry =
            json!({"TxHex":hex(&encode_frame(&[0x30,0]).unwrap()),"RxHex":"7E 00 02 B0 01 00"})
                .to_string();
        let report = inspect_reader(Cursor::new(entry)).unwrap();
        assert_eq!(report.issues[0].code, "metadata-checksum-unverified");
        assert_eq!(report.rejected_transactions, 1);
    }

    #[test]
    fn invalid_line_counts_survive_the_issue_detail_cap() {
        let report = inspect_reader(Cursor::new("not-json\n".repeat(101))).unwrap();
        assert_eq!(report.invalid_lines, 101);
        assert_eq!(report.issue_count, 101);
        assert_eq!(report.issues.len(), 100);
    }

    #[test]
    fn corrupted_record_checksum_is_visible_without_claiming_verification() {
        let payload = record();
        let sum = payload
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        let bad = (0u8..=255)
            .find(|byte| *byte != sum && *byte != 0xffu8.wrapping_sub(sum))
            .unwrap();
        let mut rx = vec![0x7e, 0];
        escape(&mut rx, payload.len() as u8);
        for byte in payload {
            escape(&mut rx, byte);
        }
        escape(&mut rx, bad);
        let log = json!({"TxHex":hex(&encode_frame(&Query::FirstRecord(*b"AA").wire().0).unwrap()),
            "RxHex":hex(&rx)})
        .to_string();
        let report = inspect_reader(Cursor::new(log)).unwrap();
        assert_eq!(report.decoded_records, 1);
        assert_eq!(report.unverified_record_checksums, 1);
        assert_eq!(report.issues[0].code, "record-checksum-unverified");
    }
}
