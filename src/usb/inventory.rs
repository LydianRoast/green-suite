// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Passive OS inventory only: never opens a serial handle or sends protocol bytes.
use anyhow::{Result, ensure};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

#[cfg(windows)]
mod windows;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Port {
    pub name: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
    pub bridge_serial: Option<String>,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub location: Option<String>,
}
impl Port {
    pub fn same_binding(&self, other: &Self) -> bool {
        self.name == other.name
            && self.vid == other.vid
            && self.pid == other.pid
            && self.bridge_serial == other.bridge_serial
            && self.location == other.location
    }
    pub fn hint(&self, previous: &Self) -> &'static str {
        if self.vid.is_some() && self.vid == previous.vid && self.pid == previous.pid {
            if self.bridge_serial.as_ref().is_some_and(|s| !s.is_empty())
                && self.bridge_serial == previous.bridge_serial
            {
                "bridge serial matches; Reporter unverified"
            } else if self.location.is_some() && self.location == previous.location {
                "USB location matches; Reporter unverified"
            } else {
                "USB model matches; Reporter unverified"
            }
        } else {
            "Reporter unverified"
        }
    }
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub ports: Vec<Port>,
    pub location_warning: Option<String>,
}
pub fn enumerate() -> Result<Snapshot> {
    let mut ports: Vec<_> = serialport::available_ports()?
        .into_iter()
        .map(|info| {
            let mut port = Port {
                name: info.port_name,
                vid: None,
                pid: None,
                bridge_serial: None,
                manufacturer: None,
                product: None,
                location: None,
            };
            if let serialport::SerialPortType::UsbPort(info) = info.port_type {
                port.vid = Some(info.vid);
                port.pid = Some(info.pid);
                port.bridge_serial = info.serial_number;
                port.manufacturer = info.manufacturer;
                port.product = info.product;
            }
            port
        })
        .collect();
    #[cfg(windows)]
    let location_warning = match windows::locations() {
        Ok(locations) => {
            for port in &mut ports {
                port.location = locations.get(&port.name).cloned();
            }
            None
        }
        Err(_) => Some(
            "Windows USB location metadata is unavailable; port enumeration remains available"
                .into(),
        ),
    };
    #[cfg(not(windows))]
    let location_warning = None;
    for port in &mut ports {
        for value in [
            &mut port.bridge_serial,
            &mut port.manufacturer,
            &mut port.product,
            &mut port.location,
        ] {
            *value = value
                .take()
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.chars().take(2048).collect());
        }
    }
    normalize(&mut ports)?;
    Ok(Snapshot {
        ports,
        location_warning,
    })
}
fn normalize(ports: &mut [Port]) -> Result<()> {
    ensure!(
        ports.len() <= 512,
        "Port inventory exceeds the supported device count"
    );
    ports.sort_by(|a, b| a.name.cmp(&b.name));
    for (index, port) in ports.iter().enumerate() {
        ensure!(
            !port.name.is_empty() && port.name.len() <= 256 && port.name.trim() == port.name,
            "Invalid OS port name"
        );
        ensure!(
            index == 0 || ports[index - 1].name != port.name,
            "OS returned ambiguous duplicate port names"
        );
    }
    Ok(())
}
pub fn write_summary(path: &std::path::Path, report: &serde_json::Value) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(serde_json::to_string_pretty(report)?.as_bytes())?;
    file.sync_all()?;
    Ok(())
}
#[derive(Clone, Debug, Serialize)]
pub enum ChangeKind {
    Arrived,
    Departed,
    BindingChanged,
}
#[derive(Clone, Debug)]
struct Change {
    utc: String,
    kind: ChangeKind,
    port: String,
}
#[derive(Default)]
pub struct Tracker {
    pub ports: Vec<Port>,
    events: VecDeque<Change>,
    pub location_warning: Option<String>,
}
impl Tracker {
    pub fn get(&self, name: &str) -> Option<&Port> {
        self.ports.iter().find(|p| p.name == name)
    }
    pub fn update(&mut self, mut snapshot: Snapshot) -> Result<()> {
        normalize(&mut snapshot.ports)?;
        let utc = chrono::Utc::now().to_rfc3339();
        let mut changes = Vec::new();
        for old in &self.ports {
            match snapshot.ports.iter().find(|p| p.name == old.name) {
                None => changes.push((ChangeKind::Departed, old.name.clone())),
                Some(new) if !old.same_binding(new) => {
                    changes.push((ChangeKind::BindingChanged, old.name.clone()))
                }
                _ => {}
            }
        }
        for port in &snapshot.ports {
            if self.get(&port.name).is_none() {
                changes.push((ChangeKind::Arrived, port.name.clone()));
            }
        }
        for (kind, port) in changes {
            self.events.push_back(Change {
                utc: utc.clone(),
                kind,
                port,
            });
        }
        while self.events.len() > 64 {
            self.events.pop_front();
        }
        self.ports = snapshot.ports;
        self.location_warning = snapshot.location_warning;
        Ok(())
    }
    /// Identifiers and free-form OS/error strings never enter this summary.
    pub fn redacted_summary(
        &self,
        enumeration_failed: bool,
        reporter_identified: bool,
    ) -> serde_json::Value {
        let mut aliases = BTreeMap::new();
        for name in self
            .ports
            .iter()
            .map(|p| &p.name)
            .chain(self.events.iter().map(|e| &e.port))
        {
            let next = aliases.len() + 1;
            aliases
                .entry(name)
                .or_insert_with(|| format!("Port {next}"));
        }
        let ports:Vec<_>=self.ports.iter().map(|p|serde_json::json!({"port":aliases[&p.name],"vid":p.vid,"pid":p.pid,"bridge_serial_available":p.bridge_serial.is_some(),"location_available":p.location.is_some()})).collect();
        let events: Vec<_> = self
            .events
            .iter()
            .map(|e| serde_json::json!({"utc":e.utc,"kind":e.kind,"port":aliases[&e.port]}))
            .collect();
        serde_json::json!({"format":"green-suite-usb-summary-v1","created_utc":chrono::Utc::now().to_rfc3339(),"identifiers_redacted":true,"enumeration_failed":enumeration_failed,"location_metadata_unavailable":self.location_warning.is_some(),"reporter_identified":reporter_identified,"ports":ports,"recent_events":events,"scope":"Passive OS inventory and application identification status only; no protocol bytes, project data, port names, serial numbers, USB locations or error messages included"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn port(name: &str) -> Port {
        Port {
            name: name.into(),
            vid: Some(0x10c4),
            pid: Some(0xea60),
            bridge_serial: Some("SYNTHETIC_BRIDGE".into()),
            manufacturer: Some("SYNTHETIC_VENDOR".into()),
            product: None,
            location: Some("SYNTHETIC_LOCATION".into()),
        }
    }
    fn snapshot(ports: Vec<Port>) -> Snapshot {
        Snapshot {
            ports,
            location_warning: None,
        }
    }
    #[test]
    fn transitions_preserve_distinct_ports_even_with_shared_bridge_serials() {
        let mut tracker = Tracker::default();
        tracker
            .update(snapshot(vec![port("COM91"), port("COM92")]))
            .unwrap();
        assert_eq!(tracker.ports.len(), 2);
        assert!(
            tracker.ports[1]
                .hint(&tracker.ports[0])
                .contains("Reporter unverified")
        );
        tracker.update(snapshot(vec![port("COM92")])).unwrap();
        assert!(matches!(
            tracker.events.back().unwrap().kind,
            ChangeKind::Departed
        ));
        let mut changed = port("COM92");
        changed.bridge_serial = Some("SYNTHETIC_OTHER".into());
        tracker.update(snapshot(vec![changed])).unwrap();
        assert!(matches!(
            tracker.events.back().unwrap().kind,
            ChangeKind::BindingChanged
        ));
        let original = tracker.ports.clone();
        assert!(
            tracker
                .update(snapshot(vec![port("COM92"), port("COM92")]))
                .is_err()
        );
        assert_eq!(tracker.ports, original);
    }
    #[test]
    fn event_history_is_bounded_and_metadata_changes_do_not_rebind_the_port() {
        let mut tracker = Tracker::default();
        let first = port("COM93");
        let mut renamed = first.clone();
        renamed.manufacturer = Some("Renamed vendor".into());
        assert!(first.same_binding(&renamed));
        for _ in 0..80 {
            tracker.update(snapshot(vec![first.clone()])).unwrap();
            tracker.update(snapshot(vec![])).unwrap();
        }
        assert_eq!(tracker.events.len(), 64);
        renamed.location = None;
        assert!(!first.same_binding(&renamed));
        renamed.vid = None;
        assert_eq!(renamed.hint(&first), "Reporter unverified");
    }
    #[test]
    fn summary_omits_identifiers_free_text_and_never_replaces_existing_files() {
        let mut tracker = Tracker::default();
        tracker.update(snapshot(vec![port("COM94")])).unwrap();
        let report = tracker.redacted_summary(true, false);
        let text = serde_json::to_string(&report).unwrap();
        for omitted in [
            "COM94",
            "SYNTHETIC_BRIDGE",
            "SYNTHETIC_VENDOR",
            "SYNTHETIC_LOCATION",
        ] {
            assert!(!text.contains(omitted));
        }
        assert_eq!(report["enumeration_failed"], true);
        let path = std::env::temp_dir().join(format!(
            "green-suite-usb-summary-{}.json",
            uuid::Uuid::new_v4()
        ));
        write_summary(&path, &report).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert!(write_summary(&path, &serde_json::json!({})).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_file(path).unwrap();
    }
}
