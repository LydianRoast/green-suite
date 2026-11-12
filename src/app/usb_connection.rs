// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;

impl GreenSuite {
    pub(super) fn accept_download_identity(&mut self, epoch: u64, device: usb::DeviceInfo) {
        if epoch == self.connection_epoch && device.port == self.port {
            self.device = Some(device);
        }
    }
    pub(super) fn refresh_inventory(&mut self, ctx: &Context) {
        if self.inventory_pending || !self.inventory_enabled {
            return;
        }
        self.inventory_pending = true;
        self.inventory_polled_at = Some(Instant::now());
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(usb::inventory::enumerate)
                .map_err(|_| "OS port enumeration worker stopped unexpectedly".to_string())
                .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = tx.send(Event::Inventory(result));
            ctx.request_repaint();
        });
    }
    pub(super) fn apply_inventory(
        &mut self,
        snapshot: usb::inventory::Snapshot,
    ) -> anyhow::Result<()> {
        let old = self.inventory.get(&self.port).cloned();
        self.inventory.update(snapshot)?;
        self.inventory_error = None;
        self.ports = self
            .inventory
            .ports
            .iter()
            .map(|p| p.name.clone())
            .collect();
        if !self.port.is_empty()
            && old.as_ref().is_none_or(|old| {
                self.inventory
                    .get(&self.port)
                    .is_none_or(|new| !old.same_binding(new))
            })
        {
            self.connection_epoch = self.connection_epoch.wrapping_add(1);
            self.device = None;
            self.port.clear();
            self.message("Selected USB port disappeared or its bridge details changed. Select a current port and reconnect; received readings remain in the project.",true);
        }
        Ok(())
    }
    pub(super) fn export_usb_summary(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("usb-summary.json")
            .add_filter("JSON", &["json"])
            .save_file()
        {
            let report = self
                .inventory
                .redacted_summary(self.inventory_error.is_some(), self.device.is_some());
            match usb::inventory::write_summary(&path, &report) {
                Ok(()) => self.message(
                    "Redacted USB summary saved. Device identifiers and project data were omitted.",
                    false,
                ),
                Err(error) => {
                    self.message(format!("USB summary could not be saved: {error:#}"), true)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn port(name: &str) -> usb::inventory::Port {
        usb::inventory::Port {
            name: name.into(),
            vid: Some(0x10c4),
            pid: Some(0xea60),
            bridge_serial: Some("EXAMPLE".into()),
            manufacturer: None,
            product: None,
            location: Some("EXAMPLE_LOCATION".into()),
        }
    }
    fn snapshot(ports: Vec<usb::inventory::Port>) -> usb::inventory::Snapshot {
        usb::inventory::Snapshot {
            ports,
            location_warning: None,
        }
    }
    fn reporter() -> usb::DeviceInfo {
        usb::DeviceInfo {
            port: "COM95".into(),
            model: "WR-401".into(),
            serial: "EXAMPLE_REPORTER".into(),
            firmware: "EXAMPLE".into(),
            group_count: 0,
            record_count: 0,
        }
    }
    #[test]
    fn unplug_replug_and_changed_com_require_explicit_selection_and_reidentification() {
        let ctx = Context::default();
        let mut app = GreenSuite::preview(&ctx, "readings", true, Scheme::default(), true);
        app.apply_inventory(snapshot(vec![port("COM95")])).unwrap();
        app.port = "COM95".into();
        app.device = Some(reporter());
        app.previous_reporter = app.device.clone();
        app.previous_bridge = Some(port("COM95"));
        let readings = serde_json::to_value(&app.project.readings).unwrap();
        let epoch = app.connection_epoch;
        app.apply_inventory(snapshot(vec![])).unwrap();
        assert!(app.device.is_none() && app.port.is_empty());
        assert!(app.connection_epoch > epoch);
        assert_eq!(
            app.previous_reporter.as_ref().unwrap().serial,
            "EXAMPLE_REPORTER"
        );
        app.apply_inventory(snapshot(vec![port("COM96")])).unwrap();
        assert!(app.device.is_none() && app.port.is_empty());
        assert_eq!(app.ports, vec!["COM96"]);
        assert_eq!(
            serde_json::to_value(&app.project.readings).unwrap(),
            readings
        );
        app.tx.send(Event::Probe(epoch, Ok(reporter()))).unwrap();
        app.receive(&ctx);
        assert!(app.device.is_none());
    }
    #[test]
    fn changed_binding_invalidates_identity_but_failed_enumeration_preserves_last_inventory() {
        let ctx = Context::default();
        let mut app = GreenSuite::preview(&ctx, "readings", true, Scheme::default(), true);
        app.apply_inventory(snapshot(vec![port("COM95")])).unwrap();
        app.port = "COM95".into();
        app.device = Some(reporter());
        app.tx
            .send(Event::Inventory(Err("Synthetic OS failure".into())))
            .unwrap();
        app.receive(&ctx);
        assert!(app.device.is_some());
        assert_eq!(app.port, "COM95");
        assert!(app.inventory_error.is_some());
        let mut changed = port("COM95");
        changed.bridge_serial = Some("DIFFERENT_EXAMPLE".into());
        app.apply_inventory(snapshot(vec![changed])).unwrap();
        assert!(app.device.is_none() && app.port.is_empty());
    }
    #[test]
    fn preview_does_not_enable_os_polling() {
        let ctx = Context::default();
        let app = GreenSuite::preview(&ctx, "readings", false, Scheme::default(), false);
        assert!(!app.inventory_enabled && !app.inventory_pending);
    }
    #[test]
    fn download_completion_cannot_restore_a_stale_or_different_port_identity() {
        let ctx = Context::default();
        let mut app = GreenSuite::preview(&ctx, "readings", false, Scheme::default(), false);
        app.port = "COM95".into();
        app.connection_epoch = 2;
        app.accept_download_identity(1, reporter());
        assert!(app.device.is_none());
        app.port = "COM96".into();
        app.accept_download_identity(2, reporter());
        assert!(app.device.is_none());
        app.port = "COM95".into();
        app.accept_download_identity(2, reporter());
        assert!(app.device.is_some());
    }
}
