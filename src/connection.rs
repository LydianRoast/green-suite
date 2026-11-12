// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Shared serial setup. Opening a port never sends Reporter protocol commands.

use anyhow::{Context, Result};
use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::time::Duration;

pub(crate) fn open(name: &str, timeout: Duration) -> Result<Box<dyn SerialPort>> {
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
    Ok(port)
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
