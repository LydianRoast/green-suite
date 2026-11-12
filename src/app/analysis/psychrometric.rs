// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;

#[derive(Debug)]
pub(super) struct PsychState {
    pub metric: calc::PsychMetric,
    pub temperature: Option<String>,
    pub humidity: Option<String>,
    pub temperature_b: Option<String>,
    pub humidity_b: Option<String>,
    pub mass_flow: Option<String>,
    pub pressure: Option<String>,
    pub recorded_pressure: bool,
    pub pressure_pa: f64,
    pub assumed: bool,
    pub pressure_confirmed: bool,
    pub ice_confirmed: bool,
    pub mass_confirmed: bool,
    pub snapshots: bool,
}
impl Default for PsychState {
    fn default() -> Self {
        Self {
            metric: Default::default(),
            temperature: None,
            humidity: None,
            temperature_b: None,
            humidity_b: None,
            mass_flow: None,
            pressure: None,
            recorded_pressure: false,
            pressure_pa: 0.,
            assumed: true,
            pressure_confirmed: false,
            ice_confirmed: false,
            mass_confirmed: false,
            snapshots: false,
        }
    }
}
impl PsychState {
    pub(super) fn form(&mut self, ui: &mut Ui, project: &Project, search: &mut String) -> bool {
        let before = format!(
            "{:?}",
            (
                &self.temperature,
                &self.humidity,
                &self.temperature_b,
                &self.humidity_b,
                &self.mass_flow,
                &self.pressure,
                self.recorded_pressure
            )
        );
        ComboBox::from_id_salt("psychrometric-metric")
            .selected_text(metric_label(self.metric))
            .show_ui(ui, |ui| {
                for metric in [
                    calc::PsychMetric::HumidityRatio,
                    calc::PsychMetric::Enthalpy,
                    calc::PsychMetric::Condensation,
                    calc::PsychMetric::MoistureRemoval,
                ] {
                    ui.selectable_value(&mut self.metric, metric, metric_label(metric));
                }
            });
        let eligible = |quantity| {
            project
                .readings
                .iter()
                .filter(|r| Unit::parse(&r.unit).is_some_and(|u| u.quantity() == quantity))
                .collect::<Vec<_>>()
        };
        let temperatures = eligible(Quantity::Temperature);
        let humidities = eligible(Quantity::RelativeHumidity);
        reading_picker(
            ui,
            "A · Dry bulb",
            &mut self.temperature,
            &temperatures,
            search,
        );
        reading_picker(
            ui,
            "A · Relative humidity",
            &mut self.humidity,
            &humidities,
            search,
        );
        if self.metric == calc::PsychMetric::MoistureRemoval {
            ui.small(
                "A = entering air; B = leaving air. Assign records to their actual locations.",
            );
            reading_picker(
                ui,
                "B · Dry bulb",
                &mut self.temperature_b,
                &temperatures,
                search,
            );
            reading_picker(
                ui,
                "B · Relative humidity",
                &mut self.humidity_b,
                &humidities,
                search,
            );
            reading_picker(
                ui,
                "Dry-air mass flow",
                &mut self.mass_flow,
                &eligible(Quantity::MassFlow),
                search,
            );
            ui.checkbox(
                &mut self.mass_confirmed,
                "Selected flow is mass of dry air per second",
            );
        }
        ui.checkbox(
            &mut self.recorded_pressure,
            "Use a stored barometric-pressure record",
        );
        if self.recorded_pressure {
            reading_picker(
                ui,
                "Absolute barometric pressure",
                &mut self.pressure,
                &eligible(Quantity::Pressure),
                search,
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label("Declared absolute pressure (Pa)");
                ui.add(DragValue::new(&mut self.pressure_pa).speed(10.));
                ui.checkbox(&mut self.assumed, "Assumed");
            });
        }
        ui.checkbox(
            &mut self.pressure_confirmed,
            "Pressure is absolute barometric pressure, rather than gauge or differential",
        );
        ui.collapsing("Subzero convention / model limits",|ui| {
            ui.checkbox(&mut self.ice_confirmed,"At/below 0.01 °C, input RH uses ice equilibrium");
            ui.label("Ideal moist air: −100 to 200 °C; 20 to 120 kPa absolute. Ice below the triple point, water above. No enhancement factor or uncertainty propagation.");
        });
        ui.checkbox(
            &mut self.snapshots,
            "Use separate snapshots (estimated operating point)",
        );
        let after = format!(
            "{:?}",
            (
                &self.temperature,
                &self.humidity,
                &self.temperature_b,
                &self.humidity_b,
                &self.mass_flow,
                &self.pressure,
                self.recorded_pressure
            )
        );
        let changed = before != after;
        if changed {
            self.pressure_confirmed = false;
            self.mass_confirmed = false;
        }
        changed
    }
    pub(super) fn calculate(
        &self,
        project: &Project,
        basis: Basis,
        max_separation_seconds: f64,
        clocks_confirmed: bool,
    ) -> anyhow::Result<calc::PsychResult> {
        let get = |id: &Option<String>, role: &str| {
            id.as_ref()
                .and_then(|id| project.readings.iter().find(|r| &r.id == id))
                .ok_or_else(|| anyhow::anyhow!("Choose {role}"))
        };
        let entering = calc::AirPair {
            temperature: get(&self.temperature, "dry-bulb temperature")?,
            humidity: get(&self.humidity, "relative humidity")?,
            role: if self.metric == calc::PsychMetric::MoistureRemoval {
                "Entering air"
            } else {
                "Selected air state"
            },
        };
        let leaving = if self.metric == calc::PsychMetric::MoistureRemoval {
            Some(calc::AirPair {
                temperature: get(&self.temperature_b, "leaving dry bulb")?,
                humidity: get(&self.humidity_b, "leaving RH")?,
                role: "Leaving air",
            })
        } else {
            None
        };
        let mass = if self.metric == calc::PsychMetric::MoistureRemoval {
            Some(get(&self.mass_flow, "dry-air mass flow")?)
        } else {
            None
        };
        let pressure = if self.recorded_pressure {
            calc::Barometric::Record(get(&self.pressure, "absolute barometric pressure")?)
        } else {
            calc::Barometric::Declared {
                pressure_pa: self.pressure_pa,
                assumed: self.assumed,
            }
        };
        calc::psychrometric(
            entering,
            leaving,
            mass,
            pressure,
            self.metric,
            &calc::PsychOptions {
                basis,
                timing: calc::InputTiming {
                    max_separation_seconds,
                    clocks_confirmed,
                    separate_snapshots: self.snapshots,
                },
                pressure_is_absolute_confirmed: self.pressure_confirmed,
                subzero_rh_ice_confirmed: self.ice_confirmed,
                dry_air_mass_flow_confirmed: self.mass_confirmed,
            },
        )
    }
}
fn metric_label(metric: calc::PsychMetric) -> &'static str {
    match metric {
        calc::PsychMetric::HumidityRatio => "Humidity ratio",
        calc::PsychMetric::Enthalpy => "Enthalpy per kg dry air",
        calc::PsychMetric::Condensation => "Dew / frost point",
        calc::PsychMetric::MoistureRemoval => "Moisture-removal rate",
    }
}
pub(super) fn draw(ui: &mut Ui, result: &calc::PsychResult) {
    ui.label(&result.selected.kind);
    let first = &result.entering;
    ui.label(format!(
        "A: {:.2} °C · {:.1}% RH · {:.3} g/kg dry air · {:.2} kJ/kg dry air",
        first.temperature_c,
        first.relative_humidity_percent,
        first.humidity_ratio * 1000.,
        first.enthalpy_j_kg_dry_air / 1000.
    ));
    ui.label(match first.condensation {
        calc::CondensationPoint::Temperature(t) => format!(
            "{}: {t:.2} °C",
            if t <= 0.01 {
                "Frost point"
            } else {
                "Dew point"
            }
        ),
        calc::CondensationPoint::NoVapour => "Zero RH: no finite dew/frost point".into(),
        calc::CondensationPoint::BelowRange => "Dew/frost point below −100 °C model limit".into(),
    });
    if let Some(second) = &result.leaving {
        ui.label(format!(
            "B: {:.2} °C · {:.1}% RH · {:.3} g/kg dry air",
            second.temperature_c,
            second.relative_humidity_percent,
            second.humidity_ratio * 1000.
        ));
    }
    if let Some(notes) = result
        .selected
        .metadata
        .as_ref()
        .and_then(|m| m.derivation.as_ref())
        .map(|d| &d.assumptions)
    {
        ui.label(format!(
            "Pressure: {:.3} kPa · {}",
            first.pressure_pa / 1000.,
            notes["barometric_pressure_kind"]
        ));
        ui.label(format!(
            "Input span: {} seconds · separate snapshots: {}",
            notes["time_span_seconds"], notes["separate_snapshots_accepted"]
        ));
    }
    chart(ui, result);
    let value = result.selected.value;
    let summary = match Unit::parse(&result.selected.unit).map(Unit::quantity) {
        Some(Quantity::HumidityRatio) => format!("{:.3} g/kg dry air", value * 1000.),
        Some(Quantity::SpecificEnthalpy) => format!("{:.2} kJ/kg dry air", value / 1000.),
        Some(Quantity::Temperature) => format!("{value:.2} °C"),
        Some(Quantity::MassFlow) => format!("{:.3} g/s water", value * 1000.),
        _ => format!("{} {}", calculated_value(value), result.selected.unit),
    };
    ui.heading(summary);
}
fn chart(ui: &mut Ui, result: &calc::PsychResult) {
    ui.small(
        "Air-state diagram · RH contours 10, 30, 50, 70, 90, 100% · ice equilibrium below 0.01 °C",
    );
    let colors = palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 190.), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(44., 10.), rect.max - vec2(12., 30.));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let y_max = 0.03_f64.max(result.entering.humidity_ratio * 1.15).max(
        result
            .leaving
            .as_ref()
            .map_or(0., |s| s.humidity_ratio * 1.15),
    );
    let position = |t: f64, w: f64| {
        pos2(
            plot.left()
                + ((t - result.chart_min_c) / (result.chart_max_c - result.chart_min_c)) as f32
                    * plot.width(),
            plot.bottom() - (w / y_max) as f32 * plot.height(),
        )
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, colors.canvas);
    let clipped = painter.with_clip_rect(plot);
    for curve in &result.curves {
        let mut previous = None;
        for point in &curve.points {
            if let Some((t, w)) = point {
                let next = position(*t, *w);
                if let Some(previous) = previous {
                    clipped.line_segment(
                        [previous, next],
                        Stroke::new(
                            1.,
                            if curve.rh_percent == 100. {
                                colors.secondary
                            } else {
                                colors.grid
                            },
                        ),
                    );
                }
                previous = Some(next);
            } else {
                previous = None;
            }
        }
    }
    for (label, state, color) in [
        ("A", Some(&result.entering), colors.green),
        ("B", result.leaving.as_ref(), colors.secondary),
    ] {
        if let Some(state) = state {
            let point = position(state.temperature_c, state.humidity_ratio);
            clipped.circle_filled(point, 4., color);
            clipped.text(
                point + vec2(6., -6.),
                Align2::LEFT_BOTTOM,
                label,
                FontId::monospace(12.),
                colors.ink,
            );
        }
    }
    painter.line_segment(
        [plot.left_top(), plot.left_bottom()],
        Stroke::new(1., colors.ink),
    );
    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        Stroke::new(1., colors.ink),
    );
    painter.text(
        rect.left_top(),
        Align2::LEFT_TOP,
        format!("{:.1}", y_max * 1000.),
        FontId::monospace(10.),
        colors.ink,
    );
    painter.text(
        plot.left_bottom() + vec2(-5., 0.),
        Align2::RIGHT_BOTTOM,
        "0",
        FontId::monospace(10.),
        colors.ink,
    );
    painter.text(
        plot.left_bottom() + vec2(0., 5.),
        Align2::LEFT_TOP,
        format!("{:.0} °C", result.chart_min_c),
        FontId::monospace(10.),
        colors.ink,
    );
    painter.text(
        plot.right_bottom() + vec2(0., 5.),
        Align2::RIGHT_TOP,
        format!("{:.0} °C", result.chart_max_c),
        FontId::monospace(10.),
        colors.ink,
    );
    painter.text(
        plot.center_bottom() + vec2(0., 5.),
        Align2::CENTER_TOP,
        "Dry bulb · humidity ratio g/kg dry air",
        FontId::monospace(10.),
        colors.ink,
    );
    if let Some(pointer) = response.hover_pos().filter(|p| plot.contains(*p)) {
        let t = result.chart_min_c
            + (pointer.x - plot.left()) as f64 / plot.width() as f64
                * (result.chart_max_c - result.chart_min_c);
        let w = (plot.bottom() - pointer.y) as f64 / plot.height() as f64 * y_max;
        response.on_hover_text(format!(
            "Chart position: {t:.2} °C · {:.3} g/kg dry air",
            w * 1000.
        ));
    }
}
