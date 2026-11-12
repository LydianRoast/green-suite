// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
mod hydronic;
mod psychrometric;
use super::*;
use crate::{
    analysis as calc,
    standards::Basis,
    units::{Quantity, Unit},
};
use std::collections::{BTreeSet, HashMap, HashSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Compare,
    Series,
    Balance,
    Hydronic,
    Psychrometric,
}
enum Output {
    Difference(Reading),
    Series(calc::SeriesResult),
    Balance(calc::BalanceResult),
    Thermal(calc::ThermalResult),
    Valve(Reading),
    Psychrometric(calc::PsychResult),
}
impl Output {
    fn reading(&self) -> Option<&Reading> {
        match self {
            Self::Difference(r) | Self::Valve(r) => Some(r),
            Self::Series(r) => Some(&r.result),
            Self::Balance(r) => r.result.as_ref(),
            Self::Thermal(r) => Some(&r.result),
            Self::Psychrometric(r) => Some(&r.selected),
        }
    }
}

pub(super) struct AnalysisState {
    pub open: bool,
    preview_pending: bool,
    preview_scroll: bool,
    mode: Mode,
    basis: Basis,
    target: Unit,
    a: Option<String>,
    b: Option<String>,
    role_a: String,
    role_b: String,
    max_separation: f64,
    clocks_confirmed: bool,
    before_after: bool,
    accept_unknown_time: bool,
    weighted: bool,
    max_gap: f64,
    series_key: String,
    series_choices: Vec<(String, String)>,
    search: String,
    system: String,
    service: String,
    device_filter: String,
    choices: HashMap<String, Option<String>>,
    excluded: HashSet<String>,
    tolerance: Option<f64>,
    result: Option<Output>,
    error: Option<String>,
    last_key: String,
    cache_project: String,
    cache_revision: u64,
    reading_index: HashMap<String, usize>,
    attach_to: Option<String>,
    hydronic: hydronic::HydronicState,
    psychrometric: psychrometric::PsychState,
}
impl Default for AnalysisState {
    fn default() -> Self {
        Self {
            open: false,
            preview_pending: false,
            preview_scroll: false,
            mode: Mode::Compare,
            basis: Basis::default(),
            target: Unit::Pa,
            a: None,
            b: None,
            role_a: "Upstream / entering".into(),
            role_b: "Downstream / leaving".into(),
            max_separation: 5.,
            clocks_confirmed: false,
            before_after: false,
            accept_unknown_time: false,
            weighted: false,
            max_gap: 60.,
            series_key: String::new(),
            series_choices: Vec::new(),
            search: String::new(),
            system: String::new(),
            service: String::new(),
            device_filter: String::new(),
            choices: HashMap::new(),
            excluded: HashSet::new(),
            tolerance: None,
            result: None,
            error: None,
            last_key: String::new(),
            cache_project: String::new(),
            cache_revision: 0,
            reading_index: HashMap::new(),
            attach_to: None,
            hydronic: Default::default(),
            psychrometric: Default::default(),
        }
    }
}
pub(super) struct SaveAnalysis {
    pub reading: Reading,
    pub point_id: Option<String>,
}

impl AnalysisState {
    fn sync(&mut self, project: &Project, revision: u64) {
        if self.cache_project == project.id && self.cache_revision == revision {
            return;
        }
        self.reading_index = project
            .readings
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id.clone(), i))
            .collect();
        let mut series = std::collections::BTreeMap::new();
        for reading in &project.readings {
            if Unit::parse(&reading.unit).is_some() {
                series.entry(series_key(reading)).or_insert_with(|| {
                    format!("{} · {} · {}", reading.group, reading.sensor, reading.unit)
                });
            }
        }
        self.series_choices = series.into_iter().collect();
        self.cache_project = project.id.clone();
        self.cache_revision = revision;
        self.result = None;
        self.error = None;
        self.last_key.clear();
        self.clocks_confirmed = false;
    }
    fn selected_points<'a>(&self, project: &'a Project) -> Vec<&'a MapPoint> {
        let filter = self.device_filter.to_lowercase();
        project
            .points
            .iter()
            .filter(|p| {
                p.system == self.system
                    && (filter.is_empty() || p.label.to_lowercase().contains(&filter))
            })
            .collect()
    }
    fn calculate(&mut self, project: &Project) {
        let result = (|| -> anyhow::Result<Output> {
            match self.mode {
                Mode::Psychrometric => self
                    .psychrometric
                    .calculate(
                        project,
                        self.basis,
                        self.max_separation,
                        self.clocks_confirmed,
                    )
                    .map(Output::Psychrometric),
                Mode::Hydronic => self.hydronic.calculate(
                    project,
                    self.basis,
                    self.max_separation,
                    self.clocks_confirmed,
                    self.hydronic.snapshots,
                ),
                Mode::Compare => {
                    let a = self
                        .a
                        .as_ref()
                        .and_then(|id| self.reading_index.get(id))
                        .map(|i| &project.readings[*i])
                        .ok_or_else(|| anyhow::anyhow!("Choose input A"))?;
                    let b = self
                        .b
                        .as_ref()
                        .and_then(|id| self.reading_index.get(id))
                        .map(|i| &project.readings[*i])
                        .ok_or_else(|| anyhow::anyhow!("Choose input B"))?;
                    Ok(Output::Difference(calc::difference(
                        a,
                        b,
                        &calc::CompareOptions {
                            target: self.target,
                            basis: self.basis,
                            role_a: self.role_a.clone(),
                            role_b: self.role_b.clone(),
                            max_separation_seconds: self.max_separation,
                            clocks_confirmed: self.clocks_confirmed,
                            before_after: self.before_after,
                            accept_unknown_time: self.accept_unknown_time,
                        },
                    )?))
                }
                Mode::Series => {
                    let readings: Vec<_> = project
                        .readings
                        .iter()
                        .filter(|r| series_key(r) == self.series_key)
                        .collect();
                    Ok(Output::Series(calc::series(
                        &readings,
                        self.target,
                        self.basis,
                        &calc::SeriesOptions {
                            clocks_confirmed: self.clocks_confirmed,
                            time_weighted: self.weighted,
                            max_gap_seconds: self.max_gap,
                        },
                    )?))
                }
                Mode::Balance => {
                    let choices: Vec<_> = self
                        .selected_points(project)
                        .iter()
                        .filter(|p| !self.excluded.contains(&p.id))
                        .map(|p| calc::BalanceChoice {
                            point_id: p.id.clone(),
                            reading_id: self.choices.get(&p.id).cloned().flatten(),
                        })
                        .collect();
                    Ok(Output::Balance(calc::balance(
                        project,
                        &choices,
                        self.target,
                        self.basis,
                        self.tolerance,
                        &self.service,
                    )?))
                }
            }
        })();
        match result {
            Ok(result) => {
                self.result = Some(result);
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error.to_string());
                self.result = None;
            }
        }
    }
    pub(super) fn show(
        &mut self,
        ctx: &Context,
        project: &Project,
        revision: u64,
    ) -> Option<SaveAnalysis> {
        if !self.open {
            return None;
        }
        self.sync(project, revision);
        let mut open = self.open;
        let mut save = None;
        Window::new("Stored-reading analysis").open(&mut open).default_width(720.).default_height(400.)
            .max_height((ctx.content_rect().height()-120.).max(240.)).vscroll(true).show(ctx,|ui| {
            ui.horizontal_wrapped(|ui| {for (mode,label) in [(Mode::Compare,"Compare"),(Mode::Series,"Trend / stability"),(Mode::Balance,"Balance"),(Mode::Hydronic,"Hydronic"),(Mode::Psychrometric,"Air state")] {
                if ui.selectable_value(&mut self.mode,mode,label).changed() {self.result=None;self.error=None;self.clocks_confirmed=false;}
            }});
            ui.add_space(8.);
            ComboBox::from_id_salt("analysis-basis").selected_text(self.basis.label()).show_ui(ui,|ui| {for basis in [Basis::FieldFactors,Basis::PhysicalDefinitions] {ui.selectable_value(&mut self.basis,basis,basis.label());}});
            if self.mode==Mode::Psychrometric {
                if self.psychrometric.form(ui,project,&mut self.search) {self.clocks_confirmed=false;}
                if !self.psychrometric.snapshots {ui.horizontal_wrapped(|ui| {ui.label("Maximum input span (seconds)");ui.add(DragValue::new(&mut self.max_separation).range(0.0..=86400.).speed(1.));});}
                ui.checkbox(&mut self.clocks_confirmed,"I verified the selected clocks are comparable");
            } else if self.mode==Mode::Hydronic {
                if self.hydronic.form(ui, project, &mut self.search) {self.clocks_confirmed=false;}
                if !self.hydronic.valve {
                    ui.checkbox(&mut self.hydronic.snapshots,"Use separate snapshots (estimated operating point)");
                    if !self.hydronic.snapshots {ui.horizontal_wrapped(|ui|{ui.label("Maximum input span (seconds)");ui.add(DragValue::new(&mut self.max_separation).range(0.0..=86400.).speed(1.));});}
                    ui.checkbox(&mut self.clocks_confirmed,"I verified the selected clocks are comparable");
                }
            } else if self.mode==Mode::Compare {
                unit_picker(ui,&mut self.target,|q|matches!(q,Quantity::Pressure|Quantity::Temperature|Quantity::Velocity|Quantity::VolumeFlow));
                let eligible:Vec<_>=project.readings.iter().filter(|r|Unit::parse(&r.unit).is_some_and(|u|u.quantity()==self.target.quantity())).collect();
                reading_picker(ui,"Input A",&mut self.a,&eligible,&mut self.search);
                ui.add(TextEdit::singleline(&mut self.role_a).hint_text("Role of A"));
                reading_picker(ui,"Input B",&mut self.b,&eligible,&mut self.search);
                ui.add(TextEdit::singleline(&mut self.role_b).hint_text("Role of B"));
                ui.label("Result = A minus B. Original timestamps remain unchanged.");
                ui.checkbox(&mut self.before_after,"Before/after snapshots (no simultaneous measurement implied)");
                if self.before_after {ui.checkbox(&mut self.accept_unknown_time,"Accept separate snapshots even if time separation is unknown");}
                if !self.before_after {ui.horizontal_wrapped(|ui|{ui.label("Maximum separation (seconds)");ui.add(DragValue::new(&mut self.max_separation).range(0.0..=86400.).speed(1.));});}
                ui.checkbox(&mut self.clocks_confirmed,"I verified the selected clocks are comparable");
            } else if self.mode==Mode::Series {
                ComboBox::from_id_salt("analysis-series").selected_text(self.series_choices.iter().find(|(id,_)|id==&self.series_key).map(|(_,label)|label.as_str()).unwrap_or("Choose stored series"))
                    .show_ui(ui,|ui| {for (id,label) in &self.series_choices {if ui.selectable_value(&mut self.series_key,id.clone(),label).changed()
                        && let Some(reading)=project.readings.iter().find(|r|series_key(r)==*id)
                        && let Some(unit)=Unit::parse(&reading.unit) {self.target=unit;}}});
                unit_picker(ui,&mut self.target,|_|true);
                ui.checkbox(&mut self.weighted,"Time-weighted mean: hold each sample until the next");
                ui.horizontal_wrapped(|ui|{ui.label("Sampling gap / maximum hold interval (seconds)");ui.add(DragValue::new(&mut self.max_gap).range(0.0..=86400.).speed(1.));});
                ui.checkbox(&mut self.clocks_confirmed,"I verified the series uses comparable clocks");
                ui.label("Uses stored order. Missing and backward timestamps are reported; the last sample has no hold duration.");
            } else {
                let systems:BTreeSet<_>=project.points.iter().map(|p|p.system.as_str()).collect();
                ComboBox::from_id_salt("analysis-system").selected_text(if self.system.is_empty(){"Choose system"}else{&self.system}).show_ui(ui,|ui| {for system in systems {ui.selectable_value(&mut self.system,system.to_owned(),if system.is_empty(){"Unassigned system"}else{system});}});
                ui.horizontal_wrapped(|ui| {
                    if ui.selectable_value(&mut self.service,"Air".into(),"Air").changed() {self.target=Unit::Cfm;}
                    if ui.selectable_value(&mut self.service,"Water".into(),"Water").changed() {self.target=Unit::UsGpm;}
                });
                if self.target.quantity()!=Quantity::VolumeFlow {self.target=if self.service=="Water" {Unit::UsGpm}else{Unit::Cfm};}
                unit_picker(ui,&mut self.target,|q|q==Quantity::VolumeFlow);
                ui.add(TextEdit::singleline(&mut self.device_filter).hint_text("Filter devices by label"));
                let mut use_tolerance=self.tolerance.is_some();
                if ui.checkbox(&mut use_tolerance,"Use a project acceptance tolerance").changed() {self.tolerance=use_tolerance.then_some(10.);}
                if let Some(tolerance)=&mut self.tolerance {ui.horizontal(|ui|{ui.label("Tolerance ± percent of design");ui.add(DragValue::new(tolerance).range(0.0..=1000.).suffix("%"));});}
                ui.label("Choose one mapped flow reading per included device. This is a set of selected snapshots.");
                let points=self.selected_points(project);
                if points.len()>400 {ui.label("More than 400 devices match. Refine the filter to analyze a clearly identified subset.");}
                else {for point in points {
                    ui.push_id(&point.id,|ui| {
                        let mut included=!self.excluded.contains(&point.id);
                        if ui.checkbox(&mut included,format!("{} · design {} {}",point.label,point.design_value,point.unit)).changed() {
                            if included {self.excluded.remove(&point.id);}else{self.excluded.insert(point.id.clone());}
                        }
                        if included {
                            let readings:Vec<_>=point.reading_ids.iter().filter_map(|id|self.reading_index.get(id)).map(|i|&project.readings[*i]).collect();
                            reading_picker(ui,"Mapped reading",self.choices.entry(point.id.clone()).or_default(),&readings,&mut self.search);
                        }
                    });
                }}
            }
            let key=format!("{:?}|{:?}|{:?}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{:?}",self.mode,self.basis,self.target,self.a,self.b,self.role_a,self.role_b,self.max_separation,self.clocks_confirmed,self.before_after,self.weighted,self.max_gap,self.series_key,self.system,self.service,self.tolerance,self.choices,self.excluded)+&self.device_filter;
            // HashMap/HashSet debug order is stable while these collections are
            // unchanged. Any changed parameter invalidates the displayed result.
            let key=key+&format!("|{}|{:?}|{:?}",self.accept_unknown_time,self.hydronic,self.psychrometric);
            if key!=self.last_key {self.result=None;self.error=None;self.last_key=key;}
            ui.add_space(8.);
            if ui.button("Preview calculation").clicked() || self.preview_pending {self.calculate(project);self.preview_pending=false;}
            if let Some(error)=&self.error {ui.label(RichText::new(error).color(palette(ctx).warning));}
            if let Some(output)=&self.result {
                ui.separator();
                match output {
                    Output::Psychrometric(result)=>psychrometric::draw(ui,result),
                    Output::Difference(result) | Output::Valve(result)=> {
                        ui.heading(format!("{} {}",calculated_value(result.value),result.unit));
                        if let Some(notes)=result.metadata.as_ref().and_then(|m|m.derivation.as_ref()).map(|d|&d.assumptions) {
                            if let Some(seconds)=notes.get("time_separation_seconds") {ui.label(format!("Time separation: {seconds} seconds"));}
                            if let Some(unknown)=notes.get("time_separation") {ui.label(unknown);}
                        }
                        if matches!(output,Output::Valve(_)) {ui.label("Estimated from the declared valve coefficient and fluid density; not a measured flow.");}
                    }
                    Output::Thermal(result)=> {
                        ui.heading(format!("{} kW",calculated_value(result.result.value/1000.)));
                        ui.label(format!("Entering − leaving: {} Δ°C · Flow {} L/s",calculated_value(result.delta_c),calculated_value(result.flow_m3_s*1000.)));
                        ui.label(format!("Mean input temperature: {} °C",calculated_value(result.mean_temperature_c)));
                        ui.label("Positive = heat lost by fluid; negative = heat gained. Uses declared constant properties.");
                        if let Some(notes)=result.result.metadata.as_ref().and_then(|m|m.derivation.as_ref()).map(|d|&d.assumptions) {
                            ui.label(format!("Input time span: {} seconds",notes["time_span_seconds"]));
                            ui.label(&notes["time_interpretation"]);
                        }
                    }
                    Output::Series(result)=> {
                        ui.strong(format!("{} samples · mean {} {}",result.count,calculated_value(result.mean),self.target.symbol()));
                        ui.label(format!("Min {} · Max {} · Sample SD {} {}",calculated_value(result.minimum),calculated_value(result.maximum),result.sample_stddev.map(calculated_value).unwrap_or_else(||"N/A".into()),result.dispersion_unit.symbol()));
                        ui.label(format!("Missing times: {} · Backward: {} · Repeated timestamps: {}",result.missing_times,result.backward_steps,result.duplicate_times));
                        ui.label(format!("Largest gap: {}",result.maximum_gap_seconds.map(|v|format!("{v} seconds")).unwrap_or_else(||"Unknown: clocks not established".into())));
                        if !result.clock_established {ui.label("Time alignment is unverified. These are descriptive sample statistics.");}
                        if let Some(mean)=result.time_weighted_mean {ui.strong(format!("Time-weighted mean: {} {}",calculated_value(mean),self.target.symbol()));}
                        stored_trend(ui,result);
                    }
                    Output::Balance(result)=> {
                        ui.strong(format!("{} · selected {} / design {} {}",if result.complete {"Selected device total"}else{"Incomplete: known subtotals only"},calculated_value(result.measured_subtotal),calculated_value(result.design_subtotal),self.target.symbol()));
                        if self.tolerance.is_none() {ui.label("Acceptance tolerance not configured.");}
                        for row in &result.rows {
                            if let Some(issue)=&row.issue {ui.label(RichText::new(format!("{} · {issue}",row.label)).color(palette(ctx).warning));}
                            else {ui.label(format!("{} · {} / {} {} · {}{}",row.label,row.measured.map(calculated_value).unwrap_or_else(||"N/A".into()),row.design.map(calculated_value).unwrap_or_else(||"N/A".into()),self.target.symbol(),row.percent.map(|p|format!("{p:.1}% of design")).unwrap_or_else(||"Design comparison unavailable".into()),match row.within_tolerance {Some(true)=>" · Within tolerance",Some(false)=>" · Outside tolerance",None=>""}));}
                        }
                    }
                }
                if let Some(reading)=output.reading() {
                    if self.attach_to.as_ref().is_some_and(|id|!project.points.iter().any(|p|&p.id==id && Unit::parse(&p.unit).is_some_and(|u|Some(u.quantity())==reading.metadata.as_ref().map(|m|m.quantity)))) {self.attach_to=None;}
                    ComboBox::from_id_salt("analysis-attachment").selected_text(self.attach_to.as_ref().and_then(|id|project.points.iter().find(|p|&p.id==id)).map(|p|p.label.as_str()).unwrap_or("Project only"))
                        .show_ui(ui,|ui| {ui.selectable_value(&mut self.attach_to,None,"Project only");for point in &project.points {
                            if Unit::parse(&point.unit).is_some_and(|u|Some(u.quantity())==reading.metadata.as_ref().map(|m|m.quantity)) {ui.selectable_value(&mut self.attach_to,Some(point.id.clone()),&point.label);}
                        }});
                    if ui.button("Save calculated reading").clicked() {save=Some(SaveAnalysis {reading:reading.clone(),point_id:self.attach_to.clone()});}
                }
            }
            if self.preview_scroll {ui.scroll_to_cursor(Some(Align::BOTTOM));}
        });
        self.open = open;
        save
    }
}

fn series_key(reading: &Reading) -> String {
    serde_json::to_string(&(
        &reading.group,
        &reading.sensor,
        &reading.unit,
        reading
            .metadata
            .as_ref()
            .and_then(|m| m.reporter_serial.as_deref()),
    ))
    .unwrap_or_default()
}

fn stored_trend(ui: &mut Ui, result: &calc::SeriesResult) {
    let colors = palette(ui.ctx());
    ui.label(if result.plot_uses_time {
        "Observed range · elapsed source-clock time"
    } else {
        "Observed range · stored record order (time axis unverified)"
    });
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 130.), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, colors.canvas);
    let plot = rect.shrink2(vec2(12., 20.));
    let scale = result
        .minimum
        .abs()
        .max(result.maximum.abs())
        .max(f64::MIN_POSITIVE);
    let low = result.minimum / scale;
    let range = result.maximum / scale - low;
    let y = |value: f64| {
        if range > f64::EPSILON {
            plot.bottom() - ((value / scale - low) / range) as f32 * plot.height()
        } else {
            plot.center().y
        }
    };
    for &(x, min, max) in &result.plot {
        let x = plot.left() + x as f32 * plot.width();
        let a = pos2(x, y(min));
        let b = pos2(x, y(max));
        if (a.y - b.y).abs() < 1.5 {
            painter.circle_filled(a, 2., colors.green);
        } else {
            painter.line_segment([a, b], Stroke::new(1.5, colors.green));
        }
    }
    painter.text(
        rect.left_top() + vec2(6., 2.),
        Align2::LEFT_TOP,
        format!(
            "{} – {} {}",
            calculated_value(result.minimum),
            calculated_value(result.maximum),
            result.result.unit
        ),
        FontId::proportional(11.),
        colors.ink,
    );
    let (left, right): (String, String) = result
        .duration_seconds
        .map(|s| {
            (
                "0 seconds".into(),
                format!("{} seconds", calculated_value(s)),
            )
        })
        .unwrap_or_else(|| ("Record 1".into(), format!("Record {}", result.count)));
    painter.text(
        rect.left_bottom() + vec2(6., -2.),
        Align2::LEFT_BOTTOM,
        left,
        FontId::proportional(11.),
        colors.ink,
    );
    painter.text(
        rect.right_bottom() + vec2(-6., -2.),
        Align2::RIGHT_BOTTOM,
        right,
        FontId::proportional(11.),
        colors.ink,
    );
    ui.label("Plot buckets preserve observed minima and maxima. Blank intervals are not interpolated; statistics use all selected records.");
}
fn unit_picker(ui: &mut Ui, target: &mut Unit, allow: impl Fn(Quantity) -> bool) {
    ComboBox::from_id_salt("analysis-output-unit")
        .selected_text(target.symbol())
        .show_ui(ui, |ui| {
            for &unit in Unit::ALL {
                if allow(unit.quantity()) {
                    ui.selectable_value(target, unit, unit.symbol());
                }
            }
        });
}
fn reading_picker(
    ui: &mut Ui,
    label: &str,
    selected: &mut Option<String>,
    readings: &[&Reading],
    search: &mut String,
) {
    let current = selected
        .as_ref()
        .and_then(|id| readings.iter().find(|r| &r.id == id));
    ComboBox::from_id_salt(label)
        .width(ui.available_width().min(520.))
        .truncate()
        .selected_text(
            current
                .map(|r| {
                    format!(
                        "{}: {} {} · {} · {}",
                        label,
                        reading_value(r),
                        r.unit,
                        r.metadata
                            .as_ref()
                            .map(|m| m.origin.label())
                            .unwrap_or("Unknown origin"),
                        r.group
                    )
                })
                .unwrap_or_else(|| label.into()),
        )
        .show_ui(ui, |ui| {
            ui.add(TextEdit::singleline(search).hint_text("Find record, sensor or timestamp"));
            ui.selectable_value(selected, None, "Choose a reading");
            let needle = search.to_lowercase();
            let mut shown = 0;
            for reading in readings {
                if !needle.is_empty()
                    && !format!(
                        "{} {} {} {} {}",
                        reading.group,
                        reading.sensor,
                        reading.timestamp,
                        reading.id,
                        reading_value(reading)
                    )
                    .to_lowercase()
                    .contains(&needle)
                {
                    continue;
                }
                if shown >= 200 {
                    ui.label("Refine the search to see more matching readings.");
                    break;
                }
                ui.selectable_value(
                    selected,
                    Some(reading.id.clone()),
                    format!(
                        "{} · {} · {} {} · {}",
                        reading.group,
                        reading.sensor,
                        reading_value(reading),
                        reading.unit,
                        reading.timestamp
                    ),
                );
                shown += 1;
            }
        })
        .response
        .on_hover_text(
            current
                .map(|r| {
                    format!(
                        "{label}: {} {} · {} · {} · {} · {}",
                        reading_value(r),
                        r.unit,
                        r.group,
                        r.timestamp,
                        r.sensor,
                        r.kind
                    )
                })
                .unwrap_or_default(),
        );
}

impl GreenSuite {
    pub(super) fn preview_stored_analysis(&mut self, view: &str) {
        self.view = View::Readings;
        if view.starts_with("analysis-air") || view.starts_with("analysis-moisture") {
            for (id, value, unit) in [
                ("t", 25., "°C"),
                ("r", 50., "%RH"),
                ("tb", 15., "°C"),
                ("rb", 40., "%RH"),
                ("m", 2., "kg/s"),
            ] {
                self.project.readings.push(Reading {
                    id: format!("example-air-{id}"),
                    value,
                    unit: unit.into(),
                    timestamp: "2026-01-01T12:00:00Z".into(),
                    group: "Example air process".into(),
                    sensor: format!("Illustrative {id}"),
                    source: "example".into(),
                    ..Default::default()
                });
            }
            self.stored_analysis.mode = Mode::Psychrometric;
            let state = &mut self.stored_analysis.psychrometric;
            state.temperature = Some("example-air-t".into());
            state.humidity = Some("example-air-r".into());
            state.temperature_b = Some("example-air-tb".into());
            state.humidity_b = Some("example-air-rb".into());
            state.mass_flow = Some("example-air-m".into());
            state.pressure_pa = 101325.;
            state.assumed = true;
            state.pressure_confirmed = true;
            state.mass_confirmed = true;
            if view.starts_with("analysis-moisture") {
                state.metric = calc::PsychMetric::MoistureRemoval;
            }
        } else if view.starts_with("analysis-heat") || view.starts_with("analysis-valve") {
            for (id, value, unit) in [
                ("q", 1., "L/s"),
                ("e", 30., "°C"),
                ("l", 20., "°C"),
                ("p", 1., "psi"),
            ] {
                self.project.readings.push(Reading {
                    id: format!("example-hydronic-{id}"),
                    value,
                    unit: unit.into(),
                    timestamp: "2026-01-01T12:00:00Z".into(),
                    group: "Example hydronic circuit".into(),
                    sensor: format!("Illustrative {id}"),
                    source: "example".into(),
                    ..Default::default()
                });
            }
            self.stored_analysis.mode = Mode::Hydronic;
            let state = &mut self.stored_analysis.hydronic;
            state.flow = Some("example-hydronic-q".into());
            state.entering = Some("example-hydronic-e".into());
            state.leaving = Some("example-hydronic-l".into());
            state.pressure = Some("example-hydronic-p".into());
            state.fluid = "Example liquid · declared properties".into();
            state.temperature_c = 25.;
            state.density = 1000.;
            state.specific_heat = 4180.;
            state.properties_confirmed = true;
            state.coefficient = 2.;
            state.configuration = "EXAMPLE valve · declared size · setting 2".into();
            state.valve_confirmed = true;
            state.valve = view.starts_with("analysis-valve");
            if state.valve {
                state.density = 999.016;
                state.temperature_c = 15.5556;
            }
        } else if view == "analysis-balance" || view == "analysis-incomplete" {
            self.stored_analysis.mode = Mode::Balance;
            self.stored_analysis.system = "AHU-01".into();
            self.stored_analysis.service = "Air".into();
            self.stored_analysis.target = Unit::Cfm;
            for point in &self.project.points {
                if point.system == "AHU-01" {
                    self.stored_analysis
                        .choices
                        .insert(point.id.clone(), point.reading_ids.first().cloned());
                }
            }
            if view == "analysis-incomplete"
                && let Some(point) = self.project.points.iter().find(|p| p.system == "AHU-01")
            {
                self.stored_analysis.choices.insert(point.id.clone(), None);
            }
        } else {
            for (index, value) in [1.25, 1., 1.1].into_iter().enumerate() {
                self.project.readings.push(Reading {
                    id: format!("example-analysis-{index}"),
                    value,
                    unit: "inW".into(),
                    timestamp: format!("2026-01-01T12:00:{:02}Z", index * 2),
                    group: "Example pressure".into(),
                    sensor: "Illustrative pressure".into(),
                    source: "example".into(),
                    ..Default::default()
                });
            }
            self.stored_analysis.target = Unit::InWaterColumn;
            if view == "analysis-series" {
                self.stored_analysis.mode = Mode::Series;
                self.stored_analysis.series_key = series_key(self.project.readings.last().unwrap());
            } else {
                self.stored_analysis.a = Some("example-analysis-0".into());
                self.stored_analysis.b = Some("example-analysis-1".into());
            }
        }
        self.stored_analysis.open = true;
        self.stored_analysis.preview_pending = true;
        self.stored_analysis.preview_scroll = !view.ends_with("-form");
    }
    pub(super) fn open_analysis(&mut self) {
        self.stored_analysis.open = true;
        if let Some(reading) = self
            .selected_reading
            .as_ref()
            .and_then(|id| self.project.readings.iter().find(|r| &r.id == id))
        {
            self.stored_analysis.a = Some(reading.id.clone());
            if let Some(unit) = Unit::parse(&reading.unit) {
                self.stored_analysis.target = unit;
            }
        }
    }
    pub(super) fn stored_analysis_window(&mut self, ctx: &Context) {
        if let Some(action) = self
            .stored_analysis
            .show(ctx, &self.project, self.project_revision)
        {
            let result = (|| -> anyhow::Result<()> {
                calc::validate_inputs(&action.reading, &self.project)?;
                if let Some(id) = &action.point_id {
                    let point = self
                        .project
                        .points
                        .iter()
                        .find(|p| &p.id == id)
                        .ok_or_else(|| anyhow::anyhow!("Selected attachment no longer exists"))?;
                    anyhow::ensure!(
                        Unit::parse(&point.unit).is_some_and(|u| Some(u.quantity())
                            == action.reading.metadata.as_ref().map(|m| m.quantity)),
                        "Attachment quantity is incompatible"
                    );
                }
                if self.project.merge_readings(vec![action.reading.clone()])? > 0 {
                    if let Some(point) = action
                        .point_id
                        .and_then(|id| self.project.points.iter_mut().find(|p| p.id == id))
                    {
                        point.reading_ids.push(action.reading.id.clone());
                    }
                    self.selected_reading = Some(action.reading.id);
                    self.changed();
                }
                Ok(())
            })();
            match result {
                Ok(()) => self.message("Calculated reading saved with its selected inputs.", false),
                Err(error) => self.message(error.to_string(), true),
            }
        }
    }
}
