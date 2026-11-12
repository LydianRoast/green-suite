// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;

#[derive(Debug)]
pub(super) struct HydronicState {
    pub valve: bool,
    pub snapshots: bool,
    pub flow: Option<String>,
    pub entering: Option<String>,
    pub leaving: Option<String>,
    pub pressure: Option<String>,
    pub fluid: String,
    pub temperature_c: f64,
    pub density: f64,
    pub specific_heat: f64,
    pub properties_confirmed: bool,
    pub coefficient: f64,
    pub coefficient_kind: calc::ValveCoefficient,
    pub configuration: String,
    pub valve_confirmed: bool,
    pub output_unit: Unit,
}
impl Default for HydronicState {
    fn default() -> Self {
        Self {
            valve: false,
            snapshots: false,
            flow: None,
            entering: None,
            leaving: None,
            pressure: None,
            fluid: String::new(),
            temperature_c: 0.,
            density: 0.,
            specific_heat: 0.,
            properties_confirmed: false,
            coefficient: 0.,
            coefficient_kind: calc::ValveCoefficient::Cv,
            configuration: String::new(),
            valve_confirmed: false,
            output_unit: Unit::UsGpm,
        }
    }
}
impl HydronicState {
    pub(super) fn form(&mut self, ui: &mut Ui, project: &Project, search: &mut String) -> bool {
        let selected_before = (
            &self.flow,
            &self.entering,
            &self.leaving,
            &self.pressure,
            self.valve,
        );
        let selected_before = format!("{selected_before:?}");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.valve, false, "Heat transfer");
            ui.selectable_value(&mut self.valve, true, "Valve flow");
        });
        if self.valve {
            let pressure: Vec<_> = project
                .readings
                .iter()
                .filter(|r| {
                    Unit::parse(&r.unit).is_some_and(|u| u.quantity() == Quantity::Pressure)
                })
                .collect();
            reading_picker(
                ui,
                "Valve pressure drop",
                &mut self.pressure,
                &pressure,
                search,
            );
            ui.label("Valve identity, size and setting");
            ui.add(
                TextEdit::singleline(&mut self.configuration)
                    .desired_width(ui.available_width())
                    .hint_text("Valve identifier, size and actual setting"),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("Coefficient");
                ComboBox::from_id_salt("hydronic-coefficient")
                    .selected_text(format!("{:?}", self.coefficient_kind))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.coefficient_kind,
                            calc::ValveCoefficient::Cv,
                            "Cv · US gpm at 1 psi",
                        );
                        ui.selectable_value(
                            &mut self.coefficient_kind,
                            calc::ValveCoefficient::Kv,
                            "Kv · m³/h at 1 bar",
                        );
                    });
                ui.add(DragValue::new(&mut self.coefficient).speed(0.1));
                ComboBox::from_id_salt("hydronic-valve-unit")
                    .selected_text(self.output_unit.symbol())
                    .show_ui(ui, |ui| {
                        for unit in [Unit::UsGpm, Unit::LitresPerSecond, Unit::CubicMetresPerHour] {
                            ui.selectable_value(&mut self.output_unit, unit, unit.symbol());
                        }
                    });
            });
            ui.checkbox(&mut self.valve_confirmed,"Record is valve differential pressure; coefficient matches setting; turbulent, non-choked liquid; no cavitation/piping correction");
            ui.small("Use a coefficient for the actual setting. No automatic curve interpolation or valve adjustment.");
            ui.small(match self.coefficient_kind {
                calc::ValveCoefficient::Cv => {
                    "Cv reference density: 999.016 kg/m³ (water at 60°F)."
                }
                calc::ValveCoefficient::Kv => "Kv reference density: 1000 kg/m³.",
            });
        } else {
            let flows: Vec<_> = project
                .readings
                .iter()
                .filter(|r| {
                    Unit::parse(&r.unit).is_some_and(|u| u.quantity() == Quantity::VolumeFlow)
                })
                .collect();
            let temperatures: Vec<_> = project
                .readings
                .iter()
                .filter(|r| {
                    Unit::parse(&r.unit).is_some_and(|u| u.quantity() == Quantity::Temperature)
                })
                .collect();
            reading_picker(ui, "Liquid flow", &mut self.flow, &flows, search);
            reading_picker(
                ui,
                "Entering temperature",
                &mut self.entering,
                &temperatures,
                search,
            );
            reading_picker(
                ui,
                "Leaving temperature",
                &mut self.leaving,
                &temperatures,
                search,
            );
        }
        ui.label("Fluid / mixture");
        ui.add(
            TextEdit::singleline(&mut self.fluid)
                .desired_width(ui.available_width())
                .hint_text("Fluid and mixture, e.g. water or declared glycol concentration"),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("Operating °C");
            ui.add(DragValue::new(&mut self.temperature_c).speed(0.1));
            ui.label("Density kg/m³");
            ui.add(DragValue::new(&mut self.density).speed(1.));
            if !self.valve {
                ui.label("Specific heat J/(kg·K)");
                ui.add(DragValue::new(&mut self.specific_heat).speed(1.));
            }
        });
        ui.checkbox(&mut self.properties_confirmed,"Single-phase liquid; declared properties suit these operating temperatures and mixture");
        ui.small("No water/glycol properties are assumed. Estimate only; steady conditions and instrument suitability require verification.");
        let selected_after = format!(
            "{:?}",
            (
                &self.flow,
                &self.entering,
                &self.leaving,
                &self.pressure,
                self.valve
            )
        );
        let changed = selected_before != selected_after;
        if changed {
            self.properties_confirmed = false;
            self.valve_confirmed = false;
        }
        changed
    }
    pub(super) fn calculate(
        &self,
        project: &Project,
        basis: Basis,
        max_separation_seconds: f64,
        clocks_confirmed: bool,
        separate_snapshots: bool,
    ) -> anyhow::Result<Output> {
        let selected = |id: &Option<String>, role: &str| -> anyhow::Result<&Reading> {
            id.as_ref()
                .and_then(|id| project.readings.iter().find(|r| &r.id == id))
                .ok_or_else(|| anyhow::anyhow!("Choose {role}"))
        };
        let fluid = calc::Fluid {
            description: self.fluid.clone(),
            operating_temperature_c: self.temperature_c,
            density_kg_m3: self.density,
            properties_confirmed: self.properties_confirmed,
        };
        if self.valve {
            Ok(Output::Valve(calc::valve_flow(
                selected(&self.pressure, "valve pressure drop")?,
                &calc::ValveOptions {
                    fluid,
                    coefficient: self.coefficient,
                    coefficient_kind: self.coefficient_kind,
                    valve_configuration: self.configuration.clone(),
                    applicability_confirmed: self.valve_confirmed,
                    basis,
                    output_unit: self.output_unit,
                },
            )?))
        } else {
            Ok(Output::Thermal(calc::heat_rate(
                selected(&self.flow, "liquid flow")?,
                selected(&self.entering, "entering temperature")?,
                selected(&self.leaving, "leaving temperature")?,
                &calc::ThermalOptions {
                    fluid,
                    specific_heat_j_kg_k: self.specific_heat,
                    basis,
                    timing: calc::HydronicTiming {
                        max_separation_seconds,
                        clocks_confirmed,
                        separate_snapshots,
                    },
                },
            )?))
        }
    }
}
