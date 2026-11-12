// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Shared serial setup. Opening a port never sends Reporter protocol commands.

use anyhow::{Context, Result};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::{
    collections::HashSet,
    io::{Read, Write},
    ops::{Deref, DerefMut},
    sync::{Mutex, OnceLock},
    time::Duration,
};

static OWNED_PORTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

struct PortLease(String);

impl PortLease {
    fn acquire(name: &str) -> Result<Self> {
        anyhow::ensure!(
            !name.is_empty() && name == name.trim(),
            "Invalid serial port name"
        );
        #[cfg(windows)]
        let key = name.trim_start_matches(r"\\.\").to_ascii_uppercase();
        #[cfg(not(windows))]
        let key = name.to_owned();
        let mut owned = OWNED_PORTS
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| anyhow::anyhow!("Serial ownership registry unavailable"))?;
        anyhow::ensure!(
            owned.insert(key.clone()),
            "This application already owns the selected serial port; finish its current operation first"
        );
        Ok(Self(key))
    }
}

impl Drop for PortLease {
    fn drop(&mut self) {
        if let Some(owned) = OWNED_PORTS.get()
            && let Ok(mut owned) = owned.lock()
        {
            owned.remove(&self.0);
        }
    }
}

/// Drops the OS handle before releasing application ownership. The application
/// keeps one wrapped handle per probe, download, or existing live session.
pub(crate) struct Connection {
    port: Box<dyn SerialPort>,
    _lease: PortLease,
}

impl Deref for Connection {
    type Target = dyn SerialPort;
    fn deref(&self) -> &Self::Target {
        &*self.port
    }
}
impl DerefMut for Connection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut *self.port
    }
}
impl Read for Connection {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.port.read(bytes)
    }
}
impl Write for Connection {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.port.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.port.flush()
    }
}

pub(crate) fn open(name: &str, timeout: Duration) -> Result<Connection> {
    let lease = PortLease::acquire(name)?;
    let mut port = serialport::new(name, 230_400)
        .data_bits(DataBits::Eight)
        .parity(Parity::None)
        .stop_bits(StopBits::One)
        .flow_control(FlowControl::None)
        .dtr_on_open(false)
        .timeout(timeout)
        .open()
        .map_err(|error| anyhow::anyhow!("Opening {name}: {error}; {}", open_diagnostic(name)))?;
    port.write_data_terminal_ready(false)
        .context("Disabling DTR")?;
    port.write_request_to_send(false).context("Disabling RTS")?;
    port.clear_break()
        .context("Returning serial transmit line to idle")?;
    Ok(Connection {
        port,
        _lease: lease,
    })
}

#[cfg(windows)]
fn open_diagnostic(name: &str) -> String {
    use std::os::windows::fs::OpenOptionsExt;
    // The serial library loses raw Win32 codes. A second, exclusive handle-open
    // check distinguishes a failed device from contention without changing its
    // serial settings or sending any bytes. A successful handle is closed here.
    let path = format!(r"\\.\{name}");
    match std::fs::OpenOptions::new()
        .read(true).write(true).share_mode(0).open(path)
    {
        Ok(_) => "Windows can open the port, but serial configuration failed; inspect the CP210x driver. No Reporter command was sent.".into(),
        Err(error) => windows_error_guidance(error.raw_os_error()),
    }
}

#[cfg(not(windows))]
fn open_diagnostic(_name: &str) -> String {
    "Check that the device is present and that this account has serial-port access. No Reporter command was sent.".into()
}

#[cfg(any(windows, test))]
fn windows_error_guidance(code: Option<i32>) -> String {
    let guidance = match code {
        Some(2 | 3 | 1167) => {
            "The port is absent or disconnected. Refresh the port list and inspect USB device status; a saved COM number does not prove a working connection."
        }
        Some(5 | 32) => {
            "Windows denied exclusive access. Another application may own the port, or access may be restricted."
        }
        Some(31 | 1117) => {
            "Windows reports a USB/device I/O failure before serial communication. Inspect USB enumeration, the CP210x driver, cable and device power; closing other applications alone may not resolve this."
        }
        _ => {
            "Windows could not open the serial device. Inspect its USB status and driver before retrying."
        }
    };
    let code = code.map_or_else(|| "unavailable".into(), |code| code.to_string());
    format!("Win32 error {code}: {guidance} No Reporter command was sent.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ownership_survives_competition_and_releases_on_drop() {
        let name = format!("ownership-{}", uuid::Uuid::new_v4());
        let first = PortLease::acquire(&name).unwrap();
        assert!(PortLease::acquire(&name).is_err());
        let other = PortLease::acquire(&format!("{name}-other")).unwrap();
        drop(other);
        assert!(PortLease::acquire(&name).is_err());
        drop(first);
        assert!(PortLease::acquire(&name).is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn windows_aliases_share_ownership() {
        let first = PortLease::acquire("COM64591").unwrap();
        assert!(PortLease::acquire(r"\\.\com64591").is_err());
        drop(first);
    }

    #[test]
    fn driver_failure_is_not_reported_as_port_contention() {
        let message = windows_error_guidance(Some(31));
        assert!(message.contains("before serial communication"));
        assert!(!message.contains("denied exclusive access"));
    }

    #[test]
    fn missing_and_exclusive_access_failures_have_different_recovery() {
        assert!(windows_error_guidance(Some(2)).contains("Refresh the port list"));
        assert!(windows_error_guidance(Some(5)).contains("Another application"));
        assert!(windows_error_guidance(Some(1167)).contains("disconnected"));
    }
}
