// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

//! Equal-area traverse planning, sample capture, and visualization.
//! This is a measurement aid; the project specification determines the required layout.

use crate::model::{Reading, Traverse};
use anyhow::{Result, bail};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
struct Palette {
    green: Color32,
    ink: Color32,
    muted: Color32,
    border: Color32,
    panel: Color32,
    canvas: Color32,
    duct: Color32,
    grid: Color32,
    warning: Color32,
    error: Color32,
    particle: Color32,
    point_text: Color32,
    heat_low: Color32,
    heat_high: Color32,
}

impl Palette {
    fn for_ui(ui: &egui::Ui) -> Self {
        if ui.visuals().dark_mode {
            Self {
                green: Color32::from_rgb(132, 218, 161),
                ink: Color32::from_rgb(235, 241, 226),
                muted: Color32::from_rgb(179, 196, 182),
                border: Color32::from_rgb(72, 94, 81),
                panel: Color32::from_rgb(29, 46, 38),
                canvas: Color32::from_rgb(24, 35, 30),
                duct: Color32::from_rgb(19, 30, 23),
                grid: Color32::from_rgb(83, 114, 92),
                warning: Color32::from_rgb(65, 48, 23),
                error: Color32::from_rgb(255, 160, 138),
                particle: Color32::from_rgba_unmultiplied(212, 243, 188, 100),
                point_text: Color32::from_rgb(15, 39, 23),
                heat_low: Color32::from_rgb(34, 60, 42),
                heat_high: Color32::from_rgb(57, 127, 78),
            }
        } else {
            Self {
                green: Color32::from_rgb(20, 115, 86),
                ink: Color32::from_rgb(28, 54, 48),
                muted: Color32::from_rgb(85, 107, 97),
                border: Color32::from_rgb(199, 215, 204),
                panel: Color32::from_rgb(235, 244, 237),
                canvas: Color32::from_rgb(244, 248, 244),
                duct: Color32::from_rgb(249, 251, 247),
                grid: Color32::from_rgb(190, 211, 197),
                warning: Color32::from_rgb(255, 242, 216),
                error: Color32::from_rgb(170, 61, 45),
                particle: Color32::from_rgba_unmultiplied(22, 90, 67, 75),
                point_text: Color32::WHITE,
                heat_low: Color32::from_rgb(225, 238, 220),
                heat_high: Color32::from_rgb(100, 190, 145),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraverseResult {
    pub area_sq_ft: f64,
    pub mean_fpm: Option<f64>,
    pub flow_cfm: Option<f64>,
    pub filled: usize,
    pub total: usize,
}

fn circular(shape: &str) -> bool {
    shape.eq_ignore_ascii_case("circular") || shape.eq_ignore_ascii_case("circle")
}

fn sample_count(traverse: &Traverse) -> Result<usize> {
    if !circular(&traverse.shape) && !traverse.shape.eq_ignore_ascii_case("rectangular") {
        bail!("Choose a rectangular or circular duct shape.");
    }
    let count = traverse.rows.checked_mul(traverse.columns);
    match count {
        Some(count) if count > 0 && count <= 400 => Ok(count),
        _ => bail!("Choose a layout containing 1 to 400 sample points."),
    }
}

/// Every rectangular cell or circular annular sector has the same area.
/// Partial means are reported, but a flow is returned only for a complete traverse.
pub fn calculate(traverse: &Traverse) -> Result<TraverseResult> {
    let total = sample_count(traverse)?;
    let area_sq_ft = if circular(&traverse.shape) {
        if !traverse.diameter.is_finite() || traverse.diameter <= 0.0 {
            bail!("Diameter must be a finite number greater than zero.");
        }
        std::f64::consts::PI * (traverse.diameter / 24.0).powi(2)
    } else {
        if !traverse.width.is_finite()
            || !traverse.height.is_finite()
            || traverse.width <= 0.0
            || traverse.height <= 0.0
        {
            bail!("Width and height must be finite numbers greater than zero.");
        }
        traverse.width / 12.0 * (traverse.height / 12.0)
    };
    if !area_sq_ft.is_finite() {
        bail!("The duct area is outside the supported numeric range.");
    }
    if traverse.velocities.len() > total {
        bail!("The saved samples do not match this layout. Apply the layout again.");
    }
    let mut sum = 0.0;
    let mut filled = 0;
    for (index, value) in traverse.velocities.iter().enumerate() {
        if let Some(value) = value {
            if !value.is_finite() || *value < 0.0 {
                bail!("Point {} needs a finite, nonnegative velocity.", index + 1);
            }
            sum += value;
            filled += 1;
        }
    }
    if !sum.is_finite() {
        bail!("Sample velocities are outside the supported numeric range.");
    }
    let mean_fpm = (filled > 0).then(|| sum / filled as f64);
    let flow_cfm = if filled == total {
        let flow = area_sq_ft * mean_fpm.unwrap_or(0.0);
        if !flow.is_finite() {
            bail!("The calculated flow is outside the supported numeric range.");
        }
        Some(flow)
    } else {
        None
    };
    Ok(TraverseResult {
        area_sq_ft,
        mean_fpm,
        flow_cfm,
        filled,
        total,
    })
}

/// Coordinates in the duct's normalized bounding box, with (0, 0) at top left.
/// Circular annular-sector points use the midpoint in area, r = sqrt((ring+.5)/rings).
pub fn point_positions(traverse: &Traverse) -> Vec<[f32; 2]> {
    if sample_count(traverse).is_err() {
        return Vec::new();
    }
    let mut points = Vec::with_capacity(traverse.rows * traverse.columns);
    for row in 0..traverse.rows {
        for column in 0..traverse.columns {
            if circular(&traverse.shape) {
                let radius = ((row as f32 + 0.5) / traverse.rows as f32).sqrt() * 0.5;
                let angle = std::f32::consts::TAU * (column as f32 + 0.5) / traverse.columns as f32
                    - std::f32::consts::FRAC_PI_2;
                points.push([0.5 + radius * angle.cos(), 0.5 + radius * angle.sin()]);
            } else {
                points.push([
                    (column as f32 + 0.5) / traverse.columns as f32,
                    (row as f32 + 0.5) / traverse.rows as f32,
                ]);
            }
        }
    }
    points
}

/// Accept only explicit velocity units; no pressure or volume conversion is inferred.
pub fn velocity_to_fpm(value: f64, unit: &str) -> Option<f64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let unit = unit.trim().to_ascii_lowercase().replace(' ', "");
    let value = match unit.as_str() {
        "fpm" | "ft/min" | "ft/minute" | "feet/min" => value,
        "m/s" | "mps" | "m/sec" => value * 60.0 / 0.3048,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

#[derive(Clone, PartialEq)]
struct Layout {
    shape: String,
    width: f64,
    height: f64,
    diameter: f64,
    rows: usize,
    columns: usize,
}

impl Layout {
    fn from_traverse(traverse: &Traverse) -> Self {
        Self {
            shape: if circular(&traverse.shape) {
                "Circular"
            } else {
                "Rectangular"
            }
            .into(),
            width: traverse.width,
            height: traverse.height,
            diameter: traverse.diameter,
            rows: traverse.rows,
            columns: traverse.columns,
        }
    }

    fn apply(&self, traverse: &mut Traverse) {
        traverse.shape = self.shape.clone();
        traverse.width = self.width;
        traverse.height = self.height;
        traverse.diameter = self.diameter;
        traverse.rows = self.rows;
        traverse.columns = self.columns;
        traverse.velocities = vec![None; self.rows * self.columns];
    }
}

pub struct TraverseView {
    active_id: String,
    draft: Option<Layout>,
    pending_layout: Option<Layout>,
    selected: usize,
    reading_id: Option<String>,
    reading_search: String,
    input_values: Vec<String>,
    running: bool,
    reduced_motion: bool,
    animation_time: f64,
    clear_confirmation: bool,
    jump_to_samples: bool,
}

impl Default for TraverseView {
    fn default() -> Self {
        Self {
            active_id: String::new(),
            draft: None,
            pending_layout: None,
            selected: 0,
            reading_id: None,
            reading_search: String::new(),
            input_values: Vec::new(),
            running: true,
            reduced_motion: false,
            animation_time: 0.0,
            clear_confirmation: false,
            jump_to_samples: false,
        }
    }
}

impl TraverseView {
    fn sync_inputs(&mut self, traverse: &Traverse) {
        self.input_values = traverse
            .velocities
            .iter()
            .map(|value| value.map(|value| format!("{value:.2}")).unwrap_or_default())
            .collect();
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, traverse: &mut Traverse, readings: &[Reading]) -> bool {
        let viewport_height = ui.available_height();
        egui::ScrollArea::vertical()
            .id_salt("traverse_workspace_scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.contents(ui, traverse, readings, viewport_height)
            })
            .inner
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        traverse: &mut Traverse,
        readings: &[Reading],
        viewport_height: f32,
    ) -> bool {
        let mut changed = false;
        if self.active_id != traverse.id || self.draft.is_none() {
            self.active_id = traverse.id.clone();
            self.draft = Some(Layout::from_traverse(traverse));
            self.pending_layout = None;
            self.selected = 0;
            self.clear_confirmation = false;
            self.jump_to_samples = false;
            self.sync_inputs(traverse);
        }
        if let Ok(total) = sample_count(traverse) {
            if traverse.velocities.len() < total {
                traverse.velocities.resize(total, None);
                self.sync_inputs(traverse);
                changed = true;
            }
            self.selected = self.selected.min(total.saturating_sub(1));
        }
        if self.input_values.len() != traverse.velocities.len() {
            self.sync_inputs(traverse);
        }

        self.summary(ui, traverse);
        ui.add_space(12.0);
        // Keep the complete diagram and its scale visible on short laptop windows.
        let diagram_height = (viewport_height - 185.0).clamp(180.0, 310.0);

        if ui.available_width() > 800.0 {
            ui.columns(2, |columns| {
                self.diagram(&mut columns[0], traverse, diagram_height);
                changed |= self.configuration(&mut columns[1], traverse);
                columns[1].add_space(12.0);
                changed |= self.reading_picker(&mut columns[1], traverse, readings);
            });
        } else {
            self.diagram(ui, traverse, diagram_height);
            ui.add_space(10.0);
            changed |= self.configuration(ui, traverse);
            changed |= self.reading_picker(ui, traverse, readings);
        }
        ui.add_space(14.0);
        changed |= self.sample_table(ui, traverse);
        changed
    }

    fn summary(&self, ui: &mut egui::Ui, traverse: &Traverse) {
        let colors = Palette::for_ui(ui);
        match calculate(traverse) {
            Ok(result) => {
                egui::Frame::new().fill(colors.panel)
                    .corner_radius(10.0).inner_margin(10.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            metric(ui, "DUCT AREA", format!("{:.3} ft²", result.area_sq_ft));
                            ui.separator();
                            metric(ui, if result.filled < result.total { "PARTIAL MEAN" } else { "MEAN VELOCITY" },
                                result.mean_fpm.map(|value| format!("{value:.1} fpm")).unwrap_or_else(|| "— fpm".into()));
                            ui.separator();
                            metric(ui, "VOLUME FLOW", result.flow_cfm.map(|value| format!("{value:.1} CFM")).unwrap_or_else(|| "Awaiting samples".into()));
                            ui.separator();
                            metric(ui, "CAPTURED", format!("{} / {}", result.filled, result.total));
                        });
                        if result.filled < result.total {
                            ui.add_space(6.0);
                            ui.small("Flow is calculated after every point has a valid velocity. Blank points are not zero.");
                        }
                    });
            }
            Err(error) => {
                ui.colored_label(colors.error, error.to_string());
            }
        }
    }

    fn configuration(&mut self, ui: &mut egui::Ui, traverse: &mut Traverse) -> bool {
        let mut changed = false;
        let colors = Palette::for_ui(ui);
        egui::Frame::new().stroke(Stroke::new(1.0, colors.border)).corner_radius(10.0)
            .inner_margin(14.0).show(ui, |ui| {
                ui.strong("Sampling configuration");
                ui.label(egui::RichText::new("EQUAL-AREA SAMPLING").small().strong().color(colors.green));
                ui.add_space(8.0);
                let draft = self.draft.as_mut().unwrap();
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut draft.shape, "Rectangular".to_string(), "Rectangular");
                    ui.selectable_value(&mut draft.shape, "Circular".to_string(), "Circular");
                });
                egui::Grid::new("traverse_layout").spacing([16.0, 8.0]).show(ui, |ui| {
                    if circular(&draft.shape) {
                        ui.label("Inside diameter");
                        ui.add(egui::DragValue::new(&mut draft.diameter).range(0.01..=1200.0).speed(0.1).suffix(" in"));
                        ui.end_row();
                        ui.label("Equal-area rings");
                        ui.add(egui::DragValue::new(&mut draft.rows).range(1..=10));
                        ui.end_row();
                        ui.label("Sectors per ring");
                        ui.add(egui::DragValue::new(&mut draft.columns).range(4..=24));
                        ui.end_row();
                    } else {
                        ui.label("Inside width");
                        ui.add(egui::DragValue::new(&mut draft.width).range(0.01..=1200.0).speed(0.1).suffix(" in"));
                        ui.end_row();
                        ui.label("Inside height");
                        ui.add(egui::DragValue::new(&mut draft.height).range(0.01..=1200.0).speed(0.1).suffix(" in"));
                        ui.end_row();
                        ui.label("Rows × columns");
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut draft.rows).range(1..=20));
                            ui.label("×");
                            ui.add(egui::DragValue::new(&mut draft.columns).range(1..=20));
                        });
                        ui.end_row();
                    }
                });
                ui.add_space(8.0);
                let has_changes = *draft != Layout::from_traverse(traverse)
                    || traverse.velocities.len() != traverse.rows.saturating_mul(traverse.columns);
                if ui.add_enabled(has_changes, egui::Button::new("Apply sampling layout")).clicked() {
                    if traverse.velocities.iter().any(Option::is_some) {
                        self.pending_layout = Some(draft.clone());
                    } else {
                        draft.apply(traverse);
                        changed = true;
                    }
                }
                ui.collapsing("About the sampling layout", |ui| {
                    ui.label(egui::RichText::new(if circular(&draft.shape) {
                        "Each annular sector has equal area. Points use the midpoint in area of each ring and the sector midpoint."
                    } else {
                        "Each rectangular cell has equal area. Sampling points sit at the cell midpoints."
                    }).small().color(colors.muted));
                    ui.label(egui::RichText::new("Verify the measurement layout against your project requirements.").small().color(colors.muted));
                });
            });
        if let Some(layout) = self.pending_layout.clone() {
            ui.add_space(6.0);
            egui::Frame::new().fill(colors.warning).inner_margin(10.0).corner_radius(6.0).show(ui, |ui| {
                ui.label("Applying this layout clears the existing velocities because the sampling positions change.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Clear samples and apply").clicked() {
                        layout.apply(traverse);
                        self.draft = Some(layout.clone());
                        self.pending_layout = None;
                        changed = true;
                    }
                    if ui.button("Keep current layout").clicked() {
                        self.draft = Some(Layout::from_traverse(traverse));
                        self.pending_layout = None;
                    }
                });
            });
        }
        if changed {
            self.selected = 0;
            self.sync_inputs(traverse);
        }
        changed
    }

    fn reading_picker(
        &mut self,
        ui: &mut egui::Ui,
        traverse: &mut Traverse,
        readings: &[Reading],
    ) -> bool {
        let mut changed = false;
        let colors = Palette::for_ui(ui);
        let compatible: Vec<&Reading> = readings
            .iter()
            .filter(|reading| velocity_to_fpm(reading.value, &reading.unit).is_some())
            .collect();
        let selected_reading = compatible
            .iter()
            .find(|reading| Some(&reading.id) == self.reading_id.as_ref())
            .copied();
        ui.strong(format!("Point {:02} · assign a reading", self.selected + 1));
        if compatible.is_empty() {
            ui.label(
                egui::RichText::new(
                    "Import USB readings in fpm or m/s, or enter velocities in the table below.",
                )
                .color(colors.muted),
            );
            return false;
        }
        if compatible.len() > 12 || !self.reading_search.is_empty() {
            ui.add(
                egui::TextEdit::singleline(&mut self.reading_search)
                    .hint_text("Filter velocity readings…")
                    .desired_width(ui.available_width().min(380.0)),
            );
        }
        let needle = self.reading_search.to_lowercase();
        let filtered: Vec<&Reading> = compatible
            .iter()
            .copied()
            .filter(|reading| {
                needle.is_empty()
                    || format!(
                        "{} {} {} {}",
                        reading.group, reading.sensor, reading.kind, reading.timestamp
                    )
                    .to_lowercase()
                    .contains(&needle)
            })
            .collect();
        egui::ComboBox::from_id_salt("traverse_reading")
            .width(ui.available_width().min(380.0))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .selected_text(
                selected_reading
                    .map(|reading| reading_label(reading))
                    .unwrap_or_else(|| "Choose a velocity reading".into()),
            )
            .show_ui(ui, |ui| {
                if filtered.is_empty() {
                    ui.label("No matching velocity readings.");
                }
                egui::ScrollArea::vertical()
                    .id_salt("traverse_reading_choices")
                    .max_height(220.0)
                    .show_rows(ui, 28.0, filtered.len(), |ui, rows| {
                        for index in rows {
                            let reading = filtered[index];
                            let selected = self.reading_id.as_ref() == Some(&reading.id);
                            if ui
                                .add_sized(
                                    [ui.available_width().min(380.0), 28.0],
                                    egui::Button::selectable(selected, reading_label(reading))
                                        .wrap_mode(egui::TextWrapMode::Truncate),
                                )
                                .clicked()
                            {
                                self.reading_id = Some(reading.id.clone());
                            }
                        }
                    });
            });
        let selected_reading = compatible
            .iter()
            .find(|reading| Some(&reading.id) == self.reading_id.as_ref())
            .copied();
        let value =
            selected_reading.and_then(|reading| velocity_to_fpm(reading.value, &reading.unit));
        ui.horizontal_wrapped(|ui| {
            let valid_target = self.selected < traverse.velocities.len();
            if ui
                .add_enabled(
                    value.is_some() && valid_target,
                    egui::Button::new("Assign to point"),
                )
                .clicked()
            {
                traverse.velocities[self.selected] = value;
                changed = true;
            }
            if ui
                .add_enabled(
                    value.is_some() && valid_target,
                    egui::Button::new("Assign + next"),
                )
                .clicked()
            {
                traverse.velocities[self.selected] = value;
                self.selected = (self.selected + 1) % traverse.velocities.len();
                changed = true;
            }
        });
        if let Some(value) = value {
            ui.small(format!(
                "Assigned velocity: {value:.2} fpm. Source reading remains in the explorer."
            ));
        }
        if changed {
            self.sync_inputs(traverse);
        }
        changed
    }

    fn sample_table(&mut self, ui: &mut egui::Ui, traverse: &mut Traverse) -> bool {
        let mut changed = false;
        let colors = Palette::for_ui(ui);
        let positions = point_positions(traverse);
        ui.horizontal_wrapped(|ui| {
            let heading = ui.strong("Point velocities");
            if self.jump_to_samples {
                heading.scroll_to_me(Some(egui::Align::TOP));
                self.jump_to_samples = false;
            }
            ui.label(
                egui::RichText::new("fpm · click a point to select it")
                    .small()
                    .color(colors.muted),
            );
            if ui
                .add_enabled(
                    traverse.velocities.iter().any(Option::is_some)
                        || self.input_values.iter().any(|text| !text.trim().is_empty()),
                    egui::Button::new("Clear samples…"),
                )
                .clicked()
            {
                self.clear_confirmation = true;
            }
        });
        if self.clear_confirmation {
            ui.horizontal_wrapped(|ui| {
                ui.label("Clear every velocity in this traverse?");
                if ui.button("Clear all velocities").clicked() {
                    traverse.velocities.fill(None);
                    self.sync_inputs(traverse);
                    self.clear_confirmation = false;
                    changed = true;
                }
                if ui.button("Keep samples").clicked() {
                    self.clear_confirmation = false;
                }
            });
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("traverse_samples_scroll")
            .max_height(250.0)
            .show(ui, |ui| {
                egui::Grid::new("traverse_samples")
                    .striped(true)
                    .spacing([24.0, 6.0])
                    .min_col_width(70.0)
                    .show(ui, |ui| {
                        ui.strong("Point");
                        ui.strong(if circular(&traverse.shape) {
                            "Ring / sector"
                        } else {
                            "Row / column"
                        });
                        ui.strong("X / Y (in)");
                        ui.strong("Velocity (fpm)");
                        ui.strong("Status");
                        ui.end_row();
                        for (index, position) in positions.iter().enumerate() {
                            if index >= traverse.velocities.len() {
                                break;
                            }
                            if ui
                                .selectable_label(
                                    self.selected == index,
                                    format!("{:02}", index + 1),
                                )
                                .clicked()
                            {
                                self.selected = index;
                            }
                            ui.label(format!(
                                "{} / {}",
                                index / traverse.columns + 1,
                                index % traverse.columns + 1
                            ));
                            let width = if circular(&traverse.shape) {
                                traverse.diameter
                            } else {
                                traverse.width
                            };
                            let height = if circular(&traverse.shape) {
                                traverse.diameter
                            } else {
                                traverse.height
                            };
                            ui.label(format!(
                                "{:.2} / {:.2}",
                                position[0] as f64 * width,
                                position[1] as f64 * height
                            ));
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.input_values[index])
                                    .desired_width(110.0)
                                    .hint_text("—"),
                            );
                            if edit.changed() {
                                let text = self.input_values[index].trim();
                                // Invalid edits clear the sample, withholding flow without placing NaN
                                // into a project that must remain serializable and safe to save.
                                traverse.velocities[index] = text
                                    .parse::<f64>()
                                    .ok()
                                    .filter(|value| value.is_finite() && *value >= 0.0);
                                self.selected = index;
                                changed = true;
                            }
                            match traverse.velocities[index] {
                                Some(value) if value.is_finite() && value >= 0.0 => {
                                    ui.colored_label(colors.green, "Captured");
                                }
                                Some(_) => {
                                    ui.colored_label(colors.error, "Invalid value");
                                }
                                None if !self.input_values[index].trim().is_empty() => {
                                    ui.colored_label(colors.error, "Invalid value");
                                }
                                None => {
                                    ui.colored_label(colors.muted, "Waiting");
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        ui.small("X and Y are measured from the top-left of the duct bounding box. Zero is a valid measured velocity.");
        changed
    }

    fn diagram(&mut self, ui: &mut egui::Ui, traverse: &Traverse, height: f32) {
        let colors = Palette::for_ui(ui);
        let font_family = egui::TextStyle::Body.resolve(ui.style()).family;
        ui.horizontal_wrapped(|ui| {
            ui.strong("Duct velocity profile");
            if ui
                .button(if self.running {
                    "Pause motion"
                } else {
                    "Resume motion"
                })
                .clicked()
            {
                self.running = !self.running;
            }
            ui.checkbox(&mut self.reduced_motion, "Reduced motion");
        });
        let animate = self.running && !self.reduced_motion;
        if animate {
            self.animation_time += ui.input(|input| input.stable_dt.min(0.1) as f64);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
        let (response, painter) = ui.allocate_painter(
            Vec2::new(ui.available_width().max(200.0), height),
            Sense::click(),
        );
        let canvas = response.rect;
        painter.rect_filled(canvas, 10.0, colors.canvas);
        let inner = canvas.shrink2(Vec2::new(38.0, 34.0));
        let is_circular = circular(&traverse.shape);
        let ratio = if is_circular {
            1.0
        } else {
            (traverse.width / traverse.height).clamp(0.2, 5.0) as f32
        };
        let size = if ratio > inner.width() / inner.height() {
            Vec2::new(inner.width(), inner.width() / ratio)
        } else {
            Vec2::new(inner.height() * ratio, inner.height())
        };
        let duct = Rect::from_center_size(inner.center(), size);
        let measured_max = traverse
            .velocities
            .iter()
            .filter_map(|value| *value)
            .filter(|value| value.is_finite() && *value >= 0.0)
            .reduce(f64::max);
        let max_velocity = measured_max.unwrap_or(0.0).max(1.0);
        if sample_count(traverse).is_ok() {
            if is_circular {
                self.draw_circular(&painter, duct, traverse, max_velocity, colors);
            } else {
                self.draw_rectangular(&painter, duct, traverse, max_velocity, colors);
            }
        }
        // Stylized moving marks convey flow direction, not a CFD simulation or measured turbulence.
        for lane in 0..7 {
            for particle in 0..6 {
                let x = ((self.animation_time as f32 * 0.13
                    + particle as f32 / 6.0
                    + lane as f32 * 0.072)
                    % 1.0)
                    .clamp(0.0, 1.0);
                let y = (lane as f32 + 0.5) / 7.0;
                if is_circular && (x - 0.5).powi(2) + (y - 0.5).powi(2) > 0.225 {
                    continue;
                }
                let point = Pos2::new(
                    duct.left() + x * duct.width(),
                    duct.top() + y * duct.height(),
                );
                let end_x = (point.x + 8.0).min(duct.right());
                painter.line_segment(
                    [point, Pos2::new(end_x, point.y)],
                    Stroke::new(1.6, colors.particle),
                );
            }
        }
        let points = point_positions(traverse);
        let hover = response.hover_pos();
        let mut hovered = None;
        let mut closest_distance = f32::INFINITY;
        let rows = traverse.rows.max(1) as f32;
        let columns = traverse.columns.max(1) as f32;
        let point_spacing = if is_circular {
            let duct_radius = duct.width() / 2.0;
            let inner_radius = (0.5 / rows).sqrt() * duct_radius;
            let angular_spacing = if columns > 1.0 {
                2.0 * inner_radius * (std::f32::consts::PI / columns).sin()
            } else {
                duct.width()
            };
            let radial_spacing = if rows > 1.0 {
                (((rows - 0.5) / rows).sqrt() - ((rows - 1.5) / rows).sqrt()) * duct_radius
            } else {
                duct.width()
            };
            angular_spacing.min(radial_spacing)
        } else {
            (duct.width() / columns).min(duct.height() / rows)
        };
        let nominal_radius: f32 = if points.len() <= 36 {
            12.0
        } else if points.len() <= 100 {
            8.0
        } else {
            5.0
        };
        let radius = nominal_radius.min(point_spacing * 0.3).max(2.0);
        for (index, position) in points.iter().enumerate() {
            let point = Pos2::new(
                duct.left() + position[0] * duct.width(),
                duct.top() + position[1] * duct.height(),
            );
            let value = traverse
                .velocities
                .get(index)
                .copied()
                .flatten()
                .filter(|value| value.is_finite() && *value >= 0.0);
            if self.selected == index {
                painter.circle_stroke(point, radius + 4.0, Stroke::new(2.5, colors.green));
            }
            painter.circle_filled(
                point,
                radius,
                if value.is_some() {
                    colors.green
                } else {
                    colors.duct
                },
            );
            painter.circle_stroke(point, radius, Stroke::new(1.5, colors.green));
            if points.len() <= 100 && radius >= 7.0 {
                painter.text(
                    point,
                    egui::Align2::CENTER_CENTER,
                    format!("{}", index + 1),
                    FontId::new((radius * 0.95).clamp(8.0, 11.0), font_family.clone()),
                    if value.is_some() {
                        colors.point_text
                    } else {
                        colors.ink
                    },
                );
            }
            if let Some(position) = hover {
                let distance = position.distance(point);
                if distance <= radius + 6.0 && distance < closest_distance {
                    closest_distance = distance;
                    hovered = Some(index);
                }
            }
        }
        if let Some(index) = hovered {
            if response.clicked() {
                self.selected = index;
            }
            let value = traverse.velocities.get(index).copied().flatten();
            response.on_hover_text(format!(
                "Point {}\n{}",
                index + 1,
                value
                    .map(|value| format!("{value:.2} fpm"))
                    .unwrap_or_else(|| "Awaiting velocity".into())
            ));
        }
        let selected_value = traverse
            .velocities
            .get(self.selected)
            .copied()
            .flatten()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| format!("{value:.2} fpm"))
            .unwrap_or_else(|| "awaiting velocity".to_string());
        painter.text(
            canvas.left_top() + Vec2::new(12.0, 11.0),
            egui::Align2::LEFT_TOP,
            format!("Point {:02} · {selected_value}", self.selected + 1),
            FontId::new(12.0, font_family.clone()),
            colors.ink,
        );
        let dimension = if is_circular {
            format!("Ø {:.2} in", traverse.diameter)
        } else {
            format!("{:.2} × {:.2} in", traverse.width, traverse.height)
        };
        painter.text(
            Pos2::new(canvas.center().x, canvas.bottom() - 14.0),
            egui::Align2::CENTER_CENTER,
            dimension,
            FontId::new(13.0, font_family),
            colors.muted,
        );
        ui.horizontal_wrapped(|ui| {
            for (color, label) in [
                (colors.heat_low, "Lower".to_string()),
                (
                    colors.heat_high,
                    match measured_max {
                        Some(value) if value > 0.0 => format!("Higher · scale 0–{value:.0} fpm"),
                        Some(_) => "All measured velocities: 0 fpm".to_string(),
                        None => "Awaiting measured velocities".to_string(),
                    },
                ),
            ] {
                let (swatch, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
                ui.painter().rect_filled(swatch, 2.0, color);
                ui.painter().rect_stroke(
                    swatch,
                    2.0,
                    Stroke::new(1.0, colors.border),
                    egui::StrokeKind::Inside,
                );
                ui.small(label);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("Illustrative flow · measured cell colors").small().color(colors.muted))
                .on_hover_text("Animation is illustrative. Cell colors represent captured velocity; empty cells are unmeasured.");
            if ui.small_button("Edit velocities ↓").clicked() { self.jump_to_samples = true; }
        });
    }

    fn draw_rectangular(
        &self,
        painter: &egui::Painter,
        duct: Rect,
        traverse: &Traverse,
        max_velocity: f64,
        colors: Palette,
    ) {
        let outline = Stroke::new(2.0, colors.ink);
        let grid = Stroke::new(1.0, colors.grid);
        painter.rect_filled(duct, 0.0, colors.duct);
        for row in 0..traverse.rows {
            for column in 0..traverse.columns {
                let index = row * traverse.columns + column;
                let cell = Rect::from_min_max(
                    Pos2::new(
                        duct.left() + column as f32 / traverse.columns as f32 * duct.width(),
                        duct.top() + row as f32 / traverse.rows as f32 * duct.height(),
                    ),
                    Pos2::new(
                        duct.left() + (column + 1) as f32 / traverse.columns as f32 * duct.width(),
                        duct.top() + (row + 1) as f32 / traverse.rows as f32 * duct.height(),
                    ),
                );
                if let Some(value) = traverse
                    .velocities
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|value| value.is_finite() && *value >= 0.0)
                {
                    painter.rect_filled(cell, 0.0, heat_color(value / max_velocity, colors));
                }
                painter.rect_stroke(cell, 0.0, grid, egui::StrokeKind::Inside);
            }
        }
        painter.rect_stroke(duct, 0.0, outline, egui::StrokeKind::Outside);
    }

    fn draw_circular(
        &self,
        painter: &egui::Painter,
        duct: Rect,
        traverse: &Traverse,
        max_velocity: f64,
        colors: Palette,
    ) {
        let outline = Stroke::new(2.0, colors.ink);
        let grid = Stroke::new(1.0, colors.grid);
        let center = duct.center();
        let radius = duct.width() / 2.0;
        painter.circle_filled(center, radius, colors.duct);
        for ring in 0..traverse.rows {
            let inner_radius = (ring as f32 / traverse.rows as f32).sqrt() * radius;
            let outer_radius = ((ring + 1) as f32 / traverse.rows as f32).sqrt() * radius;
            for sector in 0..traverse.columns {
                let index = ring * traverse.columns + sector;
                let Some(value) = traverse
                    .velocities
                    .get(index)
                    .copied()
                    .flatten()
                    .filter(|value| value.is_finite() && *value >= 0.0)
                else {
                    continue;
                };
                let start = std::f32::consts::TAU * sector as f32 / traverse.columns as f32
                    - std::f32::consts::FRAC_PI_2;
                let sweep = std::f32::consts::TAU / traverse.columns as f32;
                let mut mesh = egui::Mesh::default();
                let color = heat_color(value / max_velocity, colors);
                for step in 0..=16 {
                    let angle = start + sweep * step as f32 / 16.0;
                    let direction = Vec2::angled(angle);
                    mesh.colored_vertex(center + direction * inner_radius, color);
                    mesh.colored_vertex(center + direction * outer_radius, color);
                    if step < 16 {
                        let offset = step * 2;
                        mesh.add_triangle(offset, offset + 1, offset + 2);
                        mesh.add_triangle(offset + 1, offset + 3, offset + 2);
                    }
                }
                painter.add(egui::Shape::mesh(mesh));
            }
            painter.circle_stroke(center, outer_radius, grid);
        }
        for sector in 0..traverse.columns {
            let angle = std::f32::consts::TAU * sector as f32 / traverse.columns as f32
                - std::f32::consts::FRAC_PI_2;
            painter.line_segment([center, center + Vec2::angled(angle) * radius], grid);
        }
        painter.circle_stroke(center, radius, outline);
    }
}

fn reading_label(reading: &Reading) -> String {
    format!(
        "{} · {:.2} {} · {}",
        reading.group, reading.value, reading.unit, reading.sensor
    )
}

fn metric(ui: &mut egui::Ui, label: &str, value: String) {
    let colors = Palette::for_ui(ui);
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .strong()
                .color(colors.muted),
        );
        ui.label(
            egui::RichText::new(value)
                .size(21.0)
                .strong()
                .color(colors.ink),
        );
    });
}

fn heat_color(ratio: f64, colors: Palette) -> Color32 {
    let ratio = ratio.clamp(0.0, 1.0) as f32;
    let blend = |low: f32, high: f32| (low + (high - low) * ratio) as u8;
    Color32::from_rgb(
        blend(colors.heat_low.r() as f32, colors.heat_high.r() as f32),
        blend(colors.heat_low.g() as f32, colors.heat_high.g() as f32),
        blend(colors.heat_low.b() as f32, colors.heat_high.b() as f32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rectangle() -> Traverse {
        Traverse {
            id: "test".into(),
            name: "Test traverse".into(),
            shape: "Rectangular".into(),
            width: 24.0,
            height: 12.0,
            diameter: 12.0,
            rows: 2,
            columns: 2,
            velocities: vec![Some(100.0), Some(200.0), Some(300.0), Some(400.0)],
            point_id: None,
        }
    }

    #[test]
    fn rectangle_uses_inside_area_and_equal_weights() {
        let result = calculate(&rectangle()).unwrap();
        assert_eq!(result.area_sq_ft, 2.0);
        assert_eq!(result.mean_fpm, Some(250.0));
        assert_eq!(result.flow_cfm, Some(500.0));
        assert_eq!(
            point_positions(&rectangle()),
            vec![[0.25, 0.25], [0.75, 0.25], [0.25, 0.75], [0.75, 0.75]]
        );
    }

    #[test]
    fn circular_annular_sectors_have_equal_area_and_weights() {
        let mut traverse = rectangle();
        traverse.shape = "circular".into();
        traverse.columns = 4;
        traverse.velocities = vec![Some(600.0); 8];
        let result = calculate(&traverse).unwrap();
        assert!((result.area_sq_ft - std::f64::consts::PI / 4.0).abs() < 1e-10);
        assert!((result.flow_cfm.unwrap() - 150.0 * std::f64::consts::PI).abs() < 1e-9);
        let positions = point_positions(&traverse);
        assert_eq!(positions.len(), 8);
        for (index, point) in positions.iter().enumerate() {
            let radius_squared = (point[0] - 0.5).powi(2) + (point[1] - 0.5).powi(2);
            let expected = if index < 4 { 0.0625 } else { 0.1875 };
            assert!((radius_squared - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn incomplete_samples_withhold_flow_and_zero_is_a_measurement() {
        let mut traverse = rectangle();
        traverse.velocities = vec![Some(0.0), Some(200.0), None, None];
        let result = calculate(&traverse).unwrap();
        assert_eq!(result.filled, 2);
        assert_eq!(result.mean_fpm, Some(100.0));
        assert_eq!(result.flow_cfm, None);
        traverse.velocities.fill(None);
        assert_eq!(calculate(&traverse).unwrap().mean_fpm, None);
        traverse.velocities = vec![Some(100.0)];
        assert_eq!(calculate(&traverse).unwrap().flow_cfm, None);
    }

    #[test]
    fn invalid_samples_or_geometry_never_produce_a_flow() {
        for invalid in [f64::NAN, f64::INFINITY, -1.0] {
            let mut traverse = rectangle();
            traverse.velocities[1] = Some(invalid);
            assert!(calculate(&traverse).is_err());
        }
        let mut traverse = rectangle();
        traverse.width = 0.0;
        assert!(calculate(&traverse).is_err());
        traverse.width = 24.0;
        traverse.rows = 0;
        assert!(calculate(&traverse).is_err());
        assert!(point_positions(&traverse).is_empty());
    }

    #[test]
    fn velocity_conversion_is_explicit_and_rejects_other_measurements() {
        assert_eq!(velocity_to_fpm(10.0, "FPM"), Some(10.0));
        assert!((velocity_to_fpm(1.0, "m/s").unwrap() - 196.8503937007874).abs() < 1e-10);
        assert_eq!(velocity_to_fpm(100.0, "CFM"), None);
        assert_eq!(velocity_to_fpm(1.0, "in w.c."), None);
        assert_eq!(velocity_to_fpm(-1.0, "fpm"), None);
        assert_eq!(velocity_to_fpm(f64::INFINITY, "m/s"), None);
    }

    #[test]
    fn changing_geometry_clears_all_positional_samples() {
        let mut traverse = rectangle();
        let mut layout = Layout::from_traverse(&traverse);
        layout.columns = 3;
        layout.apply(&mut traverse);
        assert_eq!(traverse.velocities, vec![None; 6]);
        assert_eq!(calculate(&traverse).unwrap().flow_cfm, None);
    }

    #[test]
    fn unsupported_shapes_and_oversized_layouts_are_rejected() {
        let mut traverse = rectangle();
        traverse.shape = "oval".into();
        assert!(calculate(&traverse).is_err());
        assert!(point_positions(&traverse).is_empty());
        traverse.shape = "Rectangular".into();
        traverse.rows = usize::MAX;
        assert!(calculate(&traverse).is_err());
        assert!(point_positions(&traverse).is_empty());
    }
}
