// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Ideal moist-air state estimates from explicitly assigned stored observations.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CondensationPoint {
    Temperature(f64),
    NoVapour,
    BelowRange,
}
#[derive(Clone, Debug)]
pub struct AirState {
    pub temperature_c: f64,
    pub relative_humidity_percent: f64,
    pub pressure_pa: f64,
    pub humidity_ratio: f64,
    pub enthalpy_j_kg_dry_air: f64,
    pub condensation: CondensationPoint,
}

fn saturation_pa(celsius: f64) -> Result<f64> {
    ensure!(
        celsius.is_finite() && (-100.0..=200.).contains(&celsius),
        "Dry-bulb temperature must be between -100 and 200 °C"
    );
    // Coefficient order: reciprocal T, constant, T through T⁴, ln(T).
    let coefficients = if celsius <= 0.01 {
        [
            -5674.5359,
            6.3925247,
            -0.009677843,
            0.00000062215701,
            0.0000000020747825,
            -0.0000000000009484024,
            4.1635019,
        ]
    } else {
        [
            -5800.2206,
            1.3914993,
            -0.048640239,
            0.000041764768,
            -0.000000014452093,
            0.,
            6.5459673,
        ]
    };
    let kelvin = celsius + 273.15;
    let terms = [
        kelvin.recip(),
        1.,
        kelvin,
        kelvin.powi(2),
        kelvin.powi(3),
        kelvin.powi(4),
        kelvin.ln(),
    ];
    let log_pressure: f64 = coefficients
        .iter()
        .zip(terms)
        .map(|(coefficient, term)| coefficient * term)
        .sum();
    Ok(log_pressure.exp())
}
fn pressure_range(pressure: f64) -> Result<()> {
    ensure!(
        pressure.is_finite() && (20_000.0..=120_000.).contains(&pressure),
        "This atmospheric-air model supports absolute barometric pressure from 20 to 120 kPa"
    );
    Ok(())
}
fn state(celsius: f64, rh_percent: f64, pressure: f64, ice_confirmed: bool) -> Result<AirState> {
    pressure_range(pressure)?;
    ensure!(
        rh_percent.is_finite() && (0.0..=100.).contains(&rh_percent),
        "Relative humidity must be from 0 to 100 percent"
    );
    ensure!(
        celsius >= 0.01 || ice_confirmed,
        "At or below 0.01 °C, confirm the input RH is referenced to ice equilibrium"
    );
    let vapour = saturation_pa(celsius)? * rh_percent / 100.;
    ensure!(
        vapour < pressure,
        "Water-vapour pressure must be below absolute barometric pressure"
    );
    let humidity_ratio = 0.621945 * vapour / (pressure - vapour);
    let enthalpy = 1006. * celsius + humidity_ratio * (2_501_000. + 1860. * celsius);
    ensure!(
        humidity_ratio.is_finite() && enthalpy.is_finite(),
        "Air-state calculation overflow"
    );
    let condensation = if vapour == 0. {
        CondensationPoint::NoVapour
    } else if vapour < saturation_pa(-100.)? {
        CondensationPoint::BelowRange
    } else {
        let (mut lower, mut upper) = (-100., celsius);
        for _ in 0..64 {
            let midpoint = lower + (upper - lower) / 2.;
            if saturation_pa(midpoint)? < vapour {
                lower = midpoint;
            } else {
                upper = midpoint;
            }
        }
        CondensationPoint::Temperature((lower + upper) / 2.)
    };
    Ok(AirState {
        temperature_c: celsius,
        relative_humidity_percent: rh_percent,
        pressure_pa: pressure,
        humidity_ratio,
        enthalpy_j_kg_dry_air: enthalpy,
        condensation,
    })
}

pub struct AirPair<'a> {
    pub temperature: &'a Reading,
    pub humidity: &'a Reading,
    pub role: &'a str,
}
pub enum Barometric<'a> {
    Record(&'a Reading),
    Declared { pressure_pa: f64, assumed: bool },
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PsychMetric {
    #[default]
    HumidityRatio,
    Enthalpy,
    Condensation,
    MoistureRemoval,
}
pub struct PsychOptions {
    pub basis: Basis,
    pub timing: InputTiming,
    pub pressure_is_absolute_confirmed: bool,
    pub subzero_rh_ice_confirmed: bool,
    pub dry_air_mass_flow_confirmed: bool,
}
pub struct RhCurve {
    pub rh_percent: f64,
    pub points: Vec<Option<(f64, f64)>>,
}
pub struct PsychResult {
    pub selected: Reading,
    pub entering: AirState,
    pub leaving: Option<AirState>,
    pub curves: Vec<RhCurve>,
    pub chart_min_c: f64,
    pub chart_max_c: f64,
}
fn air_pair(pair: &AirPair<'_>, pressure: f64, options: &PsychOptions) -> Result<AirState> {
    ensure!(
        !pair.role.trim().is_empty(),
        "Assign an air-state location/role"
    );
    state(
        value_in(pair.temperature, Unit::Celsius, options.basis)?,
        value_in(pair.humidity, Unit::RhPercent, options.basis)?,
        pressure,
        options.subzero_rh_ice_confirmed,
    )
}
pub fn psychrometric(
    entering: AirPair<'_>,
    leaving: Option<AirPair<'_>>,
    mass_flow: Option<&Reading>,
    pressure: Barometric<'_>,
    metric: PsychMetric,
    options: &PsychOptions,
) -> Result<PsychResult> {
    ensure!(
        options.pressure_is_absolute_confirmed,
        "Confirm absolute barometric pressure; gauge or differential pressure is unsuitable"
    );
    let mut inputs = vec![entering.temperature, entering.humidity];
    let mut notes = BTreeMap::new();
    let pressure_pa = match pressure {
        Barometric::Record(reading) => {
            inputs.push(reading);
            notes.insert(
                "pressure_time_interpretation".into(),
                "Selected pressure record included in input time-span checks".into(),
            );
            notes.insert(
                "barometric_pressure_kind".into(),
                "Selected record, explicitly assigned as absolute barometric pressure".into(),
            );
            value_in(reading, Unit::Pa, options.basis)?
        }
        Barometric::Declared {
            pressure_pa,
            assumed,
        } => {
            notes.insert("pressure_time_interpretation".into(), "Declared constant pressure without a measurement timestamp; paired timing applies only to selected records".into());
            notes.insert(
                "barometric_pressure_kind".into(),
                if assumed {
                    "User-declared assumption"
                } else {
                    "User-declared observed absolute pressure; no source record supplied"
                }
                .into(),
            );
            pressure_pa
        }
    };
    let first = air_pair(&entering, pressure_pa, options)?;
    let mut roles = vec![
        (
            "temperature",
            entering.temperature.id.as_str(),
            entering.role,
        ),
        (
            "relative_humidity",
            entering.humidity.id.as_str(),
            entering.role,
        ),
    ];
    let second = if metric == PsychMetric::MoistureRemoval {
        let pair = leaving
            .as_ref()
            .context("Select a leaving air state for moisture removal")?;
        ensure!(
            pair.role.trim() != entering.role.trim(),
            "Assign distinct entering and leaving air-state roles"
        );
        inputs.extend([pair.temperature, pair.humidity]);
        roles.extend([
            ("temperature", pair.temperature.id.as_str(), pair.role),
            ("relative_humidity", pair.humidity.id.as_str(), pair.role),
        ]);
        Some(air_pair(pair, pressure_pa, options)?)
    } else {
        None
    };
    let (value, unit, kind, formula) = match metric {
        PsychMetric::HumidityRatio => (
            first.humidity_ratio,
            Unit::KgPerKg,
            "Humidity ratio estimate",
            "moist-air-ratio-v1",
        ),
        PsychMetric::Enthalpy => (
            first.enthalpy_j_kg_dry_air,
            Unit::JPerKg,
            "Moist-air enthalpy estimate (per kg dry air)",
            "moist-air-enthalpy-v1",
        ),
        PsychMetric::Condensation => {
            let value = match first.condensation {
                CondensationPoint::Temperature(value) => value,
                CondensationPoint::NoVapour => bail!(
                    "Zero RH: no finite dew/frost point exists; humidity ratio and enthalpy remain available"
                ),
                CondensationPoint::BelowRange => bail!(
                    "Dew/frost point is below the model limit of -100 °C; humidity ratio and enthalpy remain available"
                ),
            };
            (
                value,
                Unit::Celsius,
                if value <= 0.01 {
                    "Frost-point estimate"
                } else {
                    "Dew-point estimate"
                },
                "moist-air-condensation-v1",
            )
        }
        PsychMetric::MoistureRemoval => {
            ensure!(
                options.dry_air_mass_flow_confirmed,
                "Confirm the selected mass flow is dry-air mass flow, not moist-air or volume flow"
            );
            let flow = mass_flow.context("Select dry-air mass flow")?;
            let mass = value_in(flow, Unit::KgPerSecond, options.basis)?;
            ensure!(mass >= 0., "Dry-air mass flow must be nonnegative");
            inputs.push(flow);
            roles.push((
                "dry_air_mass_flow",
                flow.id.as_str(),
                "Through the declared air process",
            ));
            notes.insert("dry_air_mass_flow_kg_s".into(), mass.to_string());
            notes.insert("dry_air_mass_flow_confirmed".into(), "true".into());
            notes.insert("sign".into(), "Positive = water removed from air; negative = water added to air; does not prove condensate collection".into());
            (
                mass * (first.humidity_ratio - second.as_ref().unwrap().humidity_ratio),
                Unit::KgPerSecond,
                "Moisture-removal rate estimate",
                "moist-air-moisture-removal-v1",
            )
        }
    };
    notes.extend(options.timing.notes(&inputs)?);
    notes.extend([
        ("absolute_barometric_pressure_pa".into(), pressure_pa.to_string()),
        ("pressure_role_confirmed".into(), "true".into()),
        ("subzero_rh_ice_confirmed".into(), options.subzero_rh_ice_confirmed.to_string()),
        ("input_roles".into(), serde_json::to_string(&roles)?),
        ("calculation_basis".into(), options.basis.label().into()),
        ("unit_rule_version".into(), options.basis.rule_id().into()),
        ("model".into(), "ideal-moist-air-v1; dry bulb -100..200 °C; absolute pressure 20..120 kPa; ice equilibrium at/below 0.01 °C, water above; no enhancement factor or uncertainty propagation".into()),
        ("enthalpy_reference".into(), "Per kg dry air; dry-air sensible reference at 0 °C; vapour enthalpy offset 2501000 J/kg at 0 °C".into()),
        ("equations".into(), "pv = RH_fraction × saturation_Pa; W = 0.621945 × pv / (P - pv); h_J_kg_dry_air = 1006 × T_C + W × (2501000 + 1860 × T_C); moisture_kg_s = dry_air_kg_s × (W_entering - W_leaving); condensation point by bounded saturation-pressure inversion".into()),
        ("entering_state_si".into(), state_notes(&first)),
    ]);
    if let Some(second) = &second {
        notes.insert("leaving_state_si".into(), state_notes(second));
    }
    let selected = derived(
        &entering.temperature.group,
        kind,
        value,
        unit,
        formula,
        &inputs,
        notes,
    )?;
    let chart_min_c = first
        .temperature_c
        .min(
            second
                .as_ref()
                .map_or(first.temperature_c, |s| s.temperature_c),
        )
        .clamp(-95., -10.)
        - 5.;
    let chart_max_c = first
        .temperature_c
        .max(
            second
                .as_ref()
                .map_or(first.temperature_c, |s| s.temperature_c),
        )
        .clamp(50., 195.)
        + 5.;
    let curves = [10., 30., 50., 70., 90., 100.]
        .into_iter()
        .map(|rh_percent| {
            let points = (0..=120)
                .map(|index| {
                    let t = chart_min_c + (chart_max_c - chart_min_c) * index as f64 / 120.;
                    let vapour = saturation_pa(t).ok()? * rh_percent / 100.;
                    (vapour < pressure_pa).then(|| (t, 0.621945 * vapour / (pressure_pa - vapour)))
                })
                .collect();
            RhCurve { rh_percent, points }
        })
        .collect();
    Ok(PsychResult {
        selected,
        entering: first,
        leaving: second,
        curves,
        chart_min_c,
        chart_max_c,
    })
}
fn state_notes(state: &AirState) -> String {
    format!(
        "dry_bulb_C={}; RH_percent={}; absolute_pressure_Pa={}; W_kg_per_kg_dry_air={}; h_J_per_kg_dry_air={}; equilibrium_condensation_point={:?}",
        state.temperature_c,
        state.relative_humidity_percent,
        state.pressure_pa,
        state.humidity_ratio,
        state.enthalpy_j_kg_dry_air,
        state.condensation
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
    fn options() -> PsychOptions {
        PsychOptions {
            basis: Basis::PhysicalDefinitions,
            timing: InputTiming {
                max_separation_seconds: 3.,
                clocks_confirmed: false,
                separate_snapshots: false,
            },
            pressure_is_absolute_confirmed: true,
            subzero_rh_ice_confirmed: false,
            dry_air_mass_flow_confirmed: true,
        }
    }
    fn declared() -> Barometric<'static> {
        Barometric::Declared {
            pressure_pa: 101325.,
            assumed: true,
        }
    }
    fn pair<'a>(t: &'a Reading, rh: &'a Reading, role: &'a str) -> AirPair<'a> {
        AirPair {
            temperature: t,
            humidity: rh,
            role,
        }
    }
    #[test]
    fn saturation_and_room_air_match_independent_rounded_reference_cases() {
        for (temperature, pressure, tolerance) in [
            (-20., 103.26, 0.02),
            (0., 611.15, 0.1),
            (25., 3169.2, 0.2),
            (50., 12349.9, 0.3),
            (100., 101418., 2.),
        ] {
            assert!((saturation_pa(temperature).unwrap() - pressure).abs() < tolerance);
        }
        let air = state(25., 50., 101325., false).unwrap();
        assert!((air.humidity_ratio - 0.009881).abs() < 0.000001);
        assert!((air.enthalpy_j_kg_dry_air - 50322.).abs() < 2.);
        let CondensationPoint::Temperature(dew) = air.condensation else {
            panic!("Expected dew point")
        };
        assert!((dew - 13.864).abs() < 0.002);
        assert!(state(25., 50., 80000., false).unwrap().humidity_ratio > air.humidity_ratio);
    }
    #[test]
    fn dry_air_and_model_boundaries_do_not_get_silently_clamped() {
        let dry = state(25., 0., 101325., false).unwrap();
        assert_eq!(dry.humidity_ratio, 0.);
        assert_eq!(dry.enthalpy_j_kg_dry_air, 25150.);
        assert_eq!(dry.condensation, CondensationPoint::NoVapour);
        assert_eq!(
            state(-90., 1e-9, 101325., true).unwrap().condensation,
            CondensationPoint::BelowRange
        );
        for t in [-100., -20., 0.01, 25., 50.] {
            let air = state(t, 100., 101325., true).unwrap();
            let CondensationPoint::Temperature(condensation) = air.condensation else {
                panic!("Expected saturation")
            };
            assert!((condensation - t).abs() < 1e-7);
        }
        assert!(state(-1., 50., 101325., false).is_err());
        assert!((saturation_pa(0.01).unwrap() - saturation_pa(0.010001).unwrap()).abs() < 0.001);
        for (t, rh, p) in [
            (25., -1., 101325.),
            (25., 101., 101325.),
            (-101., 50., 101325.),
            (201., 50., 101325.),
            (25., 50., 0.),
            (25., 50., 130000.),
            (200., 100., 101325.),
            (f64::NAN, 50., 101325.),
            (25., f64::INFINITY, 101325.),
        ] {
            assert!(state(t, rh, p, true).is_err());
        }
    }
    #[test]
    fn metric_selection_pressure_provenance_and_missing_condensation_are_explicit() {
        let t = record("t", 77., "°F", 0);
        let rh = record("r", 50., "%RH", 1);
        let p = record("p", 101.325, "kPa", 2);
        let result = psychrometric(
            pair(&t, &rh, "Room"),
            None,
            None,
            Barometric::Record(&p),
            PsychMetric::Enthalpy,
            &options(),
        )
        .unwrap();
        assert!((result.selected.value - 50322.).abs() < 2.);
        let derivation = result
            .selected
            .metadata
            .as_ref()
            .unwrap()
            .derivation
            .as_ref()
            .unwrap();
        assert_eq!(derivation.inputs.len(), 3);
        assert!(derivation.assumptions["barometric_pressure_kind"].contains("Selected record"));
        let mut project = Project::new("example");
        project.readings = vec![t.clone(), rh.clone(), p];
        validate_inputs(&result.selected, &project).unwrap();
        project.readings[2].value = 102.;
        assert!(validate_inputs(&result.selected, &project).is_err());
        assert!(!result.selected.timestamp_is_measurement_candidate());
        let zero = record("z", 0., "%RH", 1);
        assert!(
            psychrometric(
                pair(&t, &zero, "Room"),
                None,
                None,
                declared(),
                PsychMetric::Condensation,
                &options()
            )
            .is_err()
        );
        let result = psychrometric(
            pair(&t, &zero, "Room"),
            None,
            None,
            declared(),
            PsychMetric::HumidityRatio,
            &options(),
        )
        .unwrap();
        assert_eq!(result.selected.value, 0.);
        assert_eq!(
            result
                .selected
                .metadata
                .unwrap()
                .derivation
                .unwrap()
                .assumptions["barometric_pressure_kind"],
            "User-declared assumption"
        );
    }
    #[test]
    fn moisture_rate_preserves_sign_zero_and_declared_dry_air_basis() {
        let t = record("t", 25., "°C", 0);
        let r = record("r", 50., "%RH", 1);
        let tb = record("tb", 15., "°C", 0);
        let rb = record("rb", 40., "%RH", 1);
        let mut mass = record("m", 2., "kg/s", 2);
        let expected = 2.
            * (0.621945 * (0.5 * 3169.2) / (101325. - 0.5 * 3169.2)
                - 0.621945 * (0.4 * 1705.45) / (101325. - 0.4 * 1705.45));
        let result = psychrometric(
            pair(&t, &r, "Entering"),
            Some(pair(&tb, &rb, "Leaving")),
            Some(&mass),
            declared(),
            PsychMetric::MoistureRemoval,
            &options(),
        )
        .unwrap();
        assert!((result.selected.value - expected).abs() < 0.000001);
        let reversed = psychrometric(
            pair(&tb, &rb, "Entering"),
            Some(pair(&t, &r, "Leaving")),
            Some(&mass),
            declared(),
            PsychMetric::MoistureRemoval,
            &options(),
        )
        .unwrap();
        assert!((reversed.selected.value + result.selected.value).abs() < 1e-12);
        let mut unconfirmed = options();
        unconfirmed.dry_air_mass_flow_confirmed = false;
        assert!(
            psychrometric(
                pair(&t, &r, "Entering"),
                Some(pair(&tb, &rb, "Leaving")),
                Some(&mass),
                declared(),
                PsychMetric::MoistureRemoval,
                &unconfirmed
            )
            .is_err()
        );
        mass.value = 0.;
        assert_eq!(
            psychrometric(
                pair(&t, &r, "Entering"),
                Some(pair(&tb, &rb, "Leaving")),
                Some(&mass),
                declared(),
                PsychMetric::MoistureRemoval,
                &options()
            )
            .unwrap()
            .selected
            .value,
            0.
        );
        mass.value = -1.;
        assert!(
            psychrometric(
                pair(&t, &r, "Entering"),
                Some(pair(&tb, &rb, "Leaving")),
                Some(&mass),
                declared(),
                PsychMetric::MoistureRemoval,
                &options()
            )
            .is_err()
        );
    }
    #[test]
    fn timing_roles_units_and_absolute_pressure_confirmation_are_required() {
        let mut t = record("t", 25., "°C", 0);
        let rh = record("r", 50., "%RH", 1);
        let p = record("p", 101325., "Pa", 10);
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                Barometric::Record(&p),
                PsychMetric::HumidityRatio,
                &options()
            )
            .is_err()
        );
        t.source = "manual".into();
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                declared(),
                PsychMetric::HumidityRatio,
                &options()
            )
            .is_err()
        );
        let mut snapshots = options();
        snapshots.timing.separate_snapshots = true;
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                declared(),
                PsychMetric::HumidityRatio,
                &snapshots
            )
            .is_ok()
        );
        snapshots.pressure_is_absolute_confirmed = false;
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                declared(),
                PsychMetric::HumidityRatio,
                &snapshots
            )
            .is_err()
        );
        let t = record("t", 25., "Pa", 0);
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                declared(),
                PsychMetric::HumidityRatio,
                &options()
            )
            .is_err()
        );
        let t = record("r", 25., "°C", 0);
        assert!(
            psychrometric(
                pair(&t, &rh, "Room"),
                None,
                None,
                declared(),
                PsychMetric::HumidityRatio,
                &options()
            )
            .is_err()
        );
    }
    #[test]
    fn chart_limits_preserve_gaps_and_saved_native_exports_preserve_state_context() {
        let t = record("t", 200., "°C", 0);
        let r = record("r", 1., "%RH", 0);
        let result = psychrometric(
            pair(&t, &r, "Example"),
            None,
            None,
            declared(),
            PsychMetric::HumidityRatio,
            &options(),
        )
        .unwrap();
        assert_eq!(result.curves.len(), 6);
        assert_eq!(result.chart_max_c, 200.);
        assert!(
            result
                .curves
                .last()
                .unwrap()
                .points
                .iter()
                .any(Option::is_none)
        );
        assert!(result.curves.iter().all(|c| c.points.len() == 121));
        let mut project = Project::new("example");
        project.readings.push(result.selected);
        let json = serde_json::to_string(&project).unwrap();
        let loaded: Project = serde_json::from_str(&json).unwrap();
        let path =
            std::env::temp_dir().join(format!("green-suite-air-{}.csv", uuid::Uuid::new_v4()));
        loaded.export_csv(&path).unwrap();
        let mut imported = Project::new("import");
        imported.import_csv(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            serde_json::to_value(&project.readings[0].metadata).unwrap(),
            serde_json::to_value(&imported.readings[0].metadata).unwrap()
        );
    }
}
