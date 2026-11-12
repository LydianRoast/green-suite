// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Single-phase sensible heat and non-choked liquid valve estimates.
use super::*;

#[derive(Clone, Debug)]
pub struct Fluid {
    pub description: String,
    pub operating_temperature_c: f64,
    pub density_kg_m3: f64,
    pub properties_confirmed: bool,
}
impl Fluid {
    fn notes(&self) -> Result<BTreeMap<String, String>> {
        ensure!(
            !self.description.trim().is_empty(),
            "Identify the fluid and mixture"
        );
        ensure!(
            self.operating_temperature_c.is_finite() && self.operating_temperature_c >= -273.15,
            "Declare a valid operating temperature in °C"
        );
        ensure!(
            self.density_kg_m3.is_finite() && self.density_kg_m3 > 0.,
            "Fluid density must be finite and positive"
        );
        ensure!(
            self.properties_confirmed,
            "Confirm single-phase liquid operation and properties appropriate to the operating conditions"
        );
        Ok(BTreeMap::from([
            ("fluid".into(), self.description.clone()),
            (
                "operating_temperature_c".into(),
                self.operating_temperature_c.to_string(),
            ),
            ("density_kg_m3".into(), self.density_kg_m3.to_string()),
            ("fluid_properties_confirmed_by_user".into(), "true".into()),
        ]))
    }
}

#[derive(Clone, Debug)]
pub struct HydronicTiming {
    pub max_separation_seconds: f64,
    pub clocks_confirmed: bool,
    pub separate_snapshots: bool,
}
impl HydronicTiming {
    pub(super) fn notes(&self, inputs: &[&Reading]) -> Result<BTreeMap<String, String>> {
        ensure!(
            self.max_separation_seconds.is_finite() && self.max_separation_seconds >= 0.,
            "Time tolerance must be finite and nonnegative"
        );
        let stamps: Result<Vec<_>> = inputs.iter().map(|r| stamp(r)).collect();
        let span = stamps.ok().and_then(|stamps| {
            let first = stamps.first()?;
            if !stamps
                .iter()
                .all(|s| comparable(first, s, self.clocks_confirmed))
            {
                return None;
            }
            let earliest = stamps.iter().map(|s| s.time).min()?;
            let latest = stamps.iter().map(|s| s.time).max()?;
            Some((latest - earliest).num_milliseconds() as f64 / 1000.)
        });
        if !self.separate_snapshots {
            ensure!(
                span.is_some(),
                "Input measurement times or comparable clocks are not established; review the records or explicitly accept separate snapshots"
            );
            ensure!(
                span.unwrap() <= self.max_separation_seconds,
                "Inputs exceed the configured time-pairing tolerance"
            );
        }
        Ok(BTreeMap::from([
            ("time_span_seconds".into(), span.map(|s| s.to_string()).unwrap_or_else(|| "Unknown".into())),
            ("time_tolerance_seconds".into(), self.max_separation_seconds.to_string()),
            ("clocks_confirmed_by_user".into(), self.clocks_confirmed.to_string()),
            ("separate_snapshots_accepted".into(), self.separate_snapshots.to_string()),
            ("time_interpretation".into(), if self.separate_snapshots {
                "Separate snapshots; estimated operating point; not established simultaneous measurements"
            } else { "Selected measurement times within declared tolerance; steady conditions still require user verification" }.into()),
        ]))
    }
}

pub struct ThermalOptions {
    pub fluid: Fluid,
    pub specific_heat_j_kg_k: f64,
    pub timing: HydronicTiming,
    pub basis: Basis,
}
pub struct ThermalResult {
    pub result: Reading,
    pub flow_m3_s: f64,
    pub delta_c: f64,
    pub mean_temperature_c: f64,
}
pub fn heat_rate(
    flow: &Reading,
    entering: &Reading,
    leaving: &Reading,
    options: &ThermalOptions,
) -> Result<ThermalResult> {
    let mut notes = options.fluid.notes()?;
    ensure!(
        options.specific_heat_j_kg_k.is_finite() && options.specific_heat_j_kg_k > 0.,
        "Specific heat must be finite and positive"
    );
    let volume = value_in(flow, Unit::CubicMetresPerSecond, options.basis)?;
    ensure!(
        volume >= 0.,
        "Flow must be nonnegative; identify entering and leaving temperatures by the actual flow direction"
    );
    let te = value_in(entering, Unit::Celsius, options.basis)?;
    let tl = value_in(leaving, Unit::Celsius, options.basis)?;
    notes.extend(options.timing.notes(&[flow, entering, leaving])?);
    let delta_c = te - tl;
    let mean_temperature_c = te / 2. + tl / 2.;
    let watts = options.fluid.density_kg_m3 * options.specific_heat_j_kg_k * volume * delta_c;
    notes.extend([
        ("equation".into(), "W = density_kg_m3 × specific_heat_J_kg_K × flow_m3_s × (entering_C - leaving_C)".into()),
        ("sign".into(), "Positive = sensible heat lost by fluid; negative = gained by fluid".into()),
        ("specific_heat_j_kg_k".into(), options.specific_heat_j_kg_k.to_string()),
        ("mean_input_temperature_c".into(), mean_temperature_c.to_string()),
        ("flow_m3_s".into(), volume.to_string()),
        ("delta_c".into(), delta_c.to_string()),
        ("input_roles".into(), serde_json::to_string(&[("flow", &flow.id), ("entering", &entering.id), ("leaving", &leaving.id)])?),
        ("calculation_basis".into(), options.basis.label().into()),
        ("unit_rule_version".into(), options.basis.rule_id().into()),
        ("applicability".into(), "User-declared constant properties appropriate to the temperature range; steady single-phase sensible heat only; excludes latent heat, energy integration and uncertainty propagation".into()),
    ]);
    let result = derived(
        &flow.group,
        "Hydronic heat rate estimate",
        watts,
        Unit::Watt,
        "liquid-sensible-heat-v1",
        &[flow, entering, leaving],
        notes,
    )?;
    Ok(ThermalResult {
        result,
        flow_m3_s: volume,
        delta_c,
        mean_temperature_c,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValveCoefficient {
    Cv,
    Kv,
}
pub struct ValveOptions {
    pub fluid: Fluid,
    pub coefficient: f64,
    pub coefficient_kind: ValveCoefficient,
    pub valve_configuration: String,
    pub applicability_confirmed: bool,
    pub basis: Basis,
    pub output_unit: Unit,
}
pub fn valve_flow(pressure_drop: &Reading, options: &ValveOptions) -> Result<Reading> {
    let mut notes = options.fluid.notes()?;
    ensure!(
        options.coefficient.is_finite() && options.coefficient > 0.,
        "Valve coefficient must be finite and positive"
    );
    ensure!(
        !options.valve_configuration.trim().is_empty(),
        "Identify the valve, size and actual setting for this coefficient"
    );
    ensure!(
        options.applicability_confirmed,
        "Confirm the coefficient matches the valve setting and the liquid flow is turbulent, non-choked and non-cavitating"
    );
    ensure!(
        options.output_unit.quantity() == Quantity::VolumeFlow,
        "Choose a volume-flow output unit"
    );
    let dp = value_in(pressure_drop, Unit::Pa, options.basis)?;
    ensure!(
        dp >= 0.,
        "Pressure drop must be nonnegative; verify pressure-port roles rather than taking the absolute value"
    );
    let (flow, native_unit, reference_density, equation) = match options.coefficient_kind {
        ValveCoefficient::Cv => (
            options.coefficient
                * (value_in(pressure_drop, Unit::Psi, options.basis)?
                    / (options.fluid.density_kg_m3 / 999.016))
                    .sqrt(),
            Unit::UsGpm,
            999.016,
            "US gpm = Cv × sqrt(delta_psi / (density_kg_m3 / 999.016))",
        ),
        ValveCoefficient::Kv => (
            options.coefficient * ((dp / 100000.) / (options.fluid.density_kg_m3 / 1000.)).sqrt(),
            Unit::CubicMetresPerHour,
            1000.,
            "m³/h = Kv × sqrt(delta_bar / (density_kg_m3 / 1000))",
        ),
    };
    let output = options
        .basis
        .calculate(flow, native_unit, options.output_unit)?
        .0;
    notes.extend([
        ("coefficient_kind".into(), format!("{:?}", options.coefficient_kind)),
        ("coefficient".into(), options.coefficient.to_string()),
        ("valve_configuration".into(), options.valve_configuration.clone()),
        ("equation".into(), equation.into()),
        ("reference_density_kg_m3".into(), reference_density.to_string()),
        ("pressure_drop_pa".into(), dp.to_string()),
        ("input_role".into(), "Selected record represents differential pressure across the valve".into()),
        ("calculation_basis".into(), options.basis.label().into()),
        ("unit_rule_version".into(), options.basis.rule_id().into()),
        ("applicability_confirmed_by_user".into(), "true".into()),
        ("applicability".into(), "User-supplied coefficient for actual setting; turbulent non-choked single-phase liquid, no cavitation or piping correction. Gas, steam and automatic curve interpolation are unsupported. Zero pressure drop returns zero estimated flow, not verified shutoff.".into()),
    ]);
    derived(
        &pressure_drop.group,
        "Valve flow estimate",
        output,
        options.output_unit,
        "liquid-valve-coefficient-v1",
        &[pressure_drop],
        notes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record(id: &str, value: f64, unit: &str, second: u32) -> Reading {
        Reading {
            id: id.into(),
            value,
            unit: unit.into(),
            timestamp: format!("2026-01-01T12:00:{second:02}Z"),
            ..Default::default()
        }
    }
    fn fluid() -> Fluid {
        Fluid {
            description: "Declared example liquid".into(),
            operating_temperature_c: 25.,
            density_kg_m3: 1000.,
            properties_confirmed: true,
        }
    }
    fn thermal() -> ThermalOptions {
        ThermalOptions {
            fluid: fluid(),
            specific_heat_j_kg_k: 4180.,
            basis: Basis::PhysicalDefinitions,
            timing: HydronicTiming {
                max_separation_seconds: 3.,
                clocks_confirmed: false,
                separate_snapshots: false,
            },
        }
    }
    fn valve() -> ValveOptions {
        ValveOptions {
            fluid: fluid(),
            coefficient: 2.,
            coefficient_kind: ValveCoefficient::Kv,
            valve_configuration: "Example valve, declared size, setting 2".into(),
            applicability_confirmed: true,
            basis: Basis::PhysicalDefinitions,
            output_unit: Unit::CubicMetresPerHour,
        }
    }
    #[test]
    fn sensible_heat_uses_explicit_properties_units_and_signed_temperature_difference() {
        let flow = record("q", 1., "L/s", 0);
        let entering = record("e", 86., "°F", 1);
        let leaving = record("l", 20., "°C", 2);
        let mut options = thermal();
        let result = heat_rate(&flow, &entering, &leaving, &options).unwrap();
        assert!((result.result.value - 41800.).abs() < 1e-7);
        assert!((result.mean_temperature_c - 25.).abs() < 1e-10);
        assert!((result.delta_c - 10.).abs() < 1e-10);
        assert!(
            (heat_rate(&flow, &leaving, &entering, &options)
                .unwrap()
                .result
                .value
                + 41800.)
                .abs()
                < 1e-7
        );
        options.fluid.density_kg_m3 = 1050.;
        options.specific_heat_j_kg_k = 3600.;
        assert!(
            (heat_rate(&flow, &entering, &leaving, &options)
                .unwrap()
                .result
                .value
                - 37800.)
                .abs()
                < 1e-7
        );
        let zero = record("z", 0., "US gpm", 0);
        assert_eq!(
            heat_rate(&zero, &entering, &leaving, &options)
                .unwrap()
                .result
                .value,
            0.
        );
    }
    #[test]
    fn thermal_requires_all_input_times_or_explicit_snapshot_acceptance() {
        let mut flow = record("q", 1., "L/s", 0);
        let entering = record("e", 30., "°C", 2);
        let leaving = record("l", 20., "°C", 4);
        let mut options = thermal();
        assert!(heat_rate(&flow, &entering, &leaving, &options).is_err());
        options.timing.max_separation_seconds = 4.;
        assert!(heat_rate(&flow, &entering, &leaving, &options).is_ok());
        flow.timestamp.clear();
        assert!(heat_rate(&flow, &entering, &leaving, &options).is_err());
        options.timing.clocks_confirmed = true;
        assert!(heat_rate(&flow, &entering, &leaving, &options).is_err());
        options.timing.separate_snapshots = true;
        let result = heat_rate(&flow, &entering, &leaving, &options)
            .unwrap()
            .result;
        assert_eq!(
            result.metadata.unwrap().derivation.unwrap().assumptions["time_span_seconds"],
            "Unknown"
        );
        flow.timestamp = "2026-01-01T12:00:00Z".into();
        flow.source = "manual".into();
        options.timing.separate_snapshots = false;
        assert!(heat_rate(&flow, &entering, &leaving, &options).is_err());
    }
    #[test]
    fn cv_kv_and_density_pressure_scaling_match_independent_cases() {
        let dp = record("p", 100., "kPa", 0);
        let mut options = valve();
        assert!((valve_flow(&dp, &options).unwrap().value - 2.).abs() < 1e-12);
        options.fluid.density_kg_m3 = 4000.;
        assert!((valve_flow(&dp, &options).unwrap().value - 1.).abs() < 1e-12);
        assert!(
            (valve_flow(&record("p", 400., "kPa", 0), &options)
                .unwrap()
                .value
                - 2.)
                .abs()
                < 1e-12
        );
        options.coefficient_kind = ValveCoefficient::Cv;
        options.output_unit = Unit::UsGpm;
        options.fluid.density_kg_m3 = 999.016;
        assert!(
            (valve_flow(&record("p", 1., "psi", 0), &options)
                .unwrap()
                .value
                - 2.)
                .abs()
                < 1e-12
        );
        options.basis = Basis::FieldFactors;
        assert!(
            (valve_flow(&record("p", 1., "psi", 0), &options)
                .unwrap()
                .value
                - 2.)
                .abs()
                < 1e-12
        );
        assert_eq!(
            valve_flow(&record("p", 0., "psi", 0), &options)
                .unwrap()
                .value,
            0.
        );
    }
    #[test]
    fn invalid_properties_inputs_and_unconfirmed_conditions_cannot_produce_results() {
        let dp = record("p", 1., "psi", 0);
        let mut options = valve();
        for invalid in [0., -1., f64::NAN, f64::INFINITY] {
            options.fluid.density_kg_m3 = invalid;
            assert!(valve_flow(&dp, &options).is_err());
        }
        options.fluid = fluid();
        options.applicability_confirmed = false;
        assert!(valve_flow(&dp, &options).is_err());
        options.applicability_confirmed = true;
        options.fluid.properties_confirmed = false;
        assert!(valve_flow(&dp, &options).is_err());
        options.fluid = fluid();
        options.coefficient = 0.;
        assert!(valve_flow(&dp, &options).is_err());
        options.coefficient = 2.;
        assert!(valve_flow(&record("p", -1., "psi", 0), &options).is_err());
        assert!(valve_flow(&record("p", 1., "CFM", 0), &options).is_err());
        let e = record("e", 30., "°C", 1);
        let l = record("l", 20., "°C", 2);
        assert!(heat_rate(&record("q", -1., "L/s", 0), &e, &l, &thermal()).is_err());
        assert!(heat_rate(&record("e", 1., "L/s", 0), &e, &l, &thermal()).is_err());
        let mut thermal_options = thermal();
        thermal_options.specific_heat_j_kg_k = f64::MAX;
        assert!(heat_rate(&record("q", 1., "L/s", 0), &e, &l, &thermal_options).is_err());
    }
    #[test]
    fn saved_hydronic_results_preserve_roles_properties_and_input_context() {
        let flow = record("q", 1., "L/s", 0);
        let e = record("e", 30., "°C", 1);
        let l = record("l", 20., "°C", 2);
        let result = heat_rate(&flow, &e, &l, &thermal()).unwrap().result;
        let notes = &result
            .metadata
            .as_ref()
            .unwrap()
            .derivation
            .as_ref()
            .unwrap()
            .assumptions;
        assert!(notes["input_roles"].contains("entering"));
        assert_eq!(notes["specific_heat_j_kg_k"], "4180");
        assert!(!result.timestamp_is_measurement_candidate());
        let mut project = Project::new("example");
        project.readings = vec![flow, e, l];
        validate_inputs(&result, &project).unwrap();
        project.readings[0].value = 2.;
        assert!(validate_inputs(&result, &project).is_err());
        project.readings = vec![
            result,
            valve_flow(&record("p", 100., "kPa", 0), &valve()).unwrap(),
        ];
        let path =
            std::env::temp_dir().join(format!("green-suite-hydronic-{}.csv", uuid::Uuid::new_v4()));
        project.export_csv(&path).unwrap();
        let mut imported = Project::new("import");
        imported.import_csv(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        for (a, b) in project.readings.iter().zip(&imported.readings) {
            assert_eq!(
                serde_json::to_value(&a.metadata).unwrap(),
                serde_json::to_value(&b.metadata).unwrap()
            );
        }
    }
}
