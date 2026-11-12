// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;

impl GreenSuite {
    pub(super) fn header(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        TopBottomPanel::top("header")
            .exact_height(86.)
            .frame(
                Frame::new()
                    .fill(colors.paper)
                    .inner_margin(Margin::symmetric(18, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let title = ui.label(RichText::new("Green Suite").font(serif_bold(20.)));
                    ui.painter().vline(
                        title.rect.left() - 6.,
                        title.rect.y_range(),
                        Stroke::new(3., colors.brand),
                    );
                    ui.separator();
                    let name_width = (ui.available_width() - 390.).clamp(120., 280.);
                    if ui
                        .add(TextEdit::singleline(&mut self.project.name).desired_width(name_width))
                        .on_hover_text("Project name")
                        .changed()
                    {
                        self.changed();
                    }
                    ui.menu_button("File", |ui| self.project_menu(ui, ctx));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button(if self.dark_mode { "Light" } else { "Dark" })
                            .on_hover_text("Switch light / dark appearance")
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            self.persist_appearance(ctx);
                        }
                        let before = self.scheme;
                        ui.menu_button("Appearance", |ui| {
                            for scheme in [Scheme::Evergreen, Scheme::Urbane] {
                                if ui
                                    .selectable_value(&mut self.scheme, scheme, scheme.label())
                                    .clicked()
                                {
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.button("About Green Suite").clicked() {
                                self.show_about = true;
                                ui.close();
                            }
                        });
                        if before != self.scheme {
                            self.persist_appearance(ctx);
                        }
                        if ui
                            .button("Save")
                            .on_hover_text("Save project · Ctrl+S")
                            .clicked()
                        {
                            self.save();
                        }
                    });
                });
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.;
                    for (view, label) in [
                        (View::Blueprint, "Plan"),
                        (View::Readings, "Readings"),
                        (View::Traverse, "Traverse"),
                        (View::Live, "Live"),
                    ] {
                        let selected = self.view == view;
                        let response = ui.add(
                            Button::selectable(
                                selected,
                                RichText::new(label).font(serif_bold(14.)),
                            )
                            .min_size(vec2(100., 28.)),
                        );
                        if selected {
                            ui.painter().hline(
                                response.rect.x_range(),
                                response.rect.bottom(),
                                Stroke::new(3., colors.green),
                            );
                        }
                        if response.clicked() {
                            self.view = view;
                        }
                    }
                    if self.demo
                        || self.project.readings.iter().any(|r| r.source == "example")
                        || self.project.documents.iter().any(|d| d.path.is_empty())
                    {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new("EXAMPLE DATA")
                                    .font(serif_bold(11.))
                                    .color(colors.warning),
                            );
                        });
                    }
                });
            });
    }

    fn project_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        if ui
            .add_enabled(!self.busy, Button::new("New project"))
            .clicked()
            && self.save()
        {
            let project = Project::new("Untitled survey");
            let path = default_path(&project);
            self.replace_project(project, path, false, ctx);
            self.changed();
            ui.close();
        }
        if ui
            .add_enabled(!self.busy, Button::new("Open project…"))
            .clicked()
        {
            ui.close();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Green Suite project", &["json"])
                .pick_file()
                && self.save()
            {
                match Project::load(&path) {
                    Ok(project) => self.replace_project(project, path, false, ctx),
                    Err(error) => self.message(error.to_string(), true),
                }
            }
        }
        if ui.button("Save as…").clicked() {
            ui.close();
            if let Some(path) = rfd::FileDialog::new()
                .set_file_name("Survey.green-suite.json")
                .save_file()
            {
                self.path = path;
                self.demo = false;
                self.save();
            }
        }
        ui.separator();
        if ui
            .add_enabled(!self.busy, Button::new("Open example project"))
            .clicked()
            && self.save()
        {
            let project = example_project();
            let path = default_path(&project);
            self.replace_project(project, path, true, ctx);
            ui.close();
        }
    }

    pub(super) fn sidebar(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        SidePanel::left("project-sidebar")
            .exact_width(220.)
            .resizable(false)
            .frame(Frame::new().fill(colors.paper).inner_margin(14))
            .show(ctx, |ui| {
                if ui
                    .add_enabled(
                        !self.busy,
                        Button::new("Add PDF…").min_size(vec2(ui.available_width(), 30.)),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Mechanical blueprint", &["pdf"])
                        .pick_file()
                {
                    self.import_pdf(path, ctx);
                }
                ui.add_space(8.);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.show_devices, false, "Sheets");
                    ui.selectable_value(&mut self.show_devices, true, "Devices");
                });
                ui.add_space(8.);
                ScrollArea::vertical()
                    .id_salt("project-navigation")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if !self.show_devices {
                            let mut chosen = None;
                            for (index, document) in self.project.documents.iter().enumerate() {
                                if ui
                                    .add(
                                        Button::selectable(self.document == index, &document.name)
                                            .wrap_mode(TextWrapMode::Wrap),
                                    )
                                    .on_hover_text(format!("{} pages", document.page_count))
                                    .clicked()
                                {
                                    chosen = Some(index);
                                }
                            }
                            if let Some(index) = chosen {
                                self.document = index;
                                self.page = 0;
                                self.request_page(ctx);
                            }
                        } else {
                            ui.add(
                                TextEdit::singleline(&mut self.point_search)
                                    .hint_text("Find device…")
                                    .desired_width(ui.available_width()),
                            );
                            let needle = self.point_search.to_lowercase();
                            let mut chosen = None;
                            for point in &self.project.points {
                                if !format!("{} {} {}", point.label, point.system, point.kind)
                                    .to_lowercase()
                                    .contains(&needle)
                                {
                                    continue;
                                }
                                if ui
                                    .selectable_label(
                                        self.selected_point.as_ref() == Some(&point.id),
                                        &point.label,
                                    )
                                    .on_hover_text(format!(
                                        "{} · {} readings",
                                        point.kind,
                                        point.reading_ids.len()
                                    ))
                                    .clicked()
                                {
                                    chosen = Some((
                                        point.id.clone(),
                                        point.document_id.clone(),
                                        point.page,
                                    ));
                                }
                            }
                            if let Some((id, document, page)) = chosen {
                                self.selected_point = Some(id);
                                if let Some(index) = self
                                    .project
                                    .documents
                                    .iter()
                                    .position(|item| item.id == document)
                                    && (index != self.document || page != self.page)
                                {
                                    self.document = index;
                                    self.page = page;
                                    self.request_page(ctx);
                                }
                            }
                            if self.project.points.is_empty() {
                                ui.label("No devices placed.");
                            }
                        }
                    });
            });
    }

    pub(super) fn usb_bar(&mut self, ui: &mut Ui, download: bool) {
        let ctx = ui.ctx().clone();
        let colors = palette(&ctx);
        ui.horizontal_wrapped(|ui| {
            let identified = self
                .device
                .as_ref()
                .is_some_and(|device| device.port == self.port);
            if identified {
                ui.label(
                    RichText::new(if download { "WR-401 identified" } else { "WR-401" })
                    .color(colors.green),
                );
                ui.menu_button("Connection", |ui| {
                    if let Some(device) = &self.device {
                        ui.label(&device.model);
                        ui.label(format!("{} · {}", device.serial, device.firmware));
                        ui.label(format!(
                            "{} records · {} groups",
                            device.record_count, device.group_count
                        ));
                        ui.separator();
                    }
                    self.connection_controls(ui, &ctx);
                });
            } else {
                self.connection_controls(ui, &ctx);
            }
            if download
                && ui
                    .add_enabled(
                        !self.busy && !self.live_running && !self.demo && identified,
                        Button::new(RichText::new("Download records").color(Color32::WHITE))
                            .fill(colors.action),
                    )
                    .clicked()
            {
                self.download(&ctx);
            }
            if !download {
                if self.live_running {
                    let stopping = self.live_stop.load(Ordering::Relaxed);
                    if ui.add_enabled(!stopping, Button::new("Stop receiving")).clicked() {
                        self.live_stop.store(true, Ordering::Relaxed);
                    }
                } else if self.live_compatible() && ui.add_enabled(!self.busy,
                    Button::new(RichText::new("Start live reception").color(Color32::WHITE)).fill(colors.action)).clicked() {
                    self.start_live(&ctx);
                }
                if self.live_running || self.live_compatible() {
                    ui.label(RichText::new("Restart Reporter after use.").small().color(colors.muted));
                }
                ui.menu_button("Setup", |ui| {
                    ui.set_max_width(390.);
                    ui.label("Connect with sensors off. Turn sensors on and wait for readings, then start reception.");
                    ui.separator();
                    ui.label(LIVE_RECOVERY);
                    if let Some(path) = &self.live_raw_path && ui.button("Session capture…").clicked() {
                        self.status_details = Some(format!("Raw session capture:\n{}", path.display()));
                        ui.close();
                    }
                });
            }
        });
    }

    fn connection_controls(&mut self, ui: &mut Ui, ctx: &Context) {
        let enabled = !self.busy && !self.live_running && !self.demo;
        if self.device.is_none()
            && let Some(previous) = &self.previous_reporter
        {
            ui.small(format!(
                "Previous: {} · {}",
                previous.model, previous.serial
            ));
        }
        ui.add_enabled_ui(enabled, |ui| {
            ComboBox::from_id_salt("ports")
                .width(105.)
                .selected_text(if self.port.is_empty() {
                    "USB port"
                } else {
                    &self.port
                })
                .show_ui(ui, |ui| {
                    for port in &self.ports {
                        let hint = self.previous_bridge.as_ref().and_then(|previous| {
                            self.inventory
                                .get(port)
                                .map(|current| current.hint(previous))
                        });
                        if ui
                            .selectable_value(
                                &mut self.port,
                                port.clone(),
                                hint.map(|hint| format!("{port} · {hint}"))
                                    .unwrap_or_else(|| port.clone()),
                            )
                            .changed()
                        {
                            self.device = None;
                            self.connection_epoch = self.connection_epoch.wrapping_add(1);
                        }
                    }
                });
        });
        if ui
            .add_enabled(
                enabled && !self.port.is_empty(),
                Button::new(if self.previous_reporter.is_some() {
                    "Reconnect"
                } else {
                    "Connect"
                }),
            )
            .clicked()
        {
            self.probe(ctx, true);
        }
        if ui
            .add_enabled(
                !self.inventory_pending && !self.demo,
                Button::new("Refresh"),
            )
            .on_hover_text("Refresh USB ports")
            .clicked()
        {
            self.refresh_inventory(ctx);
        }
        ui.menu_button("USB details",|ui| {
            ui.set_max_width(480.);
            if let Some(previous)=&self.previous_reporter {
                ui.label(format!("Previous Reporter: {} · {} · {}",previous.model,previous.serial,previous.firmware));
                if self.device.is_none() && ui.add_enabled(enabled && !self.port.is_empty(),Button::new("Identify selected as another Reporter")).clicked() {self.probe(ctx,false);ui.close();}
            }
            if let Some(port)=self.inventory.get(&self.port) {
                if let (Some(vid),Some(pid))=(port.vid,port.pid) {ui.label(format!("USB VID/PID: {vid:04X}/{pid:04X}"));}
                ui.label(format!("Bridge serial: {}",port.bridge_serial.as_deref().unwrap_or("Unavailable")));
                ui.label(format!("USB location: {}",port.location.as_deref().unwrap_or("Unavailable")));
            }
            if self.inventory_pending {ui.label("Refreshing OS port inventory…");}
            if self.inventory_error.is_some() {ui.label("Port enumeration unavailable; last successful inventory retained.");}
            if let Some(warning)=&self.inventory.location_warning {ui.label(warning);}
            ui.label("USB details are hints. Reconnect checks the previous Reporter serial before enabling a download. A new download starts from the beginning; it does not resume a record cursor.");
            if ui.button("Export redacted USB summary…").clicked() {self.export_usb_summary();ui.close();}
        });
    }
}
