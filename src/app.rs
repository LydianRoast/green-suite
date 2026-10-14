// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use crate::{model::*, pdf, traverse, usb};
use eframe::egui::{self, *};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

const GREEN: Color32 = Color32::from_rgb(19, 119, 93);
const INK: Color32 = Color32::from_rgb(28, 46, 51);
const MUTED: Color32 = Color32::from_rgb(99, 116, 119);
const LINE: Color32 = Color32::from_rgb(218, 227, 224);
const PAPER: Color32 = Color32::from_rgb(247, 249, 247);

#[derive(Clone, Copy)]
struct Palette {
    green: Color32,
    ink: Color32,
    muted: Color32,
    line: Color32,
    paper: Color32,
    panel: Color32,
    action: Color32,
    selection: Color32,
    alternate: Color32,
    canvas: Color32,
    grid: Color32,
    warning: Color32,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                green: Color32::from_rgb(132, 218, 178),
                ink: Color32::from_rgb(238, 234, 221),
                muted: Color32::from_rgb(174, 191, 179),
                line: Color32::from_rgb(68, 87, 75),
                paper: Color32::from_rgb(23, 31, 27),
                panel: Color32::from_rgb(31, 41, 35),
                action: GREEN,
                selection: Color32::from_rgb(49, 83, 63),
                alternate: Color32::from_rgb(35, 48, 40),
                canvas: Color32::from_rgb(17, 25, 21),
                grid: Color32::from_rgb(43, 58, 48),
                warning: Color32::from_rgb(242, 182, 111),
            }
        } else {
            Self {
                green: GREEN,
                ink: INK,
                muted: MUTED,
                line: LINE,
                paper: PAPER,
                panel: Color32::from_rgb(255, 254, 249),
                action: GREEN,
                selection: Color32::from_rgb(216, 237, 227),
                alternate: Color32::from_rgb(246, 249, 244),
                canvas: Color32::from_rgb(230, 237, 233),
                grid: Color32::from_rgb(203, 214, 207),
                warning: Color32::from_rgb(160, 95, 28),
            }
        }
    }
}

fn palette(ctx: &Context) -> Palette {
    Palette::new(ctx.style().visuals.dark_mode)
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Blueprint,
    Readings,
    Traverse,
}
enum Event {
    Probe(Result<usb::DeviceInfo, String>),
    Download(Result<usb::DownloadResult, String>),
    Progress(String),
    Document(Result<Document, String>),
    Page(u64, Result<(usize, usize, Vec<u8>), String>),
}

pub struct GreenSuite {
    project: Project,
    path: PathBuf,
    dirty: bool,
    last_save: Instant,
    view: View,
    document: usize,
    page: usize,
    selected_point: Option<String>,
    selected_reading: Option<String>,
    traverse_index: usize,
    traverse_view: traverse::TraverseView,
    texture: Option<TextureHandle>,
    page_request: u64,
    page_loading: bool,
    zoom: f32,
    pan: Vec2,
    add_point: bool,
    point_kind: String,
    dragging_point: Option<String>,
    search: String,
    reading_picker_search: String,
    point_search: String,
    show_devices: bool,
    unit_filter: String,
    ports: Vec<String>,
    port: String,
    device: Option<usb::DeviceInfo>,
    busy: bool,
    status: String,
    error: bool,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    manual_value: f64,
    demo: bool,
    screenshot: Option<PathBuf>,
    capture_at: Instant,
    capture_requested: bool,
    confirm_delete: Option<String>,
    show_about: bool,
    status_details: Option<String>,
    source_details: Option<Reading>,
    dark_mode: bool,
}

impl GreenSuite {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        project_path: Option<PathBuf>,
        demo: bool,
        screenshot: Option<PathBuf>,
        start_view: &str,
    ) -> Self {
        let dark_mode = std::fs::read_to_string(data_dir().join("appearance.txt"))
            .is_ok_and(|value| value.trim() == "dark");
        configure_fonts(&cc.egui_ctx);
        theme(&cc.egui_ctx, dark_mode);
        let (tx, rx) = mpsc::channel();
        let remembered = std::fs::read_to_string(data_dir().join("last-project.txt"))
            .ok()
            .map(PathBuf::from);
        let requested = project_path.or(remembered);
        let mut startup_error = None;
        let project = if demo {
            example_project()
        } else if let Some(ref p) = requested {
            match Project::load(p) {
                Ok(p) => p,
                Err(e) => {
                    startup_error = Some(format!("Could not open the last project: {e}"));
                    Project::new("Untitled survey")
                }
            }
        } else {
            Project::new("Untitled survey")
        };
        let path = if demo || startup_error.is_some() {
            default_path(&project)
        } else {
            requested
                .filter(|p| p.exists())
                .unwrap_or_else(|| default_path(&project))
        };
        let ports = usb::list_ports().unwrap_or_default();
        let mut app = Self {
            project,
            path,
            dirty: false,
            last_save: Instant::now(),
            view: match start_view {
                "readings" => View::Readings,
                "traverse" => View::Traverse,
                _ => View::Blueprint,
            },
            document: 0,
            page: 0,
            selected_point: None,
            selected_reading: None,
            traverse_index: 0,
            traverse_view: Default::default(),
            texture: None,
            page_request: 0,
            page_loading: false,
            zoom: 1.,
            pan: Vec2::ZERO,
            add_point: false,
            point_kind: "Supply air".into(),
            dragging_point: None,
            search: String::new(),
            reading_picker_search: String::new(),
            point_search: String::new(),
            show_devices: demo,
            unit_filter: "All units".into(),
            port: ports.first().cloned().unwrap_or_default(),
            ports,
            device: None,
            busy: false,
            status: startup_error
                .clone()
                .unwrap_or("Ready. Connect a Wrist Reporter or import a blueprint.".into()),
            error: startup_error.is_some(),
            tx,
            rx,
            manual_value: 0.,
            demo,
            screenshot,
            capture_at: Instant::now() + Duration::from_secs(3),
            capture_requested: false,
            confirm_delete: None,
            show_about: false,
            status_details: None,
            source_details: None,
            dark_mode,
        };
        if demo {
            app.selected_point = app.project.points.first().map(|p| p.id.clone());
        }
        app.request_page(&cc.egui_ctx);
        if !app.demo && startup_error.is_none() {
            app.save();
        }
        app
    }
    fn changed(&mut self) {
        self.dirty = true;
    }
    fn message(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }
    fn save(&mut self) -> bool {
        match self.project.save(&self.path) {
            Ok(()) => {
                self.dirty = false;
                self.last_save = Instant::now();
                if !self.demo {
                    let _ = std::fs::create_dir_all(data_dir());
                    let _ = std::fs::write(
                        data_dir().join("last-project.txt"),
                        self.path.to_string_lossy().as_bytes(),
                    );
                }
                true
            }
            Err(e) => {
                self.last_save = Instant::now();
                self.message(format!("Save failed: {e}"), true);
                false
            }
        }
    }
    fn replace_project(&mut self, p: Project, path: PathBuf, demo: bool, ctx: &Context) {
        self.project = p;
        self.show_devices = !self.project.points.is_empty();
        self.source_details = None;
        self.path = path;
        self.demo = demo;
        self.document = 0;
        self.page = 0;
        self.selected_point = None;
        self.selected_reading = None;
        self.confirm_delete = None;
        self.traverse_index = 0;
        self.traverse_view = Default::default();
        self.search.clear();
        self.point_search.clear();
        self.reading_picker_search.clear();
        self.unit_filter = "All units".into();
        self.dirty = false;
        self.zoom = 1.;
        self.pan = Vec2::ZERO;
        self.request_page(ctx);
    }
    fn request_page(&mut self, ctx: &Context) {
        self.texture = None;
        self.page_request += 1;
        self.zoom = 1.;
        self.pan = Vec2::ZERO;
        let Some(doc) = self.project.documents.get(self.document) else {
            self.page_loading = false;
            return;
        };
        if doc.path.is_empty() {
            self.page_loading = false;
            return;
        }
        let path = PathBuf::from(&doc.path);
        let page = self.page;
        let request = self.page_request;
        let tx = self.tx.clone();
        let context = ctx.clone();
        self.page_loading = true;
        std::thread::spawn(move || {
            let result = pdf::PdfDocument::open(&path)
                .and_then(|d| d.render(page, 3200))
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Page(request, result));
            context.request_repaint();
        });
    }
    fn import_pdf(&mut self, path: PathBuf, ctx: &Context) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.message("Importing PDF and inspecting its pages…", false);
        let mut project = self.project.clone();
        let tx = self.tx.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let result = project
                .import_document(&path)
                .and_then(|id| {
                    project
                        .documents
                        .into_iter()
                        .find(|d| d.id == id)
                        .ok_or_else(|| anyhow::anyhow!("Imported document was not found"))
                })
                .map_err(|e| e.to_string());
            let _ = tx.send(Event::Document(result));
            context.request_repaint();
        });
    }
    fn probe(&mut self, ctx: &Context) {
        if self.port.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.message(
            format!("Reading device information on {}…", self.port),
            false,
        );
        let port = self.port.clone();
        let tx = self.tx.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Event::Probe(usb::probe(&port).map_err(|e| e.to_string())));
            context.request_repaint();
        });
    }
    fn download(&mut self, ctx: &Context) {
        if self.busy || self.port.is_empty() {
            return;
        }
        self.busy = true;
        self.message("Downloading stored records over USB…", false);
        let port = self.port.clone();
        let tx = self.tx.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = context.clone();
            let result = usb::download(&port, move |m| {
                let _ = progress_tx.send(Event::Progress(m));
                progress_ctx.request_repaint();
            })
            .map_err(|e| e.to_string());
            let _ = tx.send(Event::Download(result));
            context.request_repaint();
        });
    }
    fn receive(&mut self, ctx: &Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Progress(m) => self.message(m, false),
                Event::Probe(r) => {
                    self.busy = false;
                    match r {
                        Ok(device) => {
                            self.message(
                                format!(
                                    "Connected to {} · {} · {}",
                                    device.model, device.serial, device.firmware
                                ),
                                false,
                            );
                            self.device = Some(device);
                        }
                        Err(e) => {
                            self.device = None;
                            self.message(e, true);
                        }
                    }
                }
                Event::Download(r) => {
                    self.busy = false;
                    match r {
                        Ok(result) => {
                            let complete = result.complete;
                            let logs = data_dir()
                                .join("projects")
                                .join(&self.project.id)
                                .join("usb-captures");
                            let log_path = logs.join(format!(
                                "{}-{}.raw.ndjson",
                                chrono::Local::now().format("%Y%m%d-%H%M%S"),
                                uuid::Uuid::new_v4()
                            ));
                            let log_error = std::fs::create_dir_all(&logs)
                                .and_then(|_| std::fs::write(&log_path, &result.raw_log))
                                .err();
                            let capture_failed = log_error.is_some();
                            let warning = result.warnings.join(" · ");
                            self.device = Some(result.device);
                            match self.project.merge_readings(result.readings) {
                                Ok(n) => {
                                    self.changed();
                                    self.message(
                                        format!(
                                            "{} Imported {n} new measurements. {}{}",
                                            if complete {
                                                "Download complete."
                                            } else {
                                                "Download incomplete; partial data retained."
                                            },
                                            warning,
                                            log_error
                                                .map(|e| format!(
                                                    " Raw capture could not be saved: {e}"
                                                ))
                                                .unwrap_or_default()
                                        ),
                                        !complete || !warning.is_empty() || capture_failed,
                                    );
                                    self.view = View::Readings;
                                }
                                Err(e) => self.message(e.to_string(), true),
                            }
                        }
                        Err(e) => self.message(e, true),
                    }
                }
                Event::Document(r) => {
                    self.busy = false;
                    match r {
                        Ok(doc) => {
                            self.show_devices = false;
                            self.project.documents.push(doc);
                            self.document = self.project.documents.len() - 1;
                            self.page = 0;
                            self.changed();
                            self.view = View::Blueprint;
                            self.message("Blueprint imported. Choose Place point to map an air device or valve.",false);
                            self.request_page(ctx);
                        }
                        Err(e) => self.message(e, true),
                    }
                }
                Event::Page(id, r) => {
                    if id == self.page_request {
                        self.page_loading = false;
                        match r {
                            Ok((w, h, rgba)) => {
                                self.texture = Some(ctx.load_texture(
                                    format!("page-{id}"),
                                    ColorImage::from_rgba_unmultiplied([w, h], &rgba),
                                    TextureOptions::LINEAR,
                                ));
                            }
                            Err(e) => self.message(format!("PDF rendering failed: {e}"), true),
                        }
                    }
                }
            }
        }
    }
    fn header(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        TopBottomPanel::top("header")
            .exact_height(70.)
            .frame(
                Frame::new()
                    .fill(colors.panel)
                    .inner_margin(Margin::symmetric(22, 12)),
            )
            .show(ctx, |ui| {
                let compact = ui.available_width() < 1120.;
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(34., 34.), Sense::hover());
                    ui.painter().rect_filled(r, 9, colors.action);
                    for (x, y) in [(0.3, 0.3), (0.7, 0.3), (0.5, 0.7)] {
                        ui.painter().circle_filled(
                            pos2(r.left() + r.width() * x, r.top() + r.height() * y),
                            3.,
                            Color32::WHITE,
                        );
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("GREEN SUITE")
                                .font(serif_bold(21.))
                                .color(colors.ink),
                        );
                        ui.label(
                            RichText::new("MEASUREMENT WORKSPACE")
                                .size(11.)
                                .color(colors.muted),
                        );
                    });
                    ui.add_space(if compact { 10. } else { 30. });
                    for (v, label) in [
                        (
                            View::Blueprint,
                            if compact {
                                "Blueprint"
                            } else {
                                "Blueprint map"
                            },
                        ),
                        (
                            View::Readings,
                            if compact {
                                "Readings"
                            } else {
                                "Reading explorer"
                            },
                        ),
                        (
                            View::Traverse,
                            if compact { "Traverse" } else { "Traverse lab" },
                        ),
                    ] {
                        if ui
                            .selectable_label(self.view == v, RichText::new(label).size(14.))
                            .clicked()
                        {
                            self.view = v;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("About").clicked() {
                            self.show_about = true;
                        }
                        if ui
                            .button(if self.dark_mode {
                                "Light mode"
                            } else {
                                "Dark mode"
                            })
                            .clicked()
                        {
                            self.dark_mode = !self.dark_mode;
                            theme(ctx, self.dark_mode);
                            let _ = std::fs::create_dir_all(data_dir());
                            let _ = std::fs::write(
                                data_dir().join("appearance.txt"),
                                if self.dark_mode { "dark" } else { "light" },
                            );
                        }
                        if ui
                            .button(if compact { "Save" } else { "Save project" })
                            .on_hover_text("Save project · Ctrl+S")
                            .clicked()
                        {
                            self.save();
                        }
                        let contains_examples = self.demo
                            || self.project.readings.iter().any(|r| r.source == "example")
                            || self.project.documents.iter().any(|d| d.path.is_empty());
                        if !compact || contains_examples {
                            ui.label(
                                RichText::new(if contains_examples {
                                    if compact { "EXAMPLE" } else { "EXAMPLE DATA" }
                                } else {
                                    "LOCAL · USB ONLY"
                                })
                                .size(11.)
                                .color(if contains_examples {
                                    colors.warning
                                } else {
                                    colors.green
                                }),
                            );
                        }
                    });
                });
            });
    }
    fn sidebar(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        SidePanel::left("project-sidebar")
            .exact_width(238.)
            .resizable(false)
            .frame(Frame::new().fill(colors.paper).inner_margin(16))
            .show(ctx, |ui| {
                ui.spacing_mut().button_padding = vec2(9., 5.);
                let instrument_height = if self.device.is_some() { 240. } else { 210. };
                ScrollArea::vertical()
                    .id_salt("project-navigation")
                    .scroll_bar_visibility(scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .max_height((ui.available_height() - instrument_height).max(120.))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        eyebrow(ui, "PROJECT");
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    TextEdit::singleline(&mut self.project.name)
                                        .font(serif_bold(18.))
                                        .desired_width((ui.available_width() - 54.).max(90.)),
                                )
                                .on_hover_text(&self.project.name)
                                .changed()
                            {
                                self.changed();
                            }
                            ui.menu_button("File", |ui| {
                                if ui.add_enabled(!self.busy, Button::new("New")).clicked()
                                    && self.save()
                                {
                                    let p = Project::new("Untitled survey");
                                    let path = default_path(&p);
                                    self.replace_project(p, path, false, ctx);
                                    self.changed();
                                    ui.close();
                                }
                                if ui.add_enabled(!self.busy, Button::new("Open…")).clicked() {
                                    ui.close();
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("Green Suite project", &["json"])
                                        .pick_file()
                                    {
                                        if self.save() {
                                            match Project::load(&path) {
                                                Ok(p) => self.replace_project(p, path, false, ctx),
                                                Err(e) => self.message(e.to_string(), true),
                                            }
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
                            });
                        });
                        ui.add_space(6.);
                        ui.separator();
                        ui.add_space(4.);
                        if ui
                            .add_enabled(
                                !self.busy,
                                Button::new("+ Import PDF blueprint")
                                    .min_size(vec2(ui.available_width(), 32.)),
                            )
                            .clicked()
                        {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Mechanical blueprint", &["pdf"])
                                .pick_file()
                            {
                                self.import_pdf(path, ctx);
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.selectable_value(
                                &mut self.show_devices,
                                false,
                                format!("Drawings {}", self.project.documents.len()),
                            );
                            ui.selectable_value(
                                &mut self.show_devices,
                                true,
                                format!("Devices {}", self.project.points.len()),
                            );
                        });
                        if !self.show_devices {
                            let mut selected = None;
                            for (i, d) in self.project.documents.iter().enumerate() {
                                if ui
                                    .add(
                                        Button::selectable(
                                            self.document == i,
                                            format!("PDF  {}", d.name),
                                        )
                                        .wrap_mode(TextWrapMode::Wrap),
                                    )
                                    .on_hover_text(format!("{} · {} pages", d.name, d.page_count))
                                    .clicked()
                                {
                                    selected = Some(i);
                                }
                            }
                            if self.project.documents.is_empty() {
                                ui.label(
                                    RichText::new("Import a PDF to begin mapping measurements.")
                                        .size(12.)
                                        .color(colors.muted),
                                );
                            }
                            if let Some(i) = selected {
                                self.document = i;
                                self.page = 0;
                                self.view = View::Blueprint;
                                self.request_page(ctx);
                            }
                        }
                        if self.show_devices {
                            ui.add(
                                TextEdit::singleline(&mut self.point_search)
                                    .hint_text("Find device or system…")
                                    .desired_width(ui.available_width()),
                            );
                            let needle = self.point_search.to_lowercase();
                            let mut chosen = None;
                            for p in &self.project.points {
                                if !format!("{} {} {}", p.label, p.system, p.kind)
                                    .to_lowercase()
                                    .contains(&needle)
                                {
                                    continue;
                                }
                                let mark = if p.reading_ids.is_empty() {
                                    "–"
                                } else {
                                    "•"
                                };
                                if ui
                                    .selectable_label(
                                        self.selected_point.as_ref() == Some(&p.id),
                                        format!("{mark}  {}", p.label),
                                    )
                                    .on_hover_text(format!(
                                        "{} · {} · {} attached readings",
                                        p.kind,
                                        p.system,
                                        p.reading_ids.len()
                                    ))
                                    .clicked()
                                {
                                    chosen = Some((p.id.clone(), p.document_id.clone(), p.page));
                                }
                            }
                            if let Some((id, doc, page)) = chosen {
                                self.selected_point = Some(id);
                                self.view = View::Blueprint;
                                if let Some(i) =
                                    self.project.documents.iter().position(|d| d.id == doc)
                                {
                                    if i != self.document || page != self.page {
                                        self.document = i;
                                        self.page = page;
                                        self.request_page(ctx);
                                    }
                                }
                            }
                            if self.project.points.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "Place devices on a drawing to associate measurements.",
                                    )
                                    .size(12.)
                                    .color(colors.muted),
                                );
                            }
                        }
                    });
                ui.add_space(6.);
                ui.separator();
                ui.add_space(6.);
                eyebrow(ui, "USB INSTRUMENT");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.busy, |ui| {
                        ComboBox::from_id_salt("ports")
                            .width(120.)
                            .selected_text(if self.port.is_empty() {
                                "Choose port"
                            } else {
                                &self.port
                            })
                            .show_ui(ui, |ui| {
                                for p in &self.ports {
                                    if ui.selectable_value(&mut self.port, p.clone(), p).changed() {
                                        self.device = None;
                                    }
                                }
                            });
                    });
                    if ui.add_enabled(!self.busy, Button::new("Refresh")).clicked() {
                        match usb::list_ports() {
                            Ok(p) => {
                                self.ports = p;
                                if !self.ports.contains(&self.port) {
                                    self.port = self.ports.first().cloned().unwrap_or_default();
                                    self.device = None;
                                }
                            }
                            Err(e) => self.message(e.to_string(), true),
                        }
                    }
                });
                if let Some(d) = &self.device {
                    ui.label(
                        RichText::new(format!("Connected · {}", d.model))
                            .color(colors.green)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("{} · {} records", d.serial, d.record_count))
                            .size(11.)
                            .color(colors.muted),
                    )
                    .on_hover_text(format!("{} · {} groups", d.firmware, d.group_count));
                } else {
                    ui.label(
                        RichText::new("Wrist Reporter · USB serial")
                            .size(11.)
                            .color(colors.muted),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.busy && !self.port.is_empty(), Button::new("Connect"))
                        .clicked()
                    {
                        self.probe(ctx);
                    }
                    if ui
                        .add_enabled(
                            !self.busy && self.device.as_ref().is_some_and(|d| d.port == self.port),
                            Button::new(RichText::new("Download").color(Color32::WHITE))
                                .fill(colors.action)
                                .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        self.download(ctx);
                    }
                });
                ui.label(
                    RichText::new("Stored readings are retained on the instrument.")
                        .size(11.)
                        .color(colors.muted),
                );
                ui.add_space(4.);
                if ui
                    .add_enabled(!self.busy, Button::new("Open example project").small())
                    .clicked()
                    && self.save()
                {
                    let p = example_project();
                    let path = default_path(&p);
                    self.replace_project(p, path, true, ctx);
                    self.selected_point = self.project.points.first().map(|p| p.id.clone());
                }
            });
    }
    fn inspector(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        SidePanel::right("inspector").default_width(285.).width_range(255.0..=400.0).frame(Frame::new().fill(colors.panel).inner_margin(18)).show(ctx,|ui|{
            ScrollArea::vertical().id_salt("device-inspector-scroll").auto_shrink([false,false]).show(ui,|ui|{
            ui.add_space(8.);eyebrow(ui,"DEVICE INSPECTOR");
            let Some(index)=self.selected_point.as_ref().and_then(|id|self.project.points.iter().position(|p|&p.id==id)) else{
                ui.add_space(16.);ui.heading("Every reading, in place.");ui.add_space(10.);ui.label(RichText::new("Select an air device, duct traverse, valve, or pipe point on the drawing to review and attach measurements.").color(colors.muted));
                ui.add_space(18.);ui.label("1  Import a mechanical PDF\n\n2  Place and name a device\n\n3  Download or import readings\n\n4  Attach a reading to that device");return;
            };
            let mut dirty=false;let p=&mut self.project.points[index];
            dirty|=ui.add(TextEdit::singleline(&mut p.label).font(TextStyle::Heading).desired_width(f32::INFINITY)).changed();
            ui.add_space(10.);
            ComboBox::from_label("Type").selected_text(&p.kind).show_ui(ui,|ui|{for kind in kinds(){dirty|=ui.selectable_value(&mut p.kind,kind.into(),kind).changed();}});
            ui.label(RichText::new("SYSTEM / EQUIPMENT").size(11.).color(colors.muted));dirty|=ui.text_edit_singleline(&mut p.system).changed();
            ui.add_space(10.);ui.horizontal(|ui|{ui.label("Design");dirty|=ui.add(DragValue::new(&mut p.design_value).range(0.0..=1e9).speed(1.)).changed();dirty|=ui.add(TextEdit::singleline(&mut p.unit).desired_width(65.)).changed();});
            ui.add_space(12.);ui.label(RichText::new("FIELD NOTES").size(11.).color(colors.muted));dirty|=ui.add(TextEdit::multiline(&mut p.notes).desired_rows(3).desired_width(f32::INFINITY).hint_text("Location, valve position, access notes…")).changed();
            let ids=p.reading_ids.clone();let design=p.design_value;let unit=p.unit.clone();
            ui.add_space(16.);ui.separator();ui.add_space(12.);eyebrow(ui,"ATTACHED MEASUREMENTS");
            let mut remove=None;
            ScrollArea::vertical().id_salt("attached").max_height(230.).show(ui,|ui|{for id in &ids{if let Some(r)=self.project.readings.iter().find(|r|&r.id==id){
                Frame::new().fill(colors.paper).corner_radius(6).inner_margin(9).show(ui,|ui|{ui.horizontal(|ui|{let value=format!("{} {}",r.value,r.unit);ui.add(Label::new(RichText::new(&value).size(20.).strong().color(colors.green)).truncate()).on_hover_text(&value);if ui.small_button("×").on_hover_text("Detach reading").clicked(){remove=Some(id.clone());}});ui.label(RichText::new(format!("{} · {}",r.kind,r.group)).size(11.).color(colors.muted));ui.label(RichText::new(&r.timestamp).size(11.).color(colors.muted));if design>0.&&r.unit.eq_ignore_ascii_case(&unit){ui.label(format!("{:.1}% of design",r.value/design*100.));}});ui.add_space(6.);
            }}});
            if let Some(id)=remove{self.project.points[index].reading_ids.retain(|i|i!=&id);dirty=true;}
            if ids.is_empty(){ui.label(RichText::new("No measurements attached.").color(colors.muted));}
            ui.add_space(10.);
            let selected=self.selected_reading.as_ref().and_then(|id|self.project.readings.iter().find(|r|&r.id==id));
            let text=selected.map(|r|format!("{} · {} {}",r.group,r.value,r.unit)).unwrap_or("Choose a downloaded reading".into());
            ui.add(TextEdit::singleline(&mut self.reading_picker_search).hint_text("Find a reading to attach…").desired_width(ui.available_width()));
            let needle=self.reading_picker_search.to_lowercase();
            let filtered:Vec<usize>=self.project.readings.iter().enumerate().filter_map(|(index,r)|{
                (needle.is_empty()||format!("{} {} {} {} {}",r.group,r.sensor,r.kind,r.value,r.unit).to_lowercase().contains(&needle)).then_some(index)
            }).collect();
            ComboBox::from_id_salt("attach-reading").width(ui.available_width()).wrap_mode(TextWrapMode::Truncate).selected_text(text).show_ui(ui,|ui|{
                if filtered.is_empty(){ui.label("No matching readings.");}
                ScrollArea::vertical().id_salt("attach-reading-choices").max_height(240.).show_rows(ui,28.,filtered.len(),|ui,rows|{
                    for index in rows {
                        let r=&self.project.readings[filtered[index]];
                        let selected=self.selected_reading.as_ref()==Some(&r.id);
                        let label=format!("{} · {} · {} {}",r.group,r.kind,r.value,r.unit);
                        if ui.add_sized([ui.available_width(),28.],Button::selectable(selected,&label).wrap_mode(TextWrapMode::Truncate)).on_hover_text(&label).clicked(){self.selected_reading=Some(r.id.clone());}
                    }
                });
            });
            if ui.add_enabled(self.selected_reading.is_some(),Button::new("Attach selected reading").min_size(vec2(ui.available_width(),30.))).clicked(){if let Some(id)=&self.selected_reading{if !self.project.points[index].reading_ids.contains(id){self.project.points[index].reading_ids.push(id.clone());dirty=true;}}}
            ui.add_space(12.);ui.collapsing("Enter a manual measurement",|ui|{ui.add(DragValue::new(&mut self.manual_value).speed(1.));ui.label(format!("Unit: {}",self.project.points[index].unit));if ui.button("Record and attach").clicked(){let p=&mut self.project.points[index];let id=uuid::Uuid::new_v4().to_string();self.project.readings.push(Reading{id:id.clone(),timestamp:chrono::Local::now().to_rfc3339(),group:p.label.clone(),sensor:"Manual entry".into(),kind:p.kind.clone(),value:self.manual_value,unit:p.unit.clone(),source:"manual".into(),raw:String::new()});p.reading_ids.push(id);dirty=true;}});
            ui.add_space(15.);if ui.small_button("Delete mapped device…").clicked(){self.confirm_delete=Some(self.project.points[index].id.clone());}
            if dirty{self.changed();}
            });
        });
    }
    fn blueprint(&mut self, ui: &mut Ui, ctx: &Context) {
        let colors = palette(ctx);
        ui.horizontal(|ui| {
            ui.heading("Blueprint map");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let name = self
                    .project
                    .documents
                    .get(self.document)
                    .map(|d| d.name.as_str())
                    .unwrap_or("DRAWING + MEASUREMENTS");
                ui.add(Label::new(RichText::new(name).size(11.).color(colors.muted)).truncate())
                    .on_hover_text(name);
            });
        });
        ui.add_space(10.);
        if self.project.documents.is_empty() {
            Frame::new().fill(colors.panel).stroke(Stroke::new(1.,colors.line)).corner_radius(12).inner_margin(40).show(ui,|ui|{
                ui.set_min_height((ui.available_height()-100.).max(250.));ui.vertical_centered(|ui|{ui.add_space(65.);ui.label(RichText::new("Start with the drawing.").size(31.).strong().color(colors.ink));ui.add_space(14.);ui.label(RichText::new("Import a mechanical blueprint and put each measurement where it belongs.").size(15.).color(colors.muted));ui.add_space(25.);if ui.add_enabled(!self.busy,Button::new(RichText::new("Import PDF blueprint").color(Color32::WHITE)).fill(colors.action).min_size(vec2(210.,45.))).clicked(){if let Some(path)=rfd::FileDialog::new().add_filter("PDF blueprint",&["pdf"]).pick_file(){self.import_pdf(path,ctx);}}ui.add_space(24.);ui.label("Air devices     ·     Duct traverses     ·     Hydronic valves     ·     Pipe measurements");ui.add_space(40.);ui.label(RichText::new("PDFs, device data, and project files stay on this computer.").size(12.).color(colors.muted));});
            });
            return;
        }
        let doc = self.project.documents[self.document].clone();
        let can_place = !self.page_loading && (self.texture.is_some() || doc.path.is_empty());
        let mut new_page = self.page;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(!self.add_point, "Select / pan")
                .clicked()
            {
                self.add_point = false;
            }
            if ui
                .add_enabled(
                    can_place,
                    Button::selectable(self.add_point, "+ Place point"),
                )
                .clicked()
            {
                self.add_point = true;
            }
            ComboBox::from_id_salt("point-kind")
                .width(110.)
                .selected_text(&self.point_kind)
                .show_ui(ui, |ui| {
                    for k in kinds() {
                        ui.selectable_value(&mut self.point_kind, k.into(), k);
                    }
                });
            ui.separator();
            if ui.small_button("−").clicked() {
                self.zoom = (self.zoom / 1.2).max(0.25);
            }
            ui.label(format!("{:.0}%", self.zoom * 100.));
            if ui.small_button("+").clicked() {
                self.zoom = (self.zoom * 1.2).min(8.);
            }
            if ui.small_button("Fit").clicked() {
                self.zoom = 1.;
                self.pan = Vec2::ZERO;
            }
            ui.separator();
            if ui.add_enabled(self.page > 0, Button::new("‹")).clicked() {
                new_page = self.page - 1;
            }
            ui.label(format!("Sheet {} / {}", self.page + 1, doc.page_count));
            if ui
                .add_enabled(self.page + 1 < doc.page_count, Button::new("›"))
                .clicked()
            {
                new_page = self.page + 1;
            }
        });
        if new_page != self.page {
            self.page = new_page;
            self.request_page(ctx);
        }
        let drawing_ready = !self.page_loading && (self.texture.is_some() || doc.path.is_empty());
        if !drawing_ready {
            self.dragging_point = None;
        }
        ui.add_space(10.);
        let size = vec2(
            ui.available_width(),
            (ui.available_height() - 48.).max(180.),
        );
        let (canvas, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(canvas);
        painter.rect_filled(canvas, 8, colors.canvas);
        for x in (0..(canvas.width() / 24.) as i32).map(|i| canvas.left() + i as f32 * 24.) {
            for y in (0..(canvas.height() / 24.) as i32).map(|i| canvas.top() + i as f32 * 24.) {
                painter.circle_filled(pos2(x, y), 0.8, colors.grid);
            }
        }
        let aspect = self
            .texture
            .as_ref()
            .map(|t| t.size_vec2().x / t.size_vec2().y)
            .unwrap_or(1.42);
        let fit = (canvas.width() - 50.).min((canvas.height() - 50.) * aspect);
        let page_size = vec2(fit, fit / aspect) * self.zoom;
        let sheet = Rect::from_center_size(canvas.center() + self.pan, page_size);
        if self.page_loading {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Rendering blueprint…",
                FontId::proportional(18.),
                colors.muted,
            );
        } else if let Some(t) = &self.texture {
            painter.rect_filled(sheet.expand(3.), 0, Color32::from_black_alpha(12));
            painter.image(
                t.id(),
                sheet,
                Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                Color32::WHITE,
            );
        } else if doc.path.is_empty() {
            draw_example(&painter, sheet);
        } else {
            painter.text(
                canvas.center(),
                Align2::CENTER_CENTER,
                "Blueprint unavailable\nReopen the drawing or import an accessible PDF.",
                FontId::proportional(17.),
                colors.muted,
            );
        }
        let mut hit = None;
        for p in self
            .project
            .points
            .iter()
            .filter(|p| drawing_ready && p.document_id == doc.id && p.page == self.page)
        {
            let at = pos2(
                sheet.left() + p.x * sheet.width(),
                sheet.top() + p.y * sheet.height(),
            );
            if !canvas.contains(at) {
                continue;
            }
            let selected = self.selected_point.as_ref() == Some(&p.id);
            let color = if p.kind.contains("Valve") || p.kind.contains("Pipe") {
                Color32::from_rgb(45, 115, 171)
            } else {
                GREEN
            };
            if selected {
                painter.circle_stroke(at, 16., Stroke::new(2., color));
            }
            painter.circle_filled(at, 10., Color32::WHITE);
            painter.circle_filled(at, 7., color);
            if !p.reading_ids.is_empty() {
                painter.circle_filled(at + vec2(7., -7.), 3.5, Color32::from_rgb(232, 167, 52));
            }
            let galley = painter.layout_no_wrap(p.label.clone(), FontId::proportional(11.), INK);
            let label = Rect::from_min_size(at + vec2(13., -8.), galley.size() + vec2(8., 5.));
            painter.rect_filled(label, 3, Color32::from_white_alpha(235));
            painter.galley(label.min + vec2(4., 2.), galley, INK);
            if response
                .interact_pointer_pos()
                .is_some_and(|pos| pos.distance(at) < 18.)
            {
                hit = Some(p.id.clone());
            }
        }
        if drawing_ready && response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.25, 8.);
                if let Some(pointer) = response.hover_pos() {
                    let anchor = pointer - canvas.center();
                    self.pan = (self.pan - anchor) * (self.zoom / old) + anchor;
                }
            }
        }
        if response.drag_started() {
            self.dragging_point = if drawing_ready && !self.add_point {
                hit.clone()
            } else {
                None
            };
            if let Some(id) = &self.dragging_point {
                self.selected_point = Some(id.clone());
            }
        }
        if drawing_ready && response.dragged() {
            if let Some(id) = &self.dragging_point {
                if let Some(pos) = response.interact_pointer_pos() {
                    if let Some(p) = self.project.points.iter_mut().find(|p| &p.id == id) {
                        p.x = ((pos.x - sheet.left()) / sheet.width()).clamp(0., 1.);
                        p.y = ((pos.y - sheet.top()) / sheet.height()).clamp(0., 1.);
                        self.dirty = true;
                    }
                }
            } else if !self.add_point {
                self.pan += ui.input(|i| i.pointer.delta());
            }
        }
        if response.drag_stopped() {
            self.dragging_point = None;
        }
        if response.clicked() {
            if let Some(id) = hit {
                self.selected_point = Some(id);
            } else if self.add_point && drawing_ready {
                if let Some(pos) = response
                    .interact_pointer_pos()
                    .filter(|p| sheet.contains(*p))
                {
                    let id = uuid::Uuid::new_v4().to_string();
                    let prefix = if self.point_kind.contains("Valve") {
                        "V"
                    } else if self.point_kind.contains("Pipe") {
                        "P"
                    } else if self.point_kind.to_ascii_lowercase().contains("traverse") {
                        "TR"
                    } else {
                        "AD"
                    };
                    self.project.points.push(MapPoint {
                        id: id.clone(),
                        label: format!("{prefix}-{:02}", self.project.points.len() + 1),
                        kind: self.point_kind.clone(),
                        system: String::new(),
                        document_id: doc.id.clone(),
                        page: self.page,
                        x: (pos.x - sheet.left()) / sheet.width(),
                        y: (pos.y - sheet.top()) / sheet.height(),
                        design_value: 0.,
                        unit: if prefix == "V" || prefix == "P" {
                            "GPM"
                        } else {
                            "CFM"
                        }
                        .into(),
                        notes: String::new(),
                        reading_ids: vec![],
                    });
                    self.selected_point = Some(id);
                    self.show_devices = true;
                    self.changed();
                }
            }
        }
        ui.add_space(9.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if !drawing_ready {
                    "Point placement is available when the drawing is visible."
                } else if self.add_point {
                    "Click the drawing to place a device."
                } else {
                    "Drag the drawing to pan · Scroll to zoom · Drag a point to reposition"
                })
                .size(11.)
                .color(colors.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "{} readings · {} devices",
                        self.project.readings.len(),
                        self.project.points.len()
                    ))
                    .size(11.)
                    .color(colors.green),
                );
            });
        });
    }
    fn readings(&mut self, ui: &mut Ui) {
        let colors = palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.heading("Reading explorer");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Export CSV…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name("Green Suite readings.csv")
                        .save_file()
                    {
                        match self.project.export_csv(&path) {
                            Ok(()) => self.message("Measurements exported.", false),
                            Err(e) => self.message(e.to_string(), true),
                        }
                    }
                }
                if ui.button("Import CSV…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Measurements", &["csv"])
                        .pick_file()
                    {
                        match self.project.import_csv(&path) {
                            Ok(n) => {
                                self.changed();
                                self.message(format!("Imported {n} measurements."), false)
                            }
                            Err(e) => self.message(e.to_string(), true),
                        }
                    }
                }
            });
        });
        ui.add_space(10.);
        ui.label(RichText::new("Explore stored measurements, inspect their source, and attach them to a mapped device.").color(colors.muted));
        ui.add_space(16.);
        let mut mapped_labels: std::collections::HashMap<&str, Vec<&str>> =
            std::collections::HashMap::new();
        for point in &self.project.points {
            for id in &point.reading_ids {
                mapped_labels
                    .entry(id.as_str())
                    .or_default()
                    .push(point.label.as_str());
            }
        }
        ui.horizontal(|ui| {
            stat(ui, "MEASUREMENTS", &self.project.readings.len().to_string());
            stat(ui, "MAPPED DEVICES", &self.project.points.len().to_string());
            stat(ui, "ASSIGNED", &mapped_labels.len().to_string());
        });
        ui.add_space(16.);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.search)
                    .hint_text("Search group, sensor, value, or date…")
                    .desired_width(330.),
            );
            let units: std::collections::BTreeSet<&str> = self
                .project
                .readings
                .iter()
                .map(|r| r.unit.as_str())
                .collect();
            ComboBox::from_id_salt("unit-filter")
                .selected_text(&self.unit_filter)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.unit_filter, "All units".into(), "All units");
                    for u in units {
                        ui.selectable_value(&mut self.unit_filter, u.to_string(), u);
                    }
                });
        });
        ui.add_space(12.);
        let needle = self.search.to_lowercase();
        let mut chosen = None;
        let filtered: Vec<usize> = self
            .project
            .readings
            .iter()
            .enumerate()
            .filter_map(|(index, reading)| {
                if self.unit_filter != "All units" && reading.unit != self.unit_filter {
                    return None;
                }
                if !needle.is_empty()
                    && !format!(
                        "{} {} {} {} {}",
                        reading.group,
                        reading.sensor,
                        reading.kind,
                        reading.value,
                        reading.timestamp
                    )
                    .to_lowercase()
                    .contains(&needle)
                {
                    return None;
                }
                Some(index)
            })
            .collect();
        ui.label(
            RichText::new(format!("{} matching measurements", filtered.len()))
                .size(11.)
                .color(colors.muted),
        );
        Frame::new()
            .fill(colors.panel)
            .stroke(Stroke::new(1., colors.line))
            .corner_radius(7)
            .inner_margin(12)
            .show(ui, |ui| {
                if filtered.is_empty() {
                    ui.label("No matching measurements. Download from USB or import a CSV.");
                }
                const ROW_HEIGHT: f32 = 32.;
                let widths = [230., 128., 72., 205., 200., 150.];
                let table_width: f32 = widths.iter().sum();
                ScrollArea::both()
                    .id_salt("reading-table")
                    .max_height(
                        (ui.available_height()
                            - if self.selected_reading.is_some() {
                                82.
                            } else {
                                20.
                            })
                        .max(70.),
                    )
                    .show_rows(ui, ROW_HEIGHT, filtered.len() + 1, |ui, visible_rows| {
                        ui.set_min_width(table_width);
                        for row_index in visible_rows {
                            let heading = row_index == 0;
                            let reading = if heading {
                                None
                            } else {
                                Some(&self.project.readings[filtered[row_index - 1]])
                            };
                            let selected = reading.is_some_and(|reading| {
                                self.selected_reading.as_ref() == Some(&reading.id)
                            });
                            let (row_rect, row_response) = ui.allocate_exact_size(
                                vec2(table_width, ROW_HEIGHT),
                                if heading {
                                    Sense::hover()
                                } else {
                                    Sense::click()
                                },
                            );
                            let color = if selected {
                                colors.selection
                            } else if heading {
                                colors.paper
                            } else if row_index % 2 == 0 {
                                colors.alternate
                            } else {
                                colors.panel
                            };
                            ui.painter().rect_filled(row_rect, 3, color);
                            if row_response.has_focus() {
                                ui.painter().rect_stroke(
                                    row_rect.shrink(1.),
                                    3,
                                    Stroke::new(2., colors.green),
                                    StrokeKind::Inside,
                                );
                            }
                            if let Some(reading) = reading {
                                row_response.widget_info(|| {
                                    WidgetInfo::selected(
                                        WidgetType::Button,
                                        true,
                                        selected,
                                        format!(
                                            "{} / {}: {} {}",
                                            reading.group,
                                            reading.sensor,
                                            reading.value,
                                            reading.unit
                                        ),
                                    )
                                });
                            }
                            if row_response.clicked() {
                                chosen = reading.map(|reading| reading.id.clone());
                            }
                            let values: Vec<String> = if let Some(reading) = reading {
                                vec![
                                    format!("{} / {}", reading.group, reading.sensor),
                                    reading.value.to_string(),
                                    reading.unit.clone(),
                                    reading.timestamp.clone(),
                                    mapped_labels
                                        .get(reading.id.as_str())
                                        .map(|labels| labels.join(", "))
                                        .unwrap_or_else(|| "Unassigned".into()),
                                    reading.source.clone(),
                                ]
                            } else {
                                [
                                    "GROUP / SENSOR",
                                    "MEASUREMENT",
                                    "UNIT",
                                    "RECORDED",
                                    "MAPPED TO",
                                    "SOURCE",
                                ]
                                .iter()
                                .map(|text| text.to_string())
                                .collect()
                            };
                            let mut left = row_rect.left();
                            for (column, (text, width)) in values.iter().zip(widths).enumerate() {
                                let cell = Rect::from_min_size(
                                    pos2(left + 8., row_rect.top()),
                                    vec2(width - 16., ROW_HEIGHT),
                                );
                                let rich = RichText::new(text)
                                    .size(if heading { 10. } else { 13. })
                                    .color(if heading {
                                        colors.muted
                                    } else if column == 1 {
                                        colors.green
                                    } else {
                                        colors.ink
                                    });
                                let label = ui.place(
                                    cell,
                                    Label::new(rich)
                                        .truncate()
                                        .halign(Align::Min)
                                        .sense(Sense::hover()),
                                );
                                label.on_hover_text(text);
                                left += width;
                            }
                        }
                    });
            });
        if let Some(id) = chosen {
            self.selected_reading = Some(id);
        }
        ui.add_space(12.);
        if let Some(r) = self
            .selected_reading
            .as_ref()
            .and_then(|id| self.project.readings.iter().find(|r| &r.id == id))
            .cloned()
        {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{}  ·  {} {}", r.kind, r.value, r.unit)).strong());
                let target = self
                    .selected_point
                    .as_ref()
                    .and_then(|id| self.project.points.iter().find(|p| &p.id == id))
                    .map(|p| p.label.clone());
                if ui
                    .add_enabled(
                        target.is_some(),
                        Button::new(format!(
                            "Attach to {}",
                            target.as_deref().unwrap_or("a mapped device")
                        )),
                    )
                    .clicked()
                {
                    if let Some(p) = self
                        .project
                        .points
                        .iter_mut()
                        .find(|p| Some(&p.id) == self.selected_point.as_ref())
                    {
                        if !p.reading_ids.contains(&r.id) {
                            p.reading_ids.push(r.id.clone());
                            self.changed();
                            self.message("Reading attached to mapped device.", false);
                        }
                    }
                }
                if ui.button("Source record…").clicked() {
                    self.source_details = Some(r.clone());
                }
            });
        }
    }
    fn traverses(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Traverse lab");
            ui.add_space(15.);
            if ui.button("+ New traverse").clicked() {
                self.project.traverses.push(new_traverse());
                self.traverse_index = self.project.traverses.len() - 1;
                self.changed();
            }
            if !self.project.traverses.is_empty() {
                ComboBox::from_id_salt("traverse-list")
                    .selected_text(
                        &self.project.traverses
                            [self.traverse_index.min(self.project.traverses.len() - 1)]
                        .name,
                    )
                    .show_ui(ui, |ui| {
                        for (i, t) in self.project.traverses.iter().enumerate() {
                            ui.selectable_value(&mut self.traverse_index, i, &t.name);
                        }
                    });
            }
        });
        if self.project.traverses.is_empty() {
            ui.add_space(30.);
            ui.label(
                "Create a traverse to visualize velocity samples and calculate volumetric airflow.",
            );
            return;
        }
        self.traverse_index = self.traverse_index.min(self.project.traverses.len() - 1);
        let t = &mut self.project.traverses[self.traverse_index];
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Name");
            changed |= ui.text_edit_singleline(&mut t.name).changed();
            ui.label("Map to");
            let label = t
                .point_id
                .as_ref()
                .and_then(|id| self.project.points.iter().find(|p| &p.id == id))
                .map(|p| p.label.as_str())
                .unwrap_or("No device");
            ComboBox::from_id_salt("traverse-point")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut t.point_id, None, "No device")
                        .changed();
                    for p in &self.project.points {
                        changed |= ui
                            .selectable_value(&mut t.point_id, Some(p.id.clone()), &p.label)
                            .changed();
                    }
                });
        });
        if let Ok(result) = traverse::calculate(t) {
            if ui
                .add_enabled(
                    result.flow_cfm.is_some(),
                    Button::new("Save calculated flow as a reading"),
                )
                .clicked()
            {
                let id = uuid::Uuid::new_v4().to_string();
                self.project.readings.push(Reading {
                    id: id.clone(),
                    timestamp: chrono::Local::now().to_rfc3339(),
                    group: t.name.clone(),
                    sensor: "Traverse calculation".into(),
                    kind: "Airflow".into(),
                    value: result.flow_cfm.unwrap(),
                    unit: "CFM".into(),
                    source: "traverse".into(),
                    raw: serde_json::to_string(t).unwrap_or_default(),
                });
                if let Some(p) = self
                    .project
                    .points
                    .iter_mut()
                    .find(|p| Some(&p.id) == t.point_id.as_ref())
                {
                    p.reading_ids.push(id);
                }
                changed = true;
            }
        }
        ui.add_space(8.);
        changed |= self.traverse_view.ui(ui, t, &self.project.readings);
        if changed {
            self.changed();
        }
    }
}

impl eframe::App for GreenSuite {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let colors = palette(ctx);
        self.receive(ctx);
        if ctx.input(|i| i.viewport().close_requested())
            && (self.busy || (self.dirty && !self.save()))
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.busy {
                self.message("A USB transfer or PDF import is running. Please wait for it to finish before closing.",false);
            }
        }
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            if let Some(path) = file.path {
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
                {
                    self.import_pdf(path, ctx);
                    break;
                }
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::S)) {
            self.save();
        }
        self.header(ctx);
        TopBottomPanel::bottom("status")
            .exact_height(32.)
            .frame(
                Frame::new()
                    .fill(colors.panel)
                    .inner_margin(Margin::symmetric(18, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy || self.page_loading {
                        ui.spinner();
                    }
                    ui.add_sized(
                        vec2((ui.available_width() - 182.).max(80.), 18.),
                        Label::new(RichText::new(&self.status).size(11.).color(if self.error {
                            colors.warning
                        } else {
                            colors.muted
                        }))
                        .truncate(),
                    )
                    .on_hover_text(&self.status);
                    if ui
                        .small_button("Details…")
                        .on_hover_text("Read or copy the full activity message")
                        .clicked()
                    {
                        self.status_details = Some(self.status.clone());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(if self.dirty {
                                "Saving…"
                            } else {
                                "Saved locally"
                            })
                            .size(11.)
                            .color(colors.muted),
                        );
                    });
                });
            });
        self.sidebar(ctx);
        if self.view == View::Blueprint {
            self.inspector(ctx);
        }
        CentralPanel::default()
            .frame(Frame::new().fill(colors.paper).inner_margin(22))
            .show(ctx, |ui| match self.view {
                View::Blueprint => self.blueprint(ui, ctx),
                View::Readings => self.readings(ui),
                View::Traverse => self.traverses(ui),
            });
        if let Some(delete_id) = self.confirm_delete.clone() {
            let label = self
                .project
                .points
                .iter()
                .find(|p| p.id == delete_id)
                .map(|p| p.label.clone())
                .unwrap_or_default();
            Window::new("Delete mapped device?").collapsible(false).resizable(false).show(ctx,|ui|{
                ui.label(format!("Remove {label} and its map associations? Measurements remain in the project."));
                ui.horizontal(|ui|{
                    if ui.button("Delete device").clicked(){
                        remove_map_point(&mut self.project, &delete_id);
                        if self.selected_point.as_ref()==Some(&delete_id){self.selected_point=None;}
                        self.changed();self.confirm_delete=None;
                    }
                    if ui.button("Cancel").clicked(){self.confirm_delete=None;}
                });
            });
        }
        if self.show_about {
            Window::new("About Green Suite").open(&mut self.show_about).resizable(false).show(ctx,|ui|{ui.heading("Green Suite 0.1");ui.label("Native Rust measurement workspace · USB only");ui.label("Copyright © 2026 Kevin R. Whiteman. All rights reserved.");ui.label("Original Green Suite code is proprietary. Evergreen-owned materials\nand third-party libraries retain their respective rights and licenses.");ui.add_space(10.);ui.label("Import PDF drawings. Place devices. Attach stored measurements.\nBuild equal-area traverses and inspect animated velocity profiles.");ui.label("PDFs and projects are stored locally. No network service is used.");});
        }
        if let Some(message) = self.status_details.clone() {
            let mut open = true;
            Window::new("Activity details")
                .open(&mut open)
                .default_width(540.)
                .show(ctx, |ui| {
                    ScrollArea::vertical().max_height(330.).show(ui, |ui| {
                        ui.add(Label::new(&message).wrap());
                    });
                    if ui.button("Copy message").clicked() {
                        ctx.copy_text(message.clone());
                    }
                });
            if !open {
                self.status_details = None;
            }
        }
        if let Some(reading) = self.source_details.clone() {
            let mut open = true;
            Window::new("Source record")
                .open(&mut open)
                .default_width(540.)
                .show(ctx, |ui| {
                    ui.heading(format!("{} {}", reading.value, reading.unit));
                    ui.label(format!(
                        "{} · {} · {}",
                        reading.group, reading.sensor, reading.kind
                    ));
                    ui.label(format!("Recorded: {}", reading.timestamp));
                    ui.label(format!("Source: {}", reading.source));
                    ui.separator();
                    ScrollArea::vertical().max_height(260.).show(ui, |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(if reading.raw.is_empty() {
                                    "No raw source record."
                                } else {
                                    &reading.raw
                                })
                                .size(12.),
                            )
                            .wrap(),
                        );
                    });
                    if ui
                        .add_enabled(!reading.raw.is_empty(), Button::new("Copy source record"))
                        .clicked()
                    {
                        ctx.copy_text(reading.raw.clone());
                    }
                });
            if !open {
                self.source_details = None;
            }
        }
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(1) {
            self.save();
        }
        if let Some(path) = &self.screenshot {
            let drawable = ctx.input(|input| {
                let viewport = input.viewport();
                viewport.minimized != Some(true)
                    && viewport
                        .inner_rect
                        .is_some_and(|rect| rect.width() > 0. && rect.height() > 0.)
            });
            if !self.capture_requested
                && Instant::now() > self.capture_at
                && !self.page_loading
                && drawable
            {
                ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
                self.capture_requested = true;
            }
            let image = ctx.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = image {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                let _ = image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                );
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.dirty {
            self.save();
        }
    }
}

fn serif_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Serif Bold".into()))
}

fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "serif-semibold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/SourceSerif4-Semibold.ttf")).into(),
    );
    fonts.font_data.insert(
        "serif-bold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/SourceSerif4-Bold.ttf")).into(),
    );
    // Every text role, including source records and painter labels, uses serif faces.
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["serif-semibold".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["serif-semibold".into()]);
    fonts.families.insert(
        FontFamily::Name("Serif Bold".into()),
        vec!["serif-bold".into()],
    );
    ctx.set_fonts(fonts);
}

fn theme(ctx: &Context, dark: bool) {
    ctx.set_theme(if dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });
    let colors = Palette::new(dark);
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.panel_fill = colors.panel;
    style.visuals.window_fill = colors.panel;
    style.visuals.extreme_bg_color = colors.paper;
    style.visuals.faint_bg_color = colors.alternate;
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke = Stroke::new(1., colors.green);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.bg_fill = colors.alternate;
        widget.weak_bg_fill = colors.alternate;
        widget.bg_stroke = Stroke::new(1., colors.line);
        widget.fg_stroke = Stroke::new(1.4, colors.ink);
    }
    style.visuals.widgets.hovered.bg_fill = colors.selection;
    style.visuals.widgets.hovered.weak_bg_fill = colors.selection;
    style.visuals.widgets.active.bg_fill = colors.selection;
    style.visuals.widgets.active.weak_bg_fill = colors.selection;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1., colors.line);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1., colors.muted);
    style.spacing.item_spacing = vec2(8., 8.);
    style.spacing.button_padding = vec2(11., 7.);
    style.spacing.scroll = style::ScrollStyle::solid();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.));
    style.text_styles.insert(TextStyle::Button, serif_bold(13.));
    style
        .text_styles
        .insert(TextStyle::Heading, serif_bold(24.));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::proportional(12.));
    ctx.set_style(style);
}
fn eyebrow(ui: &mut Ui, s: &str) {
    let colors = palette(ui.ctx());
    ui.label(RichText::new(s).font(serif_bold(11.)).color(colors.muted));
    ui.add_space(5.);
}
fn stat(ui: &mut Ui, label: &str, value: &str) {
    let colors = palette(ui.ctx());
    Frame::new()
        .fill(colors.panel)
        .stroke(Stroke::new(1., colors.line))
        .corner_radius(7)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(155.);
            eyebrow(ui, label);
            ui.label(
                RichText::new(value)
                    .font(serif_bold(30.))
                    .color(colors.green),
            );
        });
}
fn default_path(p: &Project) -> PathBuf {
    data_dir()
        .join("projects")
        .join(&p.id)
        .join("project.green-suite.json")
}
fn kinds() -> [&'static str; 6] {
    [
        "Supply air",
        "Return air",
        "Exhaust air",
        "Duct traverse",
        "Valve",
        "Pipe",
    ]
}
fn remove_map_point(project: &mut Project, id: &str) {
    project.points.retain(|point| point.id != id);
    for traverse in &mut project.traverses {
        if traverse.point_id.as_deref() == Some(id) {
            traverse.point_id = None;
        }
    }
}
fn new_traverse() -> Traverse {
    Traverse {
        id: uuid::Uuid::new_v4().to_string(),
        name: "Duct traverse".into(),
        shape: "rectangular".into(),
        width: 24.,
        height: 18.,
        diameter: 18.,
        rows: 3,
        columns: 4,
        velocities: vec![None; 12],
        point_id: None,
    }
}

fn example_project() -> Project {
    let mut p = Project::new("North wing · example survey");
    p.documents.push(Document {
        id: "example-sheet".into(),
        name: "M-101 · Air & water distribution (example)".into(),
        path: String::new(),
        page_count: 1,
    });
    for (i, (x, y, label, kind, target, value, unit)) in [
        (0.25, 0.27, "SD-01", "Supply air", 240., 232., "CFM"),
        (0.49, 0.27, "SD-02", "Supply air", 240., 246., "CFM"),
        (0.74, 0.27, "SD-03", "Supply air", 180., 172., "CFM"),
        (0.25, 0.60, "SD-04", "Supply air", 220., 214., "CFM"),
        (0.49, 0.60, "SD-05", "Supply air", 220., 224., "CFM"),
        (0.78, 0.74, "BV-01", "Valve", 12., 11.6, "GPM"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("example-reading-{i}");
        p.readings.push(Reading {
            id: id.clone(),
            timestamp: "Example · 09:41".into(),
            group: label.into(),
            sensor: "Illustrative sample".into(),
            kind: if unit == "CFM" {
                "Airflow"
            } else {
                "Water flow"
            }
            .into(),
            value,
            unit: unit.into(),
            source: "example".into(),
            raw: "Example data; not an instrument measurement.".into(),
        });
        p.points.push(MapPoint {
            id: format!("example-point-{i}"),
            label: label.into(),
            kind: kind.into(),
            system: if unit == "CFM" { "AHU-01" } else { "CHW-01" }.into(),
            document_id: "example-sheet".into(),
            page: 0,
            x,
            y,
            design_value: target,
            unit: unit.into(),
            notes: "Illustrative device. Import your drawing to start a field project.".into(),
            reading_ids: vec![id],
        });
    }
    let mut t = new_traverse();
    t.name = "AHU-01 · Example traverse".into();
    t.velocities = [
        590., 725., 790., 610., 670., 850., 910., 720., 580., 730., 780., 620.,
    ]
    .iter()
    .map(|x| Some(*x))
    .collect();
    t.point_id = Some("example-point-0".into());
    p.traverses.push(t);
    p
}

fn draw_example(p: &Painter, r: Rect) {
    p.rect_filled(r, 0, Color32::WHITE);
    let at = |x: f32, y: f32| pos2(r.left() + x * r.width(), r.top() + y * r.height());
    let line = Color32::from_rgb(111, 134, 137);
    p.rect_stroke(
        Rect::from_min_max(at(0.07, 0.10), at(0.93, 0.86)),
        0,
        Stroke::new(1.5, line),
        StrokeKind::Inside,
    );
    for x in [0.37, 0.63] {
        p.line_segment([at(x, 0.10), at(x, 0.86)], Stroke::new(1., line));
    }
    p.line_segment([at(0.07, 0.43), at(0.93, 0.43)], Stroke::new(1., line));
    for (x, y, label) in [
        (0.11, 0.14, "OFFICE 101"),
        (0.41, 0.14, "MEETING 102"),
        (0.67, 0.14, "OFFICE 103"),
        (0.11, 0.48, "STUDIO 104"),
        (0.41, 0.48, "STUDIO 105"),
        (0.67, 0.48, "MECHANICAL"),
    ] {
        p.text(
            at(x, y),
            Align2::LEFT_TOP,
            label,
            FontId::proportional((r.width() * 0.014).clamp(8., 15.)),
            line,
        );
    }
    for d in [-0.006, 0.006] {
        p.line_segment(
            [at(0.12, 0.39 + d), at(0.86, 0.39 + d)],
            Stroke::new(1.3, GREEN),
        );
    }
    for x in [0.25, 0.49, 0.74] {
        p.line_segment([at(x, 0.39), at(x, 0.27)], Stroke::new(1.2, GREEN));
        let diff = Rect::from_center_size(at(x, 0.27), vec2(r.width() * 0.045, r.height() * 0.055));
        p.rect_stroke(diff, 0, Stroke::new(1., GREEN), StrokeKind::Inside);
        p.line_segment([diff.min, diff.max], Stroke::new(1., GREEN));
        p.line_segment(
            [diff.left_bottom(), diff.right_top()],
            Stroke::new(1., GREEN),
        );
    }
    for x in [0.25, 0.49] {
        p.line_segment([at(x, 0.39), at(x, 0.60)], Stroke::new(1.2, GREEN));
        let diff = Rect::from_center_size(at(x, 0.60), vec2(r.width() * 0.045, r.height() * 0.055));
        p.rect_stroke(diff, 0, Stroke::new(1., GREEN), StrokeKind::Inside);
    }
    let blue = Color32::from_rgb(66, 131, 170);
    p.line_segment([at(0.69, 0.66), at(0.88, 0.66)], Stroke::new(2., blue));
    p.line_segment([at(0.69, 0.74), at(0.88, 0.74)], Stroke::new(2., blue));
    p.circle_stroke(at(0.70, 0.70), r.height() * 0.04, Stroke::new(1.5, blue));
    p.text(
        at(0.07, 0.93),
        Align2::LEFT_CENTER,
        "M-101   MECHANICAL DISTRIBUTION",
        FontId::proportional((r.width() * 0.016).clamp(9., 17.)),
        INK,
    );
    p.text(
        at(0.93, 0.93),
        Align2::RIGHT_CENTER,
        "EXAMPLE · NOT TO SCALE",
        FontId::proportional((r.width() * 0.011).clamp(7., 12.)),
        MUTED,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_project_has_valid_links_and_identifiable_sample_data() {
        let project = example_project();
        project.validate().unwrap();
        assert!(!project.points.is_empty());
        assert!(!project.traverses.is_empty());
        assert!(
            project
                .documents
                .iter()
                .all(|document| document.path.is_empty())
        );
        assert!(
            project
                .readings
                .iter()
                .all(|reading| reading.source == "example")
        );
    }

    #[test]
    fn pending_deletion_removes_only_its_target_and_preserves_measurements() {
        let mut project = example_project();
        let pending_delete = project.points[0].id.clone();
        // The selection may change while the confirmation window is open.
        let selected_after_confirmation_opened = project.points[1].id.clone();
        let unaffected_point = serde_json::to_value(&project.points[1]).unwrap();
        let readings_before = serde_json::to_value(&project.readings).unwrap();
        let documents_before = serde_json::to_value(&project.documents).unwrap();
        let old_point_count = project.points.len();
        let mut other_traverse = new_traverse();
        other_traverse.point_id = Some(selected_after_confirmation_opened.clone());
        project.traverses.push(other_traverse);

        remove_map_point(&mut project, &pending_delete);

        assert_eq!(project.points.len(), old_point_count - 1);
        assert!(
            !project
                .points
                .iter()
                .any(|point| point.id == pending_delete)
        );
        let selected = project
            .points
            .iter()
            .find(|point| point.id == selected_after_confirmation_opened)
            .unwrap();
        assert_eq!(serde_json::to_value(selected).unwrap(), unaffected_point);
        assert!(project.traverses[0].point_id.is_none());
        assert_eq!(
            project.traverses[1].point_id.as_deref(),
            Some(selected_after_confirmation_opened.as_str())
        );
        assert_eq!(
            serde_json::to_value(&project.readings).unwrap(),
            readings_before
        );
        assert_eq!(
            serde_json::to_value(&project.documents).unwrap(),
            documents_before
        );
        project.validate().unwrap();
    }

    #[test]
    fn stale_deletion_target_cannot_remove_a_different_point() {
        let mut project = example_project();
        let before = serde_json::to_value(&project).unwrap();
        remove_map_point(&mut project, "point-from-another-project");
        assert_eq!(serde_json::to_value(&project).unwrap(), before);
    }
}
