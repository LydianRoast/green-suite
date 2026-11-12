// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

//! Bounded USB live reception. Ending reception closes the PC port; it does not
//! assert that the Reporter's network lock or RTU mode has been reversed.

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serialport::SerialPort;
use std::{
    collections::{HashSet, VecDeque},
    fs::{File, OpenOptions},
    io::{BufWriter, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

const MAX_MODULES: u8 = 10;
const MAX_CHANNELS: usize = 32;
const MAX_CAPTURE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FRAME_BYTES: usize = 520;
const IO_TIMEOUT: Duration = Duration::from_millis(200);
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
const START_TIMEOUT: Duration = Duration::from_secs(15);
// The vendor application configures its workbook between entering RTU mode and
// navigating the screen. Allow a cancellable settling interval here; this is
// conservative pacing, not an asserted firmware timing specification.
const RTU_SETTLE: Duration = Duration::from_secs(2);
const FRAME_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModuleInfo {
    pub address: u8,
    pub kind: String,
    pub name: String,
    pub channels: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveValue {
    pub value: f64,
    pub display_value: String,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveSample {
    pub module_address: u8,
    pub values: Vec<LiveValue>,
    /// Instrument-local time, without an inferred timezone.
    pub device_timestamp: Option<String>,
    pub received_at: String,
    /// Exact frame bytes, including framing, escaping and checksum.
    pub raw: String,
}

#[derive(Clone, Debug)]
pub enum LiveEvent {
    Modules(Vec<ModuleInfo>),
    /// Sent before the first mode-changing command, including failed attempts.
    SessionChanged,
    Sample(LiveSample),
    Warning(String),
}

/// Owns one serial port until cancellation or error. No firmware, calibration,
/// record erasure or undocumented exit/reset command is available here.
pub fn monitor(
    port: &str,
    raw_path: &Path,
    stop: Arc<AtomicBool>,
    emit: impl Fn(LiveEvent),
) -> Result<()> {
    if stop.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut capture = Capture::create(raw_path)?;
    capture.record(json!({"event":"SessionOpening", "port":port}))?;
    let result = receive_session(port, &stop, &emit, &mut capture);
    let summary = capture.record(json!({
        "event":"SessionEnded", "cancelled":stop.load(Ordering::Acquire),
        "error":result.as_ref().err().map(|error| format!("{error:#}")),
        "device_mode_restored":false,
    }));
    result.and(summary)
}

fn receive_session(
    port_name: &str,
    stop: &AtomicBool,
    emit: &impl Fn(LiveEvent),
    capture: &mut Capture,
) -> Result<()> {
    let port = open_live_port(port_name)?;
    let mut session = Session {
        port,
        stop,
        emit,
        capture,
        decoder: Decoder::default(),
        pending: VecDeque::new(),
        modules: Vec::new(),
        warnings: HashSet::new(),
        modules_announced: false,
    };
    let result = session.run();
    // A user stop is normal; a simultaneous transport or capture failure must
    // still be reported rather than being hidden by the stop flag.
    match result {
        Err(error) if error.downcast_ref::<Cancelled>().is_some() => Ok(()),
        result => result,
    }
}

fn open_live_port(port_name: &str) -> Result<Box<dyn SerialPort>> {
    crate::connection::open(port_name, IO_TIMEOUT)
}

#[derive(Debug)]
struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Live reception cancelled")
    }
}

impl std::error::Error for Cancelled {}

/// Closed command set: two discovery reads and three identified session changes.
#[derive(Clone, Copy)]
enum Command {
    ModuleCount,
    ModuleInfo(u8),
    LockNetwork,
    RtuMode,
    HistoryScreen,
}

impl Command {
    fn payload(self) -> Vec<u8> {
        match self {
            Self::ModuleCount => vec![0x30, 0x40],
            Self::ModuleInfo(address) => vec![0x30, 0x41, address],
            Self::LockNetwork => vec![0x30, 0x24],
            Self::RtuMode => vec![0x30, 0x50],
            Self::HistoryScreen => vec![0x30, 0x51, 0x02],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ModuleCount => "Read linked module count",
            Self::ModuleInfo(_) => "Read module metadata",
            Self::LockNetwork => "Lock linked module set",
            Self::RtuMode => "Enter RTU mode",
            Self::HistoryScreen => "Show live history screen",
        }
    }
}

struct Session<'a, F: Fn(LiveEvent)> {
    port: Box<dyn SerialPort>,
    stop: &'a AtomicBool,
    emit: &'a F,
    capture: &'a mut Capture,
    decoder: Decoder,
    pending: VecDeque<Outcome>,
    modules: Vec<ModuleInfo>,
    warnings: HashSet<String>,
    modules_announced: bool,
}

impl<F: Fn(LiveEvent)> Session<'_, F> {
    fn run(&mut self) -> Result<()> {
        let count = self.query(Command::ModuleCount)?;
        ensure!(count.len() >= 3, "Truncated linked module count");
        let count = count[2];
        ensure!(
            count > 0,
            "No sensors are linked. Pair a sensor on the Reporter before starting live reception."
        );
        ensure!(
            count <= MAX_MODULES,
            "Unsupported linked module count {count}"
        );
        self.check_stop()?;
        (self.emit)(LiveEvent::SessionChanged);
        self.query(Command::LockNetwork)?;
        let mut terminated = false;
        for address in 1..=count + 1 {
            let payload = self.query(Command::ModuleInfo(address))?;
            ensure!(payload.len() >= 3, "Truncated module enumeration response");
            if payload[2] == 0 {
                terminated = true;
                break;
            }
            ensure!(
                address <= count,
                "Linked module inventory changed during discovery; reconnect before monitoring"
            );
            self.modules.push(decode_module(&payload, address)?);
        }
        ensure!(
            terminated && self.modules.len() == usize::from(count),
            "Module enumeration does not match the reported inventory; reconnect before monitoring"
        );
        (self.emit)(LiveEvent::Modules(self.modules.clone()));
        self.modules_announced = true;
        ensure!(
            self.modules.iter().all(|module| module.kind == "SNE"),
            "Live decoding currently supports SNE pressure modules. Other module families require validation before monitoring."
        );
        self.capture
            .record(json!({"event":"Modules", "modules":self.modules}))?;
        self.query(Command::RtuMode)?;
        self.settle()?;
        self.start_history()?;
        self.capture.record(json!({"event":"Receiving"}))?;
        while !self.stop.load(Ordering::Acquire) {
            if let Some(frame) = self.next_frame()? {
                self.unsolicited(frame)?;
            }
        }
        Ok(())
    }

    fn check_stop(&self) -> Result<()> {
        if self.stop.load(Ordering::Acquire) {
            return Err(Cancelled.into());
        }
        Ok(())
    }

    fn query(&mut self, command: Command) -> Result<Vec<u8>> {
        self.send(command)?;
        let expected = command.payload()[1];
        let deadline = Instant::now() + QUERY_TIMEOUT;
        while Instant::now() < deadline {
            self.check_stop()?;
            if let Some(frame) = self.next_frame()? {
                if frame.payload.starts_with(&[0xb0, expected]) {
                    return Ok(frame.payload);
                }
                self.unsolicited(frame)?;
            }
        }
        bail!(
            "{} timed out; session-changing commands are not retried",
            command.label()
        )
    }

    fn send(&mut self, command: Command) -> Result<()> {
        self.check_stop()?;
        let payload = command.payload();
        let frame = crate::usb::encode_frame(&payload)?;
        self.capture.record(
            json!({"event":"TransmitAttempt", "command":command.label(), "raw":hex(&frame)}),
        )?;
        self.port
            .write_all(&frame)
            .with_context(|| format!("Sending {}", command.label()))?;
        Ok(())
    }

    fn settle(&mut self) -> Result<()> {
        let deadline = Instant::now() + RTU_SETTLE;
        while Instant::now() < deadline {
            self.check_stop()?;
            if let Some(frame) = self.next_frame()? {
                self.unsolicited(frame)?;
            }
        }
        Ok(())
    }

    fn start_history(&mut self) -> Result<()> {
        self.send(Command::HistoryScreen)?;
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            self.check_stop()?;
            if let Some(frame) = self.next_frame()? {
                if frame.payload.starts_with(&[0xb0, 0x51]) {
                    ensure!(
                        frame.payload.len() >= 3 && frame.payload[2] != 0,
                        "Reporter rejected the live history screen"
                    );
                    return Ok(());
                }
                // Some revisions send the first measurement without a separate
                // screen acknowledgement. Only a fully validated sample from
                // the discovered module is evidence of live reception.
                if frame.payload.starts_with(&[0xb0, 0x42]) {
                    match decode_sample(&frame.payload, &frame.raw, &self.modules) {
                        Ok(sample) => {
                            (self.emit)(LiveEvent::Sample(sample));
                            return Ok(());
                        }
                        Err(error) => self.warn(format!(
                            "A live packet was retained without a measurement: {error:#}"
                        )),
                    }
                } else {
                    self.unsolicited(frame)?;
                }
            }
        }
        bail!(
            "No verified live reading or screen acknowledgement arrived within 15 seconds. Stop and follow the Reporter restart instructions before reconnecting."
        )
    }

    fn next_frame(&mut self) -> Result<Option<Frame>> {
        self.check_stop()?;
        if self.pending.is_empty() {
            let mut bytes = [0u8; 1024];
            match self.port.read(&mut bytes) {
                Ok(0) => std::thread::sleep(Duration::from_millis(10)),
                Ok(count) => {
                    self.capture
                        .record(json!({"event":"ReceiveBytes", "raw":hex(&bytes[..count])}))?;
                    for byte in &bytes[..count] {
                        if let Some(outcome) = self.decoder.feed(*byte) {
                            self.pending.push_back(outcome);
                        }
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => {
                    return Err(error)
                        .context("USB live reception failed; the Reporter may have disconnected");
                }
            }
            if let Some(outcome) = self.decoder.expire() {
                self.pending.push_back(outcome);
            }
        }
        while let Some(outcome) = self.pending.pop_front() {
            self.check_stop()?;
            match outcome {
                Outcome::Frame(frame) => {
                    self.capture.record(
                        json!({"event":"Frame", "raw":hex(&frame.raw), "checksum":frame.checksum}),
                    )?;
                    return Ok(Some(frame));
                }
                Outcome::Rejected { raw, reason } => {
                    self.capture.record(
                        json!({"event":"RejectedFrame", "raw":hex(&raw), "reason":reason}),
                    )?;
                    self.warn(format!(
                        "Ignored an invalid USB frame: {reason}. Its bytes were retained."
                    ));
                }
            }
        }
        Ok(None)
    }

    fn unsolicited(&mut self, frame: Frame) -> Result<()> {
        if frame.payload.starts_with(&[0xb0, 0x42]) {
            // Frames arriving during discovery remain in the raw capture. Do
            // not publish readings until the entire inventory is announced.
            if !self.modules_announced {
                return Ok(());
            }
            match decode_sample(&frame.payload, &frame.raw, &self.modules) {
                Ok(sample) => (self.emit)(LiveEvent::Sample(sample)),
                Err(error) => self.warn(format!(
                    "A live packet was retained without a measurement: {error:#}"
                )),
            }
        } else if frame.payload.starts_with(&[0xb0, 0x01]) {
            bail!(
                "The Reporter restarted. Stop and reconnect before starting another live session."
            );
        } else {
            self.warn(format!(
                "An unsupported live packet ({}) was retained without changing readings.",
                hex(&frame.payload[..frame.payload.len().min(2)])
            ));
        }
        Ok(())
    }

    fn warn(&mut self, message: String) {
        // Repeated noise must not create an unbounded GUI event queue.
        if self.warnings.len() < 32 && self.warnings.insert(message.clone()) {
            (self.emit)(LiveEvent::Warning(message));
        }
    }
}

struct Capture {
    file: BufWriter<File>,
    bytes: u64,
}

impl Capture {
    fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).context("Creating live capture directory")?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("Creating raw live capture; existing files are never overwritten")?;
        Ok(Self {
            file: BufWriter::new(file),
            bytes: 0,
        })
    }

    fn record(&mut self, mut event: Value) -> Result<()> {
        event["received_at"] = json!(Utc::now().to_rfc3339());
        let mut line = serde_json::to_vec(&event)?;
        line.push(b'\n');
        ensure!(
            self.bytes + line.len() as u64 <= MAX_CAPTURE_BYTES,
            "Raw live capture reached its 32 MiB limit. Reception stopped; start a new session to create another capture."
        );
        self.file
            .write_all(&line)
            .context("Saving raw USB live capture")?;
        self.file.flush().context("Flushing raw USB live capture")?;
        self.bytes += line.len() as u64;
        Ok(())
    }
}

#[derive(Debug)]
struct Frame {
    payload: Vec<u8>,
    raw: Vec<u8>,
    checksum: &'static str,
}

#[derive(Debug)]
enum Outcome {
    Frame(Frame),
    Rejected { raw: Vec<u8>, reason: &'static str },
}

#[derive(Default)]
struct Decoder {
    raw: Vec<u8>,
    decoded: Vec<u8>,
    escaped: bool,
    started: Option<Instant>,
}

impl Decoder {
    fn reject(&mut self, reason: &'static str) -> Outcome {
        let raw = std::mem::take(&mut self.raw);
        self.decoded.clear();
        self.escaped = false;
        self.started = None;
        Outcome::Rejected { raw, reason }
    }

    fn expire(&mut self) -> Option<Outcome> {
        self.started
            .filter(|started| started.elapsed() >= FRAME_TIMEOUT)
            .map(|_| self.reject("partial frame timed out"))
    }

    fn feed(&mut self, byte: u8) -> Option<Outcome> {
        if byte == 0x7e {
            let rejected =
                (!self.raw.is_empty()).then(|| self.reject("unexpected frame delimiter"));
            self.raw.push(byte);
            self.started = Some(Instant::now());
            return rejected;
        }
        if self.raw.is_empty() {
            return None;
        }
        self.raw.push(byte);
        if self.raw.len() > MAX_FRAME_BYTES {
            return Some(self.reject("frame exceeded its length bound"));
        }
        let decoded = if self.escaped {
            self.escaped = false;
            let value = byte ^ 0x20;
            if !matches!(value, 0x7e | 0x7d | 0x11 | 0x13) {
                return Some(self.reject("invalid escape sequence"));
            }
            value
        } else if byte == 0x7d {
            self.escaped = true;
            return None;
        } else {
            byte
        };
        self.decoded.push(decoded);
        if self.decoded[0] != 0 {
            return Some(self.reject("unsupported length prefix"));
        }
        if self.decoded.len() < 2 {
            return None;
        }
        let length = usize::from(self.decoded[1]);
        if length < 2 {
            return Some(self.reject("payload shorter than two bytes"));
        }
        if self.decoded.len() != length + 3 {
            return None;
        }
        let payload = self.decoded[2..2 + length].to_vec();
        let sum = payload
            .iter()
            .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        let checksum = self.decoded[length + 2];
        let checksum = if checksum == 0xffu8.wrapping_sub(sum) {
            "ones-complement"
        } else if checksum == sum {
            "payload-sum"
        } else {
            return Some(self.reject("checksum could not be verified"));
        };
        let raw = std::mem::take(&mut self.raw);
        self.decoded.clear();
        self.started = None;
        Some(Outcome::Frame(Frame {
            payload,
            raw,
            checksum,
        }))
    }
}

fn ascii_field(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' '),
        "Unsupported metadata text"
    );
    Ok(std::str::from_utf8(bytes)?.trim().to_owned())
}

fn nul_field<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    let remaining = bytes
        .get(*offset..)
        .ok_or_else(|| anyhow!("Truncated module metadata"))?;
    let length = remaining
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| anyhow!("Unterminated module metadata"))?;
    let field = &remaining[..length];
    *offset += length + 1;
    Ok(field)
}

fn decode_module(payload: &[u8], address: u8) -> Result<ModuleInfo> {
    ensure!(
        payload.starts_with(&[0xb0, 0x41]),
        "Unexpected module metadata response"
    );
    let mut offset = 2;
    let kind = ascii_field(nul_field(payload, &mut offset)?)?;
    ensure!(
        !kind.is_empty(),
        "Linked module disappeared during discovery"
    );
    let channels = usize::from(
        *payload
            .get(offset)
            .ok_or_else(|| anyhow!("Missing module channel count"))?,
    );
    ensure!(
        (1..=MAX_CHANNELS).contains(&channels),
        "Unsupported module channel count"
    );
    offset += 1;
    ensure!(
        payload.get(offset) == Some(&0),
        "Unsupported module channel delimiter"
    );
    offset += 1;
    let name = ascii_field(nul_field(payload, &mut offset)?)?;
    ensure!(!name.is_empty(), "Empty module display name");
    Ok(ModuleInfo {
        address,
        kind,
        name,
        channels,
    })
}

fn bcd(value: u8) -> Result<u32> {
    ensure!(
        value & 0x0f <= 9 && value >> 4 <= 9,
        "Invalid live BCD timestamp"
    );
    Ok(u32::from(value >> 4) * 10 + u32::from(value & 0x0f))
}

fn decode_sample(payload: &[u8], raw: &[u8], modules: &[ModuleInfo]) -> Result<LiveSample> {
    ensure!(
        payload.len() >= 7 && payload.starts_with(&[0xb0, 0x42]),
        "Unsupported live payload header"
    );
    let address = payload[4];
    let module = modules
        .iter()
        .find(|module| module.address == address)
        .ok_or_else(|| anyhow!("Live packet came from an undiscovered module"))?;
    ensure!(
        module.kind == "SNE",
        "Live sensor family has not been validated"
    );
    let count = usize::from(payload[6]);
    ensure!(
        count > 0 && count <= MAX_CHANNELS && count == module.channels,
        "Live channel count does not match module metadata"
    );
    let mut offset = 7;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        while payload
            .get(offset)
            .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
        {
            offset += 1;
        }
        let tail = payload
            .get(offset..)
            .ok_or_else(|| anyhow!("Missing live field"))?;
        let end = tail
            .iter()
            .position(|byte| *byte == b'\r')
            .ok_or_else(|| anyhow!("Unterminated live field"))?;
        let field = &tail[..end];
        ensure!(
            field
                .iter()
                .all(|byte| byte.is_ascii_graphic() || *byte == b' ' || *byte == 0xdf),
            "Unsupported live text encoding"
        );
        let colon = field
            .iter()
            .position(|byte| *byte == b':')
            .ok_or_else(|| anyhow!("Live field has no unit separator"))?;
        let unit: String = field[..colon]
            .iter()
            .map(|byte| {
                if *byte == 0xdf {
                    '°'
                } else {
                    char::from(*byte)
                }
            })
            .collect();
        let unit = unit.trim().to_owned();
        ensure!(
            !unit.is_empty(),
            "Unitless live field cannot become a measurement"
        );
        let display_value = std::str::from_utf8(&field[colon + 1..])?.trim().to_owned();
        let value: f64 = display_value.parse().context("Live value is not numeric")?;
        ensure!(value.is_finite(), "Non-finite live measurement");
        values.push(LiveValue {
            value,
            display_value,
            unit,
        });
        offset += end + 1;
    }
    while payload
        .get(offset)
        .is_some_and(|byte| matches!(byte, b'\r' | b'\n'))
    {
        offset += 1;
    }
    let device_timestamp = if payload.get(offset) == Some(&0xb0) {
        let date = payload
            .get(offset + 1..offset + 7)
            .ok_or_else(|| anyhow!("Truncated live timestamp"))?;
        let timestamp =
            NaiveDate::from_ymd_opt(2000 + bcd(date[2])? as i32, bcd(date[0])?, bcd(date[1])?)
                .and_then(|day| {
                    day.and_hms_opt(
                        bcd(date[3] & 0x7f).ok()?,
                        bcd(date[4]).ok()?,
                        bcd(date[5]).ok()?,
                    )
                })
                .ok_or_else(|| anyhow!("Invalid live date or time"))?;
        offset += 7;
        Some(timestamp.format("%Y-%m-%dT%H:%M:%S").to_string())
    } else {
        None
    };
    ensure!(
        payload.get(offset..).is_some_and(|tail| tail
            .iter()
            .all(|byte| matches!(byte, 0 | 0xff | b'\r' | b'\n'))),
        "Unrecognized trailing live data"
    );
    Ok(LiveSample {
        module_address: address,
        values,
        device_timestamp,
        received_at: Utc::now().to_rfc3339(),
        raw: hex(raw),
    })
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicitly opt-in, receive-only hardware diagnostic. Metadata and output
    /// are private runtime files; the test never starts or resets a live session.
    #[test]
    #[ignore = "requires an authorized live device and private capture paths"]
    fn passive_live_reader_diagnostic() -> Result<()> {
        use std::cell::Cell;

        let port_name = std::env::var("GREEN_SUITE_PASSIVE_PORT")
            .context("Set GREEN_SUITE_PASSIVE_PORT for this opt-in diagnostic")?;
        let metadata_path = std::env::var_os("GREEN_SUITE_PASSIVE_METADATA_CAPTURE")
            .context("Set GREEN_SUITE_PASSIVE_METADATA_CAPTURE to a prior raw capture")?;
        let output_path = std::env::var_os("GREEN_SUITE_PASSIVE_OUTPUT")
            .context("Set GREEN_SUITE_PASSIVE_OUTPUT to a new private capture path")?;
        let seconds = std::env::var("GREEN_SUITE_PASSIVE_SECONDS")
            .map(|value| value.parse::<u64>())
            .unwrap_or(Ok(20))
            .context("Passive duration must be an integer")?;
        ensure!(
            (1..=30).contains(&seconds),
            "Passive duration must be 1–30 seconds"
        );
        ensure!(
            std::fs::metadata(&metadata_path)?.len() <= MAX_CAPTURE_BYTES,
            "Metadata capture exceeds its size bound"
        );
        let metadata = std::fs::read_to_string(&metadata_path)?;
        let mut modules = None;
        for line in metadata.lines() {
            let event: Value = serde_json::from_str(line)
                .map_err(|_| anyhow!("Invalid private metadata capture JSON"))?;
            if event["event"] == "Modules" {
                modules = Some(
                    serde_json::from_value::<Vec<ModuleInfo>>(event["modules"].clone())
                        .map_err(|_| anyhow!("Invalid private module metadata"))?,
                );
                break;
            }
        }
        let modules = modules.context("Metadata capture contains no Modules event")?;
        ensure!(
            !modules.is_empty()
                && modules.len() <= usize::from(MAX_MODULES)
                && modules.iter().all(|module| {
                    module.address > 0
                        && module.address <= MAX_MODULES
                        && module.kind == "SNE"
                        && (1..=MAX_CHANNELS).contains(&module.channels)
                })
                && modules
                    .iter()
                    .map(|module| module.address)
                    .collect::<HashSet<_>>()
                    .len()
                    == modules.len(),
            "Metadata is outside the validated live module bounds"
        );
        let mut capture = Capture::create(Path::new(&output_path))?;
        capture.record(json!({
            "event":"PassiveOpening", "port":port_name, "seconds":seconds,
            "commands_sent":0, "modules":modules,
        }))?;
        let stop = AtomicBool::new(false);
        let warning_count = Cell::new(0usize);
        let emit = |event| {
            if matches!(event, LiveEvent::Warning(_)) {
                warning_count.set(warning_count.get() + 1);
            }
        };
        let mut frame_count = 0usize;
        let mut sample_count = 0usize;
        let mut decode_errors = 0usize;
        let result = (|| -> Result<()> {
            let port = open_live_port(&port_name)?;
            capture.record(json!({
                "event":"PortSettings", "baud":format!("{:?}", port.baud_rate()),
                "data_bits":format!("{:?}", port.data_bits()),
                "parity":format!("{:?}", port.parity()),
                "stop_bits":format!("{:?}", port.stop_bits()),
                "flow_control":format!("{:?}", port.flow_control()),
            }))?;
            let mut session = Session {
                port,
                stop: &stop,
                emit: &emit,
                capture: &mut capture,
                decoder: Decoder::default(),
                pending: VecDeque::new(),
                modules,
                warnings: HashSet::new(),
                modules_announced: true,
            };
            let deadline = Instant::now() + Duration::from_secs(seconds);
            while Instant::now() < deadline {
                if let Some(frame) = session.next_frame()? {
                    frame_count += 1;
                    if frame.payload.starts_with(&[0xb0, 0x42]) {
                        match decode_sample(&frame.payload, &frame.raw, &session.modules) {
                            Ok(sample) => {
                                session
                                    .capture
                                    .record(json!({"event":"PassiveSample", "sample":sample}))?;
                                sample_count += 1;
                            }
                            Err(error) => {
                                decode_errors += 1;
                                session.capture.record(json!({
                                    "event":"PassiveSampleRejected", "raw":hex(&frame.raw),
                                    "error":format!("{error:#}"),
                                }))?;
                            }
                        }
                    }
                }
            }
            ensure!(sample_count > 0, "No validated live samples arrived");
            Ok(())
        })();
        // Session has dropped its serial port before the summary is flushed.
        let summary = capture.record(json!({
            "event":"PassiveEnded", "commands_sent":0, "frames":frame_count,
            "validated_samples":sample_count, "decode_errors":decode_errors,
            "warnings":warning_count.get(), "device_mode_restored":false,
            "error":result.as_ref().err().map(|error| format!("{error:#}")),
        }));
        eprintln!(
            "passive frames={frame_count} validated_samples={sample_count} decode_errors={decode_errors} warnings={} failed={}",
            warning_count.get(),
            result.is_err() || summary.is_err()
        );
        ensure!(
            summary.is_ok(),
            "Passive diagnostic could not save its summary"
        );
        ensure!(
            result.is_ok(),
            "Passive diagnostic failed; inspect the private capture"
        );
        Ok(())
    }

    fn module() -> ModuleInfo {
        ModuleInfo {
            address: 1,
            kind: "SNE".into(),
            name: "Pressure test".into(),
            channels: 1,
        }
    }

    fn sample() -> Vec<u8> {
        let mut payload = vec![0xb0, 0x42, 0, 0, 1, 0, 1];
        payload.extend(b"inW: -0.0003\r\n");
        payload.extend([0xb0, 0x10, 0x30, 0x26, 0x12, 0x34, 0x56]);
        payload
    }

    #[test]
    fn live_framer_preserves_split_escapes_and_checksums() {
        let payload = [0xb0, 0x42, 0x7e, 0x7d, 0x11, 0x13];
        let encoded = crate::usb::encode_frame(&payload).unwrap();
        let mut decoder = Decoder::default();
        let mut decoded = Vec::new();
        for chunk in encoded.chunks(2) {
            for byte in chunk {
                if let Some(outcome) = decoder.feed(*byte) {
                    decoded.push(outcome);
                }
            }
        }
        let Outcome::Frame(frame) = &decoded[0] else {
            panic!("Expected complete frame")
        };
        assert_eq!(frame.payload, payload);
        assert_eq!(frame.raw, encoded);
        assert_eq!(frame.checksum, "ones-complement");
        assert_eq!(decoded.len(), 1);
        let summed = [0x7e, 0, 3, 0xb0, 0x40, 1, 0xf1];
        let result = summed
            .into_iter()
            .filter_map(|byte| decoder.feed(byte))
            .collect::<Vec<_>>();
        assert!(matches!(&result[0], Outcome::Frame(frame) if frame.checksum == "payload-sum"));
    }

    #[test]
    fn malformed_frames_recover_at_next_delimiter() {
        let mut decoder = Decoder::default();
        let mut wire = vec![0x7e, 0, 3, 0xb0, 0x40, 1, 0];
        wire.extend(crate::usb::encode_frame(&[0xb0, 0x40, 1]).unwrap());
        let outcomes = wire
            .into_iter()
            .filter_map(|byte| decoder.feed(byte))
            .collect::<Vec<_>>();
        assert!(matches!(outcomes[0], Outcome::Rejected { .. }));
        assert!(matches!(outcomes[1], Outcome::Frame(_)));
        decoder.feed(0x7e);
        decoder.feed(0);
        decoder.started = Some(Instant::now() - FRAME_TIMEOUT);
        assert!(matches!(decoder.expire(), Some(Outcome::Rejected { .. })));
        assert!(decoder.raw.is_empty());
    }

    #[test]
    fn pressure_samples_preserve_precision_units_and_instrument_time() {
        let payload = sample();
        let raw = crate::usb::encode_frame(&payload).unwrap();
        let reading = decode_sample(&payload, &raw, &[module()]).unwrap();
        assert_eq!(reading.values[0].value, -0.0003);
        assert_eq!(reading.values[0].display_value, "-0.0003");
        assert_eq!(reading.values[0].unit, "inW");
        assert_eq!(
            reading.device_timestamp.as_deref(),
            Some("2026-10-30T12:34:56")
        );
        assert_eq!(reading.raw, hex(&raw));
    }

    #[test]
    fn pressure_display_scale_and_hour_flag_are_preserved() {
        // Synthetic reading using the observed pressure-packet structure.
        let mut payload = vec![0xb0, 0x42, 0x90, 0, 1, 0x80, 1];
        payload.extend(b"inW: 0.0000\r\n");
        payload.extend([0xb0, 0x01, 0x02, 0x26, 0x97, 0x08, 0x09]);
        let reading = decode_sample(&payload, &[], &[module()]).unwrap();
        assert_eq!(reading.values[0].value, 0.0);
        assert_eq!(reading.values[0].display_value, "0.0000");
        assert_eq!(
            reading.device_timestamp.as_deref(),
            Some("2026-01-02T17:08:09")
        );
    }

    #[test]
    fn malformed_or_mismatched_live_samples_never_partially_decode() {
        let valid = sample();
        for end in 0..valid.len() {
            let truncated = &valid[..end];
            // A timestamp is optional; the complete field without its marker is valid.
            if truncated.ends_with(b"\r") || truncated.ends_with(b"\r\n") {
                continue;
            }
            assert!(
                decode_sample(truncated, &[], &[module()]).is_err(),
                "accepted length {end}"
            );
        }
        for value in ["NaN", "inf", "", "0.2 trailing"] {
            let mut payload = vec![0xb0, 0x42, 0, 0, 1, 0, 1];
            payload.extend(format!("inW: {value}\r").as_bytes());
            assert!(decode_sample(&payload, &[], &[module()]).is_err());
        }
        let mut unknown = valid.clone();
        unknown[4] = 2;
        assert!(decode_sample(&unknown, &[], &[module()]).is_err());
        let mut count = valid;
        count[6] = 2;
        assert!(decode_sample(&count, &[], &[module()]).is_err());
    }

    #[test]
    fn module_metadata_is_bounded_and_requires_display_identity() {
        let payload = b"\xb0\x41SNE\0\x01\0Pressure test\0\x05\0B3\x17";
        let module = decode_module(payload, 1).unwrap();
        assert_eq!(module.name, "Pressure test");
        assert_eq!(module.channels, 1);
        assert_eq!(module.kind, "SNE");
        for bad in [
            &b"\xb0\x41\0"[..],
            &b"\xb0\x41SNE\0\0\0name\0"[..],
            &b"\xb0\x41SNE\0\x01\0unterminated"[..],
        ] {
            assert!(decode_module(bad, 1).is_err());
        }
    }
}
