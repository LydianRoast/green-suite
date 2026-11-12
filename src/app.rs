// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
mod shell;

use crate::{
    appearance::{self, Scheme, palette},
    live,
    model::*,
    pdf, traverse, usb,
};
use eframe::egui::{self, *};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant},
};

// Drawing annotations remain legible against unmodified white PDF sheets.
const GREEN: Color32 = Color32::from_rgb(1, 97, 59);
const INK: Color32 = Color32::from_rgb(15, 23, 40);
const MUTED: Color32 = Color32::from_rgb(81, 86, 90);

#[derive(Clone, Copy, PartialEq)]
enum View {
    Blueprint,
    Readings,
    Traverse,
    Live,
}
enum Event {
    Probe(Result<usb::DeviceInfo, String>),
    Download(Result<usb::DownloadResult, String>),
    Progress(String),
    Document(Result<Document, String>),
    Page(u64, Result<(usize, usize, Vec<u8>), String>),
    Live(live::LiveEvent),
    LiveStopped(Result<(), String>),
}

const LIVE_HISTORY_LIMIT: usize = 180;
const LIVE_FRESHNESS: Duration = Duration::from_secs(15);
const LIVE_RECOVERY: &str = "After stopping, turn sensors off, restart the Reporter, then turn sensors on to return to normal use.";

#[derive(Clone)]
struct LiveChannel {
    value: f64,
    display_value: String,
    unit: String,
    received: Instant,
    received_at: String,
    device_timestamp: Option<String>,
    raw: String,
    history: VecDeque<(Instant, f64)>,
}

impl LiveChannel {
    fn update(&mut self, value: &live::LiveValue, sample: &live::LiveSample, received: Instant) {
        if self.unit != value.unit {
            self.history.clear();
        }
        self.value = value.value;
        self.display_value = value.display_value.clone();
        self.unit = value.unit.clone();
        self.received = received;
        self.received_at = sample.received_at.clone();
        self.device_timestamp = sample.device_timestamp.clone();
        self.raw = sample.raw.clone();
        self.history.push_back((received, value.value));
        while self.history.len() > LIVE_HISTORY_LIMIT {
            self.history.pop_front();
        }
    }
    fn fresh(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.received) < LIVE_FRESHNESS
    }
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
    scheme: Scheme,
    live_running: bool,
    live_started_at: Option<Instant>,
    live_stop: Arc<AtomicBool>,
    live_updates: Option<Receiver<(live::LiveEvent, Instant)>>,
    live_dropped: Arc<AtomicUsize>,
    live_modules: Vec<live::ModuleInfo>,
    live_channels: BTreeMap<(u8, usize), LiveChannel>,
    live_session_changed: bool,
    live_failure: Option<String>,
    live_warning: Option<String>,
    live_raw_path: Option<PathBuf>,
    live_attach_to_selected: bool,
    close_after_live: bool,
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
        let scheme = std::fs::read_to_string(data_dir().join("palette.txt"))
            .map(|value| Scheme::from_key(&value))
            .unwrap_or_default();
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
        let mut app = Self::workspace(&cc.egui_ctx, project, path, start_view, dark_mode, scheme);
        app.port = ports.first().cloned().unwrap_or_default();
        app.ports = ports;
        app.show_devices = demo;
        app.demo = demo;
        app.screenshot = screenshot;
        app.status = startup_error.clone().unwrap_or_else(|| "Ready".into());
        app.error = startup_error.is_some();
        if demo {
            app.selected_point = app.project.points.first().map(|p| p.id.clone());
        }
        app.request_page(&cc.egui_ctx);
        if !app.demo && startup_error.is_none() {
            app.save();
        }
        app
    }
    fn workspace(
        ctx: &Context,
        project: Project,
        path: PathBuf,
        start_view: &str,
        dark_mode: bool,
        scheme: Scheme,
    ) -> Self {
        configure_fonts(ctx);
        appearance::apply(ctx, dark_mode, scheme);
        let (tx, rx) = mpsc::channel();
        Self {
            project,
            path,
            dirty: false,
            last_save: Instant::now(),
            view: match start_view {
                "readings" => View::Readings,
                "traverse" => View::Traverse,
                "live" => View::Live,
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
            show_devices: false,
            unit_filter: "All units".into(),
            port: String::new(),
            ports: Vec::new(),
            device: None,
            busy: false,
            status: String::from("Ready"),
            error: false,
            tx,
            rx,
            manual_value: 0.,
            demo: false,
            screenshot: None,
            capture_at: Instant::now() + Duration::from_secs(3),
            capture_requested: false,
            confirm_delete: None,
            show_about: false,
            status_details: None,
            source_details: None,
            dark_mode,
            scheme,
            live_running: false,
            live_started_at: None,
            live_stop: Arc::new(AtomicBool::new(false)),
            live_updates: None,
            live_dropped: Arc::new(AtomicUsize::new(0)),
            live_modules: Vec::new(),
            live_channels: BTreeMap::new(),
            live_session_changed: false,
            live_failure: None,
            live_warning: None,
            live_raw_path: None,
            live_attach_to_selected: false,
            close_after_live: false,
        }
    }
    /// Isolated in-memory state for CPU previews. No ports, preferences or files
    /// are read, and no project is saved by this constructor.
    pub fn preview(ctx: &Context, view: &str, dark: bool, scheme: Scheme, example: bool) -> Self {
        let project = if example {
            example_project()
        } else {
            Project::new("Untitled survey")
        };
        let mut app = Self::workspace(ctx, project, PathBuf::new(), view, dark, scheme);
        app.demo = example;
        app.status = if example { "Example project" } else { "Ready" }.into();
        if example {
            app.port = "Example USB".into();
            app.ports = vec![app.port.clone()];
            app.device = Some(usb::DeviceInfo {
                port: app.port.clone(),
                model: "WR-401".into(),
                serial: "EXAMPLE".into(),
                firmware: "Rev. 1.04Y6".into(),
                group_count: 2,
                record_count: 18,
            });
            if app.view == View::Live {
                let now = Instant::now();
                app.live_running = true;
                app.live_modules.push(live::ModuleInfo {
                    address: 1,
                    kind: "SNE".into(),
                    name: "Example pressure".into(),
                    channels: 1,
                });
                let history = (0..31)
                    .map(|index| {
                        let value = [-0.0001, 0., -0.0001, -0.0002, 0.][index % 5];
                        (now - Duration::from_secs((30 - index) as u64 * 7), value)
                    })
                    .collect();
                app.live_channels.insert(
                    (1, 0),
                    LiveChannel {
                        value: -0.0001,
                        display_value: "-0.0001".into(),
                        unit: "inW".into(),
                        received: now,
                        received_at: "2026-01-01T12:00:00Z".into(),
                        device_timestamp: Some("2026-01-01T12:00:00".into()),
                        raw: "Synthetic preview sample".into(),
                        history,
                    },
                );
            }
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
        if self.port.is_empty() || self.busy || self.live_running {
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
        if self.busy || self.port.is_empty() || self.live_running {
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
    fn start_live(&mut self, ctx: &Context) {
        if self.busy || self.live_running || !self.live_compatible() {
            return;
        }
        let raw_path = data_dir().join("live-captures").join(format!(
            "{}-{}.raw.ndjson",
            chrono::Local::now().format("%Y%m%d-%H%M%S"),
            uuid::Uuid::new_v4()
        ));
        self.live_running = true;
        self.live_started_at = Some(Instant::now());
        self.live_stop = Arc::new(AtomicBool::new(false));
        self.live_dropped = Arc::new(AtomicUsize::new(0));
        self.live_modules.clear();
        self.live_channels.clear();
        self.live_session_changed = false;
        self.live_failure = None;
        self.live_warning = None;
        self.live_raw_path = Some(raw_path.clone());
        let (updates_tx, updates_rx) = mpsc::sync_channel(128);
        self.live_updates = Some(updates_rx);
        self.message("Opening live USB reception…", false);
        let port = self.port.clone();
        let stop = self.live_stop.clone();
        let dropped = self.live_dropped.clone();
        let tx = self.tx.clone();
        let context = ctx.clone();
        std::thread::spawn(move || {
            let event_tx = tx.clone();
            let event_context = context.clone();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                live::monitor(&port, &raw_path, stop, move |event| {
                    match event {
                        event @ (live::LiveEvent::Modules(_) | live::LiveEvent::SessionChanged) => {
                            let _ = event_tx.send(Event::Live(event));
                        }
                        event => {
                            // Native dialogs may pause UI updates. Keep the view queue bounded;
                            // the worker's raw capture retains packets independently.
                            if updates_tx.try_send((event, Instant::now())).is_err() {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    event_context.request_repaint();
                })
            }))
            .map_err(|_| "The live USB worker stopped unexpectedly.".to_string())
            .and_then(|result| result.map_err(|error| format!("{error:#}")));
            let _ = tx.send(Event::LiveStopped(result));
            context.request_repaint();
        });
    }
    fn live_compatible(&self) -> bool {
        self.device.as_ref().is_some_and(|device| {
            device.port == self.port && device.firmware.replace(' ', "") == "Rev.1.04Y6"
        })
    }
    fn apply_live_event(&mut self, event: live::LiveEvent, received: Instant) {
        match event {
            live::LiveEvent::Modules(modules) => {
                self.live_modules = modules.into_iter().take(10).collect();
            }
            live::LiveEvent::SessionChanged => {
                self.live_session_changed = true;
                self.message("Starting live reception…", false);
            }
            live::LiveEvent::Warning(warning) => self.live_warning = Some(warning),
            live::LiveEvent::Sample(sample) => {
                let Some(module) = self
                    .live_modules
                    .iter()
                    .find(|module| module.address == sample.module_address)
                else {
                    return;
                };
                for (index, value) in sample
                    .values
                    .iter()
                    .take(module.channels.min(16))
                    .enumerate()
                {
                    if !value.value.is_finite() || value.unit.trim().is_empty() {
                        continue;
                    }
                    let channel = self
                        .live_channels
                        .entry((sample.module_address, index))
                        .or_insert_with(|| LiveChannel {
                            value: value.value,
                            display_value: value.display_value.clone(),
                            unit: value.unit.clone(),
                            received,
                            received_at: sample.received_at.clone(),
                            device_timestamp: sample.device_timestamp.clone(),
                            raw: sample.raw.clone(),
                            history: VecDeque::new(),
                        });
                    channel.update(value, &sample, received);
                }
            }
        }
    }
    fn capture_live(&mut self, key: (u8, usize)) {
        if !self.live_running || self.live_stop.load(Ordering::Relaxed) {
            return;
        }
        let Some(channel) = self
            .live_channels
            .get(&key)
            .filter(|channel| channel.fresh(Instant::now()))
            .cloned()
        else {
            return;
        };
        let Some(module) = self
            .live_modules
            .iter()
            .find(|module| module.address == key.0)
        else {
            return;
        };
        let target = self
            .live_attach_to_selected
            .then(|| self.selected_point.clone())
            .flatten();
        let group = target
            .as_ref()
            .and_then(|id| self.project.points.iter().find(|point| &point.id == id))
            .map(|point| point.label.clone())
            .unwrap_or_else(|| format!("Live USB · {}", module.name));
        let reading = live_reading(&channel, module, key.1, group);
        let id = reading.id.clone();
        self.project.readings.push(reading);
        if let Some(point) =
            target.and_then(|id| self.project.points.iter_mut().find(|point| point.id == id))
        {
            point.reading_ids.push(id.clone());
        }
        self.selected_reading = Some(id);
        self.changed();
        self.message("Live reading captured in the project.", false);
    }
    fn receive(&mut self, ctx: &Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Progress(m) => self.message(m, false),
                Event::Live(event) => self.apply_live_event(event, Instant::now()),
                Event::LiveStopped(result) => {
                    self.live_running = false;
                    self.live_stop.store(true, Ordering::Relaxed);
                    if self.live_session_changed {
                        self.device = None;
                    }
                    match result {
                        Ok(()) => self.message(
                            if self.live_session_changed {
                                format!("Stopped receiving on the PC. {LIVE_RECOVERY}")
                            } else {
                                "Live connection closed.".into()
                            },
                            false,
                        ),
                        Err(error) => {
                            self.live_failure = Some(error.clone());
                            self.message(format!("Live reception ended: {error}"), true);
                        }
                    }
                }
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
        while let Some((event, received)) = self
            .live_updates
            .as_ref()
            .and_then(|updates| updates.try_recv().ok())
        {
            self.apply_live_event(event, received);
        }
        let dropped = self.live_dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            self.live_warning = Some(format!(
                "The view skipped {dropped} updates while busy. Received packets remain in the session capture."
            ));
        }
    }
    fn persist_appearance(&mut self, ctx: &Context) {
        appearance::apply(ctx, self.dark_mode, self.scheme);
        let result = std::fs::create_dir_all(data_dir()).and_then(|()| {
            std::fs::write(
                data_dir().join("appearance.txt"),
                if self.dark_mode { "dark" } else { "light" },
            )?;
            std::fs::write(data_dir().join("palette.txt"), self.scheme.key())
        });
        if let Err(error) = result {
            self.message(
                format!("Appearance applied, but could not save the preference: {error}"),
                true,
            );
        }
    }
    fn inspector(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        SidePanel::right("inspector").default_width(285.).width_range(255.0..=400.0).frame(Frame::new().fill(colors.panel).inner_margin(18)).show(ctx,|ui|{
            ScrollArea::vertical().id_salt("device-inspector-scroll").auto_shrink([false,false]).show(ui,|ui|{
            ui.horizontal(|ui| { ui.heading("Device"); if ui.small_button("Close").clicked() { self.selected_point = None; } });
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
            ui.add_space(12.);ui.collapsing("Notes", |ui| { dirty|=ui.add(TextEdit::multiline(&mut p.notes).desired_rows(3).desired_width(f32::INFINITY).hint_text("Location, valve position, access…")).changed(); });
            let ids=p.reading_ids.clone();let design=p.design_value;let unit=p.unit.clone();
            ui.add_space(16.);ui.separator();ui.add_space(12.);eyebrow(ui,"ATTACHED MEASUREMENTS");
            let mut remove=None;
            ScrollArea::vertical().id_salt("attached").max_height(230.).show(ui,|ui|{for id in &ids{if let Some(r)=self.project.readings.iter().find(|r|&r.id==id){
                Frame::new().fill(colors.paper).corner_radius(1).inner_margin(9).show(ui,|ui|{ui.horizontal(|ui|{let value=format!("{} {}",reading_value(r),r.unit);ui.add(Label::new(RichText::new(&value).size(20.).strong().color(colors.green)).truncate()).on_hover_text(&value);if ui.small_button("×").on_hover_text("Detach reading").clicked(){remove=Some(id.clone());}});ui.label(RichText::new(format!("{} · {}",r.kind,r.group)).size(11.).color(colors.muted));ui.label(RichText::new(&r.timestamp).size(11.).color(colors.muted));if design>0.&&r.unit.eq_ignore_ascii_case(&unit){ui.label(format!("{:.1}% of design",r.value/design*100.));}});ui.add_space(6.);
            }}});
            if let Some(id)=remove{self.project.points[index].reading_ids.retain(|i|i!=&id);dirty=true;}
            if ids.is_empty(){ui.label(RichText::new("No measurements attached.").color(colors.muted));}
            ui.add_space(10.);
            let selected=self.selected_reading.as_ref().and_then(|id|self.project.readings.iter().find(|r|&r.id==id));
            let text=selected.map(|r|format!("{} · {} {}",r.group,reading_value(r),r.unit)).unwrap_or("Choose a downloaded reading".into());
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
                        let label=format!("{} · {} · {} {}",r.group,r.kind,reading_value(r),r.unit);
                        if ui.add_sized([ui.available_width(),28.],Button::selectable(selected,&label).wrap_mode(TextWrapMode::Truncate)).on_hover_text(&label).clicked(){self.selected_reading=Some(r.id.clone());}
                    }
                });
            });
            if ui.add_enabled(self.selected_reading.is_some(),Button::new("Attach selected reading").min_size(vec2(ui.available_width(),30.))).clicked()&& let Some(id)=&self.selected_reading&& !self.project.points[index].reading_ids.contains(id){self.project.points[index].reading_ids.push(id.clone());dirty=true;}
            ui.add_space(12.);ui.collapsing("Enter a manual measurement",|ui|{ui.add(DragValue::new(&mut self.manual_value).speed(1.));ui.label(format!("Unit: {}",self.project.points[index].unit));if ui.button("Record and attach").clicked(){let p=&mut self.project.points[index];let id=uuid::Uuid::new_v4().to_string();self.project.readings.push(Reading{id:id.clone(),timestamp:chrono::Local::now().to_rfc3339(),group:p.label.clone(),sensor:"Manual entry".into(),kind:p.kind.clone(),value:self.manual_value,unit:p.unit.clone(),source:"manual".into(),raw:String::new()});p.reading_ids.push(id);dirty=true;}});
            ui.add_space(15.);if ui.small_button("Delete mapped device…").clicked(){self.confirm_delete=Some(self.project.points[index].id.clone());}
            if dirty{self.changed();}
            });
        });
    }
    fn blueprint(&mut self, ui: &mut Ui, ctx: &Context) {
        let colors = palette(ctx);
        if self.project.documents.is_empty() {
            ui.add_space((ui.available_height() * 0.18).max(24.));
            ui.vertical_centered(|ui| {
                ui.heading("Add a drawing");
                ui.add_space(16.);
                if ui
                    .add_enabled(
                        !self.busy,
                        Button::new(RichText::new("Import PDF…").color(Color32::WHITE))
                            .fill(colors.action)
                            .min_size(vec2(190., 42.)),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("Mechanical drawing", &["pdf"])
                        .pick_file()
                {
                    self.import_pdf(path, ctx);
                }
                ui.add_space(12.);
                ui.label(RichText::new("Or drop a mechanical PDF here.").color(colors.muted));
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
            if self.add_point {
                ComboBox::from_id_salt("point-kind")
                    .width(110.)
                    .selected_text(&self.point_kind)
                    .show_ui(ui, |ui| {
                        for k in kinds() {
                            ui.selectable_value(&mut self.point_kind, k.into(), k);
                        }
                    });
            }
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
            if doc.page_count > 1 {
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
        painter.rect_filled(canvas, 1, colors.canvas);
        painter.rect_stroke(canvas, 1, Stroke::new(1., colors.line), StrokeKind::Inside);
        let aspect = self
            .texture
            .as_ref()
            .map(|t| t.size_vec2().x / t.size_vec2().y)
            .unwrap_or(1.42);
        let fit = (canvas.width() - 24.).min((canvas.height() - 24.) * aspect);
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
                Color32::from_rgb(0, 47, 150)
            } else {
                GREEN
            };
            if selected {
                painter.circle_stroke(at, 16., Stroke::new(2., color));
            }
            painter.circle_filled(at, 10., Color32::WHITE);
            painter.circle_filled(at, 7., color);
            if !p.reading_ids.is_empty() {
                painter.circle_filled(at + vec2(7., -7.), 3.5, Color32::from_rgb(192, 96, 0));
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
                if let Some(pos) = response.interact_pointer_pos()
                    && let Some(p) = self.project.points.iter_mut().find(|p| &p.id == id)
                {
                    p.x = ((pos.x - sheet.left()) / sheet.width()).clamp(0., 1.);
                    p.y = ((pos.y - sheet.top()) / sheet.height()).clamp(0., 1.);
                    self.dirty = true;
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
            } else if self.add_point
                && drawing_ready
                && let Some(pos) = response
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
            ui.heading("Readings");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("CSV", |ui| {
                    if ui.button("Import CSV…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Measurements", &["csv"])
                            .pick_file()
                        {
                            match self.project.import_csv(&path) {
                                Ok(count) => {
                                    self.changed();
                                    self.message(format!("Imported {count} measurements."), false);
                                }
                                Err(error) => self.message(error.to_string(), true),
                            }
                        }
                    }
                    if ui.button("Export CSV…").clicked() {
                        ui.close();
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name("Green Suite readings.csv")
                            .save_file()
                        {
                            match self.project.export_csv(&path) {
                                Ok(()) => self.message("Measurements exported.", false),
                                Err(error) => self.message(error.to_string(), true),
                            }
                        }
                    }
                });
            });
        });
        ui.add_space(10.);
        self.usb_bar(ui, true);
        ui.add_space(12.);
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
        ui.horizontal_wrapped(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.search)
                    .hint_text("Search readings…")
                    .desired_width(320.),
            );
            let units: std::collections::BTreeSet<&str> = self
                .project
                .readings
                .iter()
                .map(|reading| reading.unit.as_str())
                .collect();
            ComboBox::from_id_salt("unit-filter")
                .selected_text(&self.unit_filter)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.unit_filter, "All units".into(), "All units");
                    for unit in units {
                        ui.selectable_value(&mut self.unit_filter, unit.to_string(), unit);
                    }
                });
        });
        ui.add_space(8.);
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
            RichText::new(format!("{} readings", filtered.len()))
                .size(11.)
                .color(colors.muted),
        );
        Frame::new()
            .fill(colors.panel)
            .stroke(Stroke::new(1., colors.line))
            .corner_radius(1)
            .inner_margin(12)
            .show(ui, |ui| {
                if filtered.is_empty() {
                    ui.label("No matching measurements. Download from USB or import a CSV.");
                }
                const ROW_HEIGHT: f32 = 32.;
                let table_width = ui.available_width().max(780.);
                let widths = [
                    table_width * 0.34,
                    table_width * 0.17,
                    table_width * 0.25,
                    table_width * 0.24,
                ];

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
                                    format!("{} {}", reading_value(reading), reading.unit),
                                    reading.timestamp.clone(),
                                    mapped_labels
                                        .get(reading.id.as_str())
                                        .map(|labels| labels.join(", "))
                                        .unwrap_or_else(|| "Unassigned".into()),
                                ]
                            } else {
                                ["READING", "VALUE", "RECORDED", "MAPPED TO"]
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
                ui.label(RichText::new(format!("{} {}", reading_value(&r), r.unit)).strong());
                if !self.project.points.is_empty() {
                    ComboBox::from_id_salt("reading-target")
                        .selected_text(
                            self.selected_point
                                .as_ref()
                                .and_then(|id| self.project.points.iter().find(|p| &p.id == id))
                                .map(|p| p.label.as_str())
                                .unwrap_or("Choose device"),
                        )
                        .show_ui(ui, |ui| {
                            for point in &self.project.points {
                                ui.selectable_value(
                                    &mut self.selected_point,
                                    Some(point.id.clone()),
                                    &point.label,
                                );
                            }
                        });
                }
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
                    && let Some(p) = self
                        .project
                        .points
                        .iter_mut()
                        .find(|p| Some(&p.id) == self.selected_point.as_ref())
                    && !p.reading_ids.contains(&r.id)
                {
                    p.reading_ids.push(r.id.clone());
                    self.changed();
                    self.message("Reading attached to mapped device.", false);
                }
                if ui.button("Source record…").clicked() {
                    self.source_details = Some(r.clone());
                }
            });
        }
    }
    fn live_monitor(&mut self, ui: &mut Ui) {
        let colors = palette(ui.ctx());
        let stopping = self.live_running && self.live_stop.load(Ordering::Relaxed);
        let fresh = self
            .live_channels
            .values()
            .any(|channel| channel.fresh(Instant::now()));
        let state = if stopping {
            "Stopping"
        } else if self.live_running && self.live_channels.is_empty() {
            "Waiting for sensor"
        } else if self.live_running && !fresh {
            "Stale"
        } else if self.live_running {
            "Receiving"
        } else if self.live_failure.is_some() {
            "Connection ended"
        } else if self.live_compatible() {
            "Ready"
        } else if self.live_session_changed {
            "Stopped"
        } else {
            "Not connected"
        };
        let mut capture = None;
        ScrollArea::vertical()
            .id_salt("live-monitor-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Live readings");
                    ui.label(RichText::new(state).color(if self.live_running && fresh {
                        colors.green
                    } else {
                        colors.muted
                    }));
                });
                ui.add_space(12.);
                self.usb_bar(ui, false);
                if self.device.is_some() && !self.live_running && !self.live_compatible() {
                    ui.label(
                        RichText::new("Live reception requires Rev. 1.04Y6.").color(colors.warning),
                    );
                }
                if let Some(error) = &self.live_failure {
                    ui.label(RichText::new(error).color(colors.warning));
                }
                if let Some(warning) = &self.live_warning {
                    ui.label(RichText::new(warning).small().color(colors.warning));
                }
                if self.live_session_changed && !self.live_running {
                    ui.label(RichText::new(LIVE_RECOVERY).color(colors.warning));
                }
                if !self.project.points.is_empty() {
                    ui.add_space(8.);
                    let mut target = self
                        .live_attach_to_selected
                        .then(|| self.selected_point.clone())
                        .flatten();
                    let label = target
                        .as_ref()
                        .and_then(|id| self.project.points.iter().find(|p| &p.id == id))
                        .map(|p| p.label.as_str())
                        .unwrap_or("Project only");
                    ui.horizontal(|ui| {
                        ui.label("Capture to");
                        ComboBox::from_id_salt("live-capture-target")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut target, None, "Project only");
                                for point in &self.project.points {
                                    ui.selectable_value(
                                        &mut target,
                                        Some(point.id.clone()),
                                        &point.label,
                                    );
                                }
                            });
                    });
                    self.live_attach_to_selected = target.is_some();
                    if target.is_some() {
                        self.selected_point = target;
                    }
                } else {
                    self.live_attach_to_selected = false;
                }
                ui.add_space(16.);
                if self.live_channels.is_empty() {
                    ui.label(
                        RichText::new(if self.live_running {
                            "Waiting for the first reading…"
                        } else {
                            "No live readings yet."
                        })
                        .color(colors.muted),
                    );
                    return;
                }
                for (key, channel) in &self.live_channels {
                    let Some(module) = self
                        .live_modules
                        .iter()
                        .find(|module| module.address == key.0)
                    else {
                        continue;
                    };
                    let recent = self.live_running && !stopping && channel.fresh(Instant::now());
                    Frame::new()
                        .fill(colors.paper)
                        .stroke(Stroke::new(2., colors.line))
                        .inner_margin(18)
                        .show(ui, |ui| {
                            ui.set_min_width((ui.available_width() - 1.).max(0.));
                            ui.horizontal_wrapped(|ui| {
                                let label = if module.channels == 1 {
                                    module.name.clone()
                                } else {
                                    format!("{} / {}", module.name, key.1 + 1)
                                };
                                ui.label(RichText::new(label).font(serif_bold(16.)));
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {:.1}s ago",
                                        if recent {
                                            "Live"
                                        } else if self.live_running {
                                            "Stale"
                                        } else {
                                            "Last received"
                                        },
                                        channel.received.elapsed().as_secs_f32()
                                    ))
                                    .small()
                                    .color(if recent {
                                        colors.green
                                    } else {
                                        colors.warning
                                    }),
                                );
                            });
                            ui.add_space(8.);
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(&channel.display_value)
                                        .font(serif_bold(48.))
                                        .color(colors.green),
                                );
                                ui.label(RichText::new(&channel.unit).font(serif_bold(22.)));
                                ui.add_space(18.);
                                if ui
                                    .add_enabled(
                                        recent,
                                        Button::new(if self.live_attach_to_selected {
                                            "Capture & attach"
                                        } else {
                                            "Capture reading"
                                        }),
                                    )
                                    .clicked()
                                {
                                    capture = Some(*key);
                                }
                            });
                            ui.add_space(12.);
                            live_trend(ui, channel);
                            ui.collapsing("Reading details", |ui| {
                                if let Some(stamp) = &channel.device_timestamp {
                                    ui.label(format!("Reporter time: {stamp}"));
                                }
                                ui.label(format!("PC received: {}", channel.received_at));
                            });
                        });
                    ui.add_space(12.);
                }
            });
        if let Some(key) = capture {
            self.capture_live(key);
        }
    }
    fn traverses(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.heading("Traverse");
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
        let flow = traverse::calculate(t)
            .ok()
            .and_then(|result| result.flow_cfm);
        let mut capture_flow = false;
        ui.horizontal(|ui| {
            ui.menu_button("Details", |ui| {
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
            capture_flow = ui
                .add_enabled(flow.is_some(), Button::new("Capture flow"))
                .on_hover_text("Save calculated CFM in this project")
                .clicked();
        });
        if let Some(flow) = flow.filter(|_| capture_flow) {
            let id = uuid::Uuid::new_v4().to_string();
            self.project.readings.push(Reading {
                id: id.clone(),
                timestamp: chrono::Local::now().to_rfc3339(),
                group: t.name.clone(),
                sensor: "Traverse calculation".into(),
                kind: "Airflow".into(),
                value: flow,
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
        ui.add_space(8.);
        changed |= self.traverse_view.ui(ui, t, &self.project.readings);
        if changed {
            self.changed();
        }
    }
}

impl GreenSuite {
    pub fn ui(&mut self, ctx: &Context) {
        let colors = palette(ctx);
        self.receive(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && self.live_running {
            self.live_stop.store(true, Ordering::Relaxed);
            self.close_after_live = true;
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
        }
        if self.close_after_live && !self.live_running && !self.busy {
            self.close_after_live = false;
            if !self.dirty || self.save() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        if ctx.input(|i| i.viewport().close_requested())
            && (self.busy || (self.dirty && !self.save()))
        {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if self.busy {
                self.message("A USB transfer or PDF import is running. Please wait for it to finish before closing.",false);
            }
        }
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            if let Some(path) = file.path
                && path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            {
                self.import_pdf(path, ctx);
                break;
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::S)) {
            self.save();
        }
        self.header(ctx);
        TopBottomPanel::bottom("status")
            .exact_height(30.)
            .frame(
                Frame::new()
                    .fill(colors.paper)
                    .inner_margin(Margin::symmetric(18, 4)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy || self.page_loading {
                        ui.spinner();
                    }
                    ui.add_sized(
                        vec2((ui.available_width() - 92.).max(80.), 20.),
                        Label::new(
                            RichText::new(if self.dirty {
                                "Saving…"
                            } else {
                                &self.status
                            })
                            .size(12.)
                            .color(if self.error {
                                colors.warning
                            } else {
                                colors.muted
                            }),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&self.status);
                    if ui.small_button("Details").clicked() {
                        self.status_details = Some(self.status.clone());
                    }
                });
            });
        if self.view == View::Blueprint && !self.project.documents.is_empty() {
            self.sidebar(ctx);
            if self.selected_point.is_some() {
                self.inspector(ctx);
            }
        }
        CentralPanel::default()
            .frame(Frame::new().fill(colors.paper).inner_margin(22))
            .show(ctx, |ui| match self.view {
                View::Blueprint => self.blueprint(ui, ctx),
                View::Readings => self.readings(ui),
                View::Traverse => self.traverses(ui),
                View::Live => self.live_monitor(ui),
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
            Window::new("About Green Suite").open(&mut self.show_about).resizable(false).show(ctx,|ui|{ui.heading("Green Suite 0.1");ui.label("Measurement workspace · USB instruments");ui.label("Copyright © 2026 Kevin R. Whiteman. All rights reserved.");ui.label("Original Green Suite code is proprietary. Evergreen-owned materials\nand third-party libraries retain their respective rights and licenses.");ui.add_space(10.);ui.label("Import PDF drawings. Place devices. Attach stored measurements.\nBuild equal-area traverses and inspect animated velocity profiles.");ui.label("PDFs and projects are stored locally. No network service is used.");});
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
                    ui.heading(format!("{} {}", reading_value(&reading), reading.unit));
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
}

impl eframe::App for GreenSuite {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.live_stop.store(true, Ordering::Relaxed);
        if self.dirty {
            self.save();
        }
    }
}

fn live_reading(
    channel: &LiveChannel,
    module: &live::ModuleInfo,
    index: usize,
    group: String,
) -> Reading {
    let timestamp_source = if channel.device_timestamp.is_some() {
        "reporter-local"
    } else {
        "pc-received"
    };
    Reading {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: channel
            .device_timestamp
            .clone()
            .unwrap_or_else(|| channel.received_at.clone()),
        group,
        sensor: format!("{} · {} · Channel {}", module.name, module.kind, index + 1),
        kind: "Live measurement".into(),
        value: channel.value,
        unit: channel.unit.clone(),
        source: "wrist-reporter-live".into(),
        raw: serde_json::json!({
            "packet": channel.raw,
            "received_at": channel.received_at,
            "timestamp_source": timestamp_source,
            "module_address": module.address,
            "channel": index + 1,
            "display_value": channel.display_value,
        })
        .to_string(),
    }
}

fn live_trend(ui: &mut Ui, channel: &LiveChannel) {
    let colors = palette(ui.ctx());
    ui.label(
        RichText::new(format!(
            "TREND · {} updates · {}",
            channel.history.len(),
            channel.unit
        ))
        .font(serif_bold(11.))
        .color(colors.muted),
    );
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.), Sense::hover());
    if !ui.is_rect_visible(rect) || channel.history.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 1, colors.canvas);
    let plot = rect.shrink2(vec2(12., 19.));
    let low = channel
        .history
        .iter()
        .map(|(_, value)| *value)
        .fold(f64::INFINITY, f64::min);
    let high = channel
        .history
        .iter()
        .map(|(_, value)| *value)
        .fold(f64::NEG_INFINITY, f64::max);
    // Normalize before subtraction so even finite, very large values cannot
    // produce an infinite range or non-finite painter coordinates.
    let scale = low.abs().max(high.abs()).max(f64::MIN_POSITIVE);
    let min = low / scale;
    let max = high / scale;
    let first = channel.history.front().unwrap().0;
    let last = channel.history.back().unwrap().0;
    let elapsed = last.saturating_duration_since(first).as_secs_f64();
    for fraction in [0., 0.5, 1.] {
        let y = plot.top() + plot.height() * fraction;
        painter.hline(plot.x_range(), y, Stroke::new(0.6, colors.grid));
    }
    let points: Vec<Pos2> = channel
        .history
        .iter()
        .map(|(time, value)| {
            let x = if elapsed > 0. {
                time.saturating_duration_since(first).as_secs_f64() / elapsed
            } else {
                0.5
            };
            let y = if max > min {
                ((*value / scale - min) / (max - min)).clamp(0., 1.)
            } else {
                0.5
            };
            pos2(
                plot.left() + plot.width() * x as f32,
                plot.bottom() - plot.height() * y as f32,
            )
        })
        .collect();
    if points.len() > 1 {
        painter.add(Shape::line(points.clone(), Stroke::new(1.8, colors.green)));
    }
    if let Some(point) = points.last() {
        painter.circle_filled(*point, 3., colors.secondary);
    }
    painter.text(
        rect.left_top() + vec2(8., 3.),
        Align2::LEFT_TOP,
        format!("{low} – {high} {}", channel.unit),
        FontId::proportional(10.),
        colors.muted,
    );
    painter.text(
        rect.left_bottom() + vec2(8., -2.),
        Align2::LEFT_BOTTOM,
        format!("{elapsed:.0}s history"),
        FontId::proportional(10.),
        colors.muted,
    );
    painter.text(
        rect.right_bottom() + vec2(-8., -2.),
        Align2::RIGHT_BOTTOM,
        "Latest received",
        FontId::proportional(10.),
        colors.muted,
    );
}

fn reading_value(reading: &Reading) -> String {
    if reading.source == "wrist-reporter-live"
        && let Ok(raw) = serde_json::from_str::<serde_json::Value>(&reading.raw)
        && let Some(display) = raw["display_value"].as_str()
        && display.len() <= 40
        && display
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite() && value == reading.value)
    {
        return display.to_owned();
    }
    reading.value.to_string()
}

fn serif_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Serif Bold".into()))
}

fn configure_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "serif-semibold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/IosevkaSlab-Semibold.ttf")).into(),
    );
    fonts.font_data.insert(
        "serif-bold".into(),
        FontData::from_static(include_bytes!("../assets/fonts/IosevkaSlab-Bold.ttf")).into(),
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

fn eyebrow(ui: &mut Ui, s: &str) {
    let colors = palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(s).font(serif_bold(11.)).color(colors.muted));
        let (rule, _) =
            ui.allocate_exact_size(vec2(ui.available_width().max(0.), 10.), Sense::hover());
        ui.painter().hline(
            rule.x_range(),
            rule.center().y,
            Stroke::new(0.7, colors.line),
        );
    });
    ui.add_space(5.);
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
    let line = Color32::from_rgb(32, 32, 32);
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
    let blue = Color32::from_rgb(0, 47, 150);
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

    #[test]
    fn live_history_is_bounded_and_never_carries_old_units_forward() {
        let now = Instant::now();
        let mut sample = live::LiveSample {
            module_address: 1,
            values: vec![live::LiveValue {
                value: 0.0003,
                display_value: "0.0003".into(),
                unit: "inW".into(),
            }],
            device_timestamp: None,
            received_at: "2026-01-01T12:00:00Z".into(),
            raw: "synthetic pressure packet".into(),
        };
        let mut channel = LiveChannel {
            value: 0.,
            display_value: "0".into(),
            unit: String::new(),
            received: now,
            received_at: String::new(),
            device_timestamp: None,
            raw: String::new(),
            history: VecDeque::new(),
        };
        for index in 0..200 {
            sample.values[0].value = index as f64;
            sample.values[0].display_value = index.to_string();
            channel.update(&sample.values[0], &sample, now + Duration::from_secs(index));
        }
        assert_eq!(channel.history.len(), LIVE_HISTORY_LIMIT);
        assert_eq!(channel.history.front().unwrap().1, 20.);
        assert_eq!(channel.history.back().unwrap().1, 199.);
        assert!(channel.fresh(now + Duration::from_secs(213)));
        assert!(!channel.fresh(now + Duration::from_secs(214)));
        sample.values[0].unit = "Pa".into();
        channel.update(&sample.values[0], &sample, now + Duration::from_secs(205));
        assert_eq!(channel.history.len(), 1);
    }

    #[test]
    fn live_capture_preserves_precision_and_identifies_timestamp_source() {
        let channel = LiveChannel {
            value: 0.0003,
            display_value: "0.0003".into(),
            unit: "inW".into(),
            received: Instant::now(),
            received_at: "2026-01-01T12:00:00Z".into(),
            device_timestamp: None,
            raw: "synthetic pressure packet".into(),
            history: VecDeque::new(),
        };
        let module = live::ModuleInfo {
            address: 1,
            kind: "SNE".into(),
            name: "Example pressure".into(),
            channels: 1,
        };
        let pc = live_reading(&channel, &module, 0, "Example system".into());
        assert_eq!(pc.value, 0.0003);
        assert_eq!(pc.unit, "inW");
        assert_eq!(pc.timestamp, channel.received_at);
        assert_eq!(pc.source, "wrist-reporter-live");
        let raw: serde_json::Value = serde_json::from_str(&pc.raw).unwrap();
        assert_eq!(raw["timestamp_source"], "pc-received");
        assert_eq!(raw["display_value"], "0.0003");
        let mut instrument_channel = channel.clone();
        instrument_channel.device_timestamp = Some("2026-01-01 06:00:00".into());
        let instrument = live_reading(&instrument_channel, &module, 0, "Example system".into());
        assert_eq!(instrument.timestamp, "2026-01-01 06:00:00");
        let raw: serde_json::Value = serde_json::from_str(&instrument.raw).unwrap();
        assert_eq!(raw["timestamp_source"], "reporter-local");
    }

    #[test]
    fn live_readout_preserves_only_precision_that_matches_the_stored_value() {
        let mut reading = Reading {
            source: "wrist-reporter-live".into(),
            value: 0.,
            raw: serde_json::json!({"display_value":"0.0000"}).to_string(),
            ..Default::default()
        };
        assert_eq!(reading_value(&reading), "0.0000");
        reading.value = -0.0001;
        reading.raw = serde_json::json!({"display_value":"-0.0001"}).to_string();
        assert_eq!(reading_value(&reading), "-0.0001");
        reading.raw = serde_json::json!({"display_value":"99.0000"}).to_string();
        assert_eq!(reading_value(&reading), "-0.0001");
    }

    #[test]
    #[ignore = "requires a private CLI result in GREEN_SUITE_LIVE_RESULT_PATH"]
    fn private_live_capture_round_trips_through_project_storage() {
        fn text<'a>(value: &'a serde_json::Value, key: &str) -> &'a str {
            value[key].as_str().expect("Missing capture text field")
        }
        struct TemporaryProject(PathBuf);
        impl Drop for TemporaryProject {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        let input = std::env::var_os("GREEN_SUITE_LIVE_RESULT_PATH")
            .expect("Set GREEN_SUITE_LIVE_RESULT_PATH to a private CLI result");
        let result: serde_json::Value =
            serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
        let latest = &result["latest"];
        let values = latest["values"]
            .as_array()
            .expect("The CLI result has no validated latest sample");
        assert!(!values.is_empty(), "The latest sample has no channels");
        let device_timestamp = latest["device_timestamp"].as_str();
        let received_at = text(latest, "received_at");
        let module = live::ModuleInfo {
            address: u8::try_from(latest["module_address"].as_u64().unwrap()).unwrap(),
            kind: "SNE".into(),
            name: "Private pressure capture".into(),
            channels: values.len(),
        };
        let mut project = Project::new("Private live persistence test");
        for (index, value) in values.iter().enumerate() {
            let channel = LiveChannel {
                value: value["value"].as_f64().unwrap(),
                display_value: text(value, "display_value").into(),
                unit: text(value, "unit").into(),
                received: Instant::now(),
                received_at: received_at.into(),
                device_timestamp: device_timestamp.map(str::to_owned),
                raw: text(latest, "raw").into(),
                history: VecDeque::new(),
            };
            project.readings.push(live_reading(
                &channel,
                &module,
                index,
                "Private capture".into(),
            ));
        }

        let temporary = TemporaryProject(std::env::temp_dir().join(format!(
            "green-suite-live-test-{}.green-suite.json",
            uuid::Uuid::new_v4()
        )));
        project.save(&temporary.0).unwrap();
        let restored = Project::load(&temporary.0).unwrap();
        assert_eq!(restored.readings.len(), values.len());
        for (index, (reading, value)) in restored.readings.iter().zip(values).enumerate() {
            assert_eq!(reading.id, project.readings[index].id);
            assert_eq!(
                reading.value.to_bits(),
                value["value"].as_f64().unwrap().to_bits()
            );
            assert_eq!(reading.unit, text(value, "unit"));
            assert_eq!(reading.timestamp, device_timestamp.unwrap_or(received_at));
            assert_eq!(reading.source, "wrist-reporter-live");
            assert_eq!(reading.kind, "Live measurement");
            let raw: serde_json::Value = serde_json::from_str(&reading.raw).unwrap();
            assert_eq!(raw["display_value"], value["display_value"]);
            assert_eq!(raw["packet"], latest["raw"]);
            assert_eq!(raw["received_at"], latest["received_at"]);
            assert_eq!(raw["module_address"], latest["module_address"]);
            assert_eq!(raw["channel"], index + 1);
            assert_eq!(
                raw["timestamp_source"],
                if device_timestamp.is_some() {
                    "reporter-local"
                } else {
                    "pc-received"
                }
            );
        }
        std::fs::remove_file(&temporary.0).unwrap();
    }
}
