// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Calculations from selected stored readings; no device or service access.
mod balance;
mod hydronic;
mod psychrometric;
mod series;
pub use balance::{BalanceChoice, BalanceResult, balance};
pub use hydronic::HydronicTiming as InputTiming;
pub use hydronic::{
    Fluid, HydronicTiming, ThermalOptions, ThermalResult, ValveCoefficient, ValveOptions,
    heat_rate, valve_flow,
};
pub use psychrometric::{
    AirPair, Barometric, CondensationPoint, PsychMetric, PsychOptions, PsychResult, psychrometric,
};
pub use series::{SeriesOptions, SeriesResult, series};

use crate::{
    measurement::{Derivation, Input, Metadata, Origin, TimeBasis},
    model::{Project, Reading},
    standards::Basis,
    units::{Quantity, Unit},
};
use anyhow::{Context, Result, bail, ensure};
use chrono::{Duration, NaiveDateTime};
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_INPUTS: usize = 100_000;
#[derive(PartialEq, Eq)]
enum Clock {
    Utc,
    Reporter(String),
    Unknown,
}
struct Stamp {
    time: NaiveDateTime,
    clock: Clock,
}

fn stamp(reading: &Reading) -> Result<Stamp> {
    let metadata = reading.metadata.as_ref();
    ensure!(
        reading.timestamp_is_measurement_candidate(),
        "Entry, receipt or calculation time is not an established measurement time"
    );
    let (mut time, clock) =
        if let Ok(time) = chrono::DateTime::parse_from_rfc3339(&reading.timestamp) {
            (time.naive_utc(), Clock::Utc)
        } else {
            let time = [
                "%Y-%m-%dT%H:%M:%S%.f",
                "%Y-%m-%d %H:%M:%S%.f",
                "%m/%d/%Y %H:%M:%S",
                "%m/%d/%Y %I:%M:%S %p",
            ]
            .iter()
            .find_map(|pattern| NaiveDateTime::parse_from_str(&reading.timestamp, pattern).ok())
            .context("A complete date and time is required for time pairing")?;
            let clock = match metadata.map(|m| m.time_basis) {
                Some(TimeBasis::PcUtc) => Clock::Utc,
                Some(TimeBasis::InstrumentLocal) => metadata
                    .and_then(|m| m.reporter_serial.clone())
                    .filter(|s| !s.is_empty())
                    .map(Clock::Reporter)
                    .unwrap_or(Clock::Unknown),
                _ => Clock::Unknown,
            };
            (time, clock)
        };
    if let Some(offset) = metadata.and_then(|m| m.declared_clock_offset_seconds) {
        time = time
            .checked_add_signed(
                Duration::try_seconds(offset).context("Clock correction is out of range")?,
            )
            .context("Corrected timestamp is out of range")?;
    }
    Ok(Stamp { time, clock })
}
fn comparable(a: &Stamp, b: &Stamp, confirmed: bool) -> bool {
    confirmed || (a.clock != Clock::Unknown && a.clock == b.clock)
}

fn value_in(reading: &Reading, target: Unit, basis: Basis) -> Result<f64> {
    ensure!(
        reading.value.is_finite(),
        "Input contains a non-finite value"
    );
    let from = Unit::parse(&reading.unit)
        .context("An input unit requires interpretation in source-record review")?;
    if let Some(metadata) = &reading.metadata {
        metadata.validate(reading.value)?;
        ensure!(
            metadata.quantity == Quantity::Unknown || metadata.quantity == from.quantity(),
            "Input quantity conflicts with its unit"
        );
    }
    Ok(basis.calculate(reading.value, from, target)?.0)
}

fn derived(
    group: &str,
    kind: &str,
    value: f64,
    unit: Unit,
    formula: &str,
    inputs: &[&Reading],
    mut assumptions: BTreeMap<String, String>,
) -> Result<Reading> {
    ensure!(value.is_finite(), "Calculated result is not finite");
    ensure!(
        !inputs.is_empty() && inputs.len() <= MAX_INPUTS,
        "Select between 1 and 100,000 input readings"
    );
    let mut ids = HashSet::new();
    for input in inputs {
        ensure!(
            !input.id.is_empty() && ids.insert(&input.id),
            "The same reading cannot be counted twice"
        );
    }
    assumptions.insert(
        "result_timestamp".into(),
        "PC calculation time in UTC; source measurement times preserved in inputs".into(),
    );
    let derivation = Derivation {
        formula: formula.into(),
        version: "1".into(),
        inputs: inputs
            .iter()
            .map(|r| Input::from_reading(r))
            .collect::<Result<_>>()?,
        assumptions,
    };
    let result = Reading {
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        group: group.into(),
        sensor: "Stored-reading analysis".into(),
        kind: kind.into(),
        value,
        unit: unit.symbol().into(),
        source: "Green Suite calculation".into(),
        raw: serde_json::json!({"formula":derivation.formula,"version":derivation.version,"input_count":derivation.inputs.len(),"assumptions":derivation.assumptions}).to_string(),
        metadata: Some(Metadata {
            origin: Origin::ApplicationCalculated,
            quantity: unit.quantity(),
            time_basis: TimeBasis::PcUtc,
            derivation: Some(derivation),
            ..Default::default()
        }),
    };
    result.metadata.as_ref().unwrap().validate(value)?;
    Ok(result)
}

pub fn validate_inputs(result: &Reading, project: &Project) -> Result<()> {
    let derivation = result
        .metadata
        .as_ref()
        .and_then(|m| m.derivation.as_ref())
        .context("Calculation inputs are missing")?;
    let current: HashMap<_, _> = project
        .readings
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();
    for input in &derivation.inputs {
        let reading = current
            .get(input.reading_id.as_str())
            .context("A calculation input was removed; preview again")?;
        ensure!(
            &Input::from_reading(reading)? == input,
            "A calculation input changed; preview again"
        );
    }
    if derivation.formula == "device-balance-summary-v1" {
        type DesignSnapshot = (String, String, f64, String, String, Option<String>);
        let designs: Vec<DesignSnapshot> = serde_json::from_str(
            derivation
                .assumptions
                .get("device_designs")
                .context("Design input snapshots are missing")?,
        )?;
        for (id, label, design, unit, system, reading_id) in designs {
            let point = project
                .points
                .iter()
                .find(|p| p.id == id)
                .context("A selected device was removed; preview again")?;
            ensure!(
                point.label == label
                    && point.design_value == design
                    && point.unit == unit
                    && point.system == system
                    && reading_id.is_some_and(|reading| point.reading_ids.contains(&reading)),
                "A design value or device mapping changed; preview again"
            );
        }
    }
    Ok(())
}

pub struct CompareOptions {
    pub target: Unit,
    pub basis: Basis,
    pub role_a: String,
    pub role_b: String,
    pub max_separation_seconds: f64,
    pub clocks_confirmed: bool,
    pub before_after: bool,
    pub accept_unknown_time: bool,
}
pub fn difference(a: &Reading, b: &Reading, options: &CompareOptions) -> Result<Reading> {
    ensure!(a.id != b.id, "Choose two different readings");
    ensure!(
        !options.role_a.trim().is_empty() && !options.role_b.trim().is_empty(),
        "Assign a role to each input"
    );
    ensure!(
        matches!(
            options.target.quantity(),
            Quantity::Pressure | Quantity::Temperature | Quantity::VolumeFlow | Quantity::Velocity
        ),
        "Unsupported comparison quantity"
    );
    ensure!(
        options.max_separation_seconds.is_finite() && options.max_separation_seconds >= 0.,
        "Time tolerance must be finite and nonnegative"
    );
    let va = value_in(a, options.target, options.basis)?;
    let vb = value_in(b, options.target, options.basis)?;
    let mut notes = BTreeMap::from([
        ("role_a".into(), options.role_a.clone()),
        ("role_b".into(), options.role_b.clone()),
        ("sign".into(), "A minus B".into()),
        ("calculation_basis".into(), options.basis.label().into()),
        ("unit_rule_version".into(), options.basis.rule_id().into()),
        (
            "time_tolerance_seconds".into(),
            options.max_separation_seconds.to_string(),
        ),
        (
            "comparison_purpose".into(),
            if options.before_after {
                "before-after snapshots"
            } else {
                "paired comparison"
            }
            .into(),
        ),
        (
            "clocks_confirmed_by_user".into(),
            options.clocks_confirmed.to_string(),
        ),
        (
            "unknown_snapshot_time_accepted".into(),
            options.accept_unknown_time.to_string(),
        ),
    ]);
    match (stamp(a), stamp(b)) {
        (Ok(ta), Ok(tb)) if comparable(&ta, &tb, options.clocks_confirmed) => {
            let separation = (ta.time - tb.time).num_milliseconds().unsigned_abs() as f64 / 1000.;
            notes.insert("time_separation_seconds".into(), separation.to_string());
            if !options.before_after {
                ensure!(
                    separation <= options.max_separation_seconds,
                    "Readings exceed the configured time-pairing tolerance"
                );
            }
        }
        _ if options.before_after && options.accept_unknown_time => {
            notes.insert(
                "time_separation".into(),
                "Unknown; explicitly accepted as separate snapshots".into(),
            );
        }
        _ => bail!(
            "Input times are missing or their clocks are not established as comparable. Review times and clock corrections, or explicitly confirm a before/after snapshot comparison."
        ),
    }
    let unit = if options.target.quantity() == Quantity::Temperature {
        match options.target {
            Unit::Fahrenheit => Unit::DeltaF,
            Unit::Kelvin => Unit::DeltaK,
            _ => Unit::DeltaC,
        }
    } else {
        options.target
    };
    derived(
        &a.group,
        "Stored-reading difference",
        va - vb,
        unit,
        "reading-difference-v1",
        &[a, b],
        notes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reading(id: &str, value: f64, unit: &str, second: u32) -> Reading {
        Reading {
            id: id.into(),
            value,
            unit: unit.into(),
            timestamp: format!("2026-01-01T12:00:{second:02}Z"),
            ..Default::default()
        }
    }
    fn options(target: Unit) -> CompareOptions {
        CompareOptions {
            target,
            basis: Basis::PhysicalDefinitions,
            role_a: "Entering".into(),
            role_b: "Leaving".into(),
            max_separation_seconds: 3.,
            clocks_confirmed: false,
            before_after: false,
            accept_unknown_time: false,
        }
    }
    #[test]
    fn temperature_comparison_uses_difference_units_and_enforces_pairing() {
        let a = reading("a", 68., "°F", 0);
        let b = reading("b", 10., "°C", 2);
        let mut options = options(Unit::Celsius);
        let result = difference(&a, &b, &options).unwrap();
        assert!((result.value - 10.).abs() < 1e-9);
        assert_eq!(result.unit, "Δ°C");
        options.max_separation_seconds = 1.;
        assert!(difference(&a, &b, &options).is_err());
        options.before_after = true;
        assert!(difference(&a, &b, &options).is_ok());
    }
    #[test]
    fn unknown_clocks_and_missing_times_require_explicit_snapshot_confirmation() {
        let mut a = reading("a", 0., "Pa", 0);
        let mut b = reading("b", -1., "Pa", 0);
        a.timestamp = "2026-01-01T12:00:00".into();
        b.timestamp = a.timestamp.clone();
        let mut options = options(Unit::Pa);
        assert!(difference(&a, &b, &options).is_err());
        options.clocks_confirmed = true;
        assert!(difference(&a, &b, &options).is_ok());
        a.timestamp.clear();
        assert!(difference(&a, &b, &options).is_err());
        options.before_after = true;
        assert!(difference(&a, &b, &options).is_err());
        options.accept_unknown_time = true;
        let result = difference(&a, &b, &options).unwrap();
        assert_eq!(result.value, 1.);
        assert!(
            result.metadata.unwrap().derivation.unwrap().assumptions["time_separation"]
                .starts_with("Unknown")
        );
    }
    #[test]
    fn clock_corrections_are_separate_and_changed_input_metadata_invalidates_preview() {
        let mut a = reading("a", 2., "Pa", 0);
        let b = reading("b", 1., "Pa", 10);
        a.metadata = Some(Metadata {
            declared_clock_offset_seconds: Some(10),
            ..Default::default()
        });
        let options = options(Unit::Pa);
        let result = difference(&a, &b, &options).unwrap();
        let mut project = Project::new("Example");
        project.readings = vec![a, b];
        validate_inputs(&result, &project).unwrap();
        assert_eq!(project.readings[0].timestamp, "2026-01-01T12:00:00Z");
        project.readings[0]
            .metadata
            .as_mut()
            .unwrap()
            .declared_clock_offset_seconds = Some(20);
        assert!(validate_inputs(&result, &project).is_err());
    }
    #[test]
    fn sample_and_hold_weighted_statistics_are_different_and_saved_explicitly() {
        let a = reading("a", 1., "Pa", 0);
        let b = reading("b", 2., "Pa", 1);
        let c = reading("c", 3., "Pa", 3);
        let result = series(
            &[&a, &b, &c],
            Unit::Pa,
            Basis::FieldFactors,
            &SeriesOptions {
                time_weighted: true,
                max_gap_seconds: 3.,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.count, 3);
        assert!((result.mean - 2.).abs() < 1e-12);
        assert!((result.sample_stddev.unwrap() - 1.).abs() < 1e-12);
        assert!((result.time_weighted_mean.unwrap() - 5. / 3.).abs() < 1e-12);
        assert_eq!(result.maximum_gap_seconds, Some(2.));
        assert_eq!(
            result
                .result
                .metadata
                .unwrap()
                .derivation
                .unwrap()
                .inputs
                .len(),
            3
        );
    }
    #[test]
    fn gaps_out_of_order_and_missing_timestamps_are_not_hidden() {
        let a = reading("a", 0., "Pa", 3);
        let mut b = reading("b", 2., "Pa", 1);
        let mut options = SeriesOptions {
            max_gap_seconds: 1.,
            ..Default::default()
        };
        let result = series(&[&a, &b], Unit::Pa, Basis::FieldFactors, &options).unwrap();
        assert_eq!(result.backward_steps, 1);
        assert_eq!(result.minimum, 0.);
        options.time_weighted = true;
        assert!(series(&[&a, &b], Unit::Pa, Basis::FieldFactors, &options).is_err());
        b.timestamp.clear();
        options.time_weighted = false;
        let result = series(&[&a, &b], Unit::Pa, Basis::FieldFactors, &options).unwrap();
        assert_eq!(result.missing_times, 1);
        assert!(!result.clock_established);
        assert_eq!(result.maximum_gap_seconds, None);
    }

    #[test]
    fn summary_plot_preserves_spikes_and_temperature_dispersion_uses_delta_units() {
        let readings: Vec<_> = (0..1000)
            .map(|i| reading(&format!("r{i}"), if i == 501 { 30. } else { 20. }, "°C", 0))
            .collect();
        let refs: Vec<_> = readings.iter().collect();
        let result = series(
            &refs,
            Unit::Celsius,
            Basis::FieldFactors,
            &SeriesOptions::default(),
        )
        .unwrap();
        assert!(result.plot.len() <= 600);
        assert_eq!(result.maximum, 30.);
        assert!(result.plot.iter().any(|(_, _, high)| *high == 30.));
        assert_eq!(result.dispersion_unit, Unit::DeltaC);
        let single = series(
            &refs[..1],
            Unit::Celsius,
            Basis::FieldFactors,
            &SeriesOptions::default(),
        )
        .unwrap();
        assert!(single.sample_stddev.is_none());
    }

    #[test]
    fn calculated_input_snapshots_survive_native_project_and_csv_roundtrips() {
        let root =
            std::env::temp_dir().join(format!("green-suite-analysis-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let a = reading("a", 1.25, "inW", 0);
        let b = reading("b", 1., "inW", 1);
        let result = difference(&a, &b, &options(Unit::InWaterColumn)).unwrap();
        let mut project = Project::new("Example analysis");
        project.readings = vec![a, b, result.clone()];
        let path = root.join("example.green-suite.json");
        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        validate_inputs(&result, &loaded).unwrap();
        let csv = root.join("example.csv");
        loaded.export_csv(&csv).unwrap();
        let mut imported = Project::new("Imported");
        assert_eq!(imported.import_csv(&csv).unwrap(), 3);
        let imported_result = imported
            .readings
            .iter()
            .find(|r| r.id == result.id)
            .unwrap();
        validate_inputs(imported_result, &imported).unwrap();
        assert_eq!(
            serde_json::to_value(&imported_result.metadata).unwrap(),
            serde_json::to_value(&result.metadata).unwrap()
        );
        for path in [path, csv] {
            std::fs::remove_file(path).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn calculated_generation_times_cannot_masquerade_as_measurement_pairing() {
        let mut a = reading("a", 2., "Pa", 0);
        let mut b = reading("b", 1., "Pa", 1);
        a.metadata = Some(Metadata {
            reporter_serial: Some("EXAMPLE".into()),
            declared_clock_offset_seconds: Some(1),
            time_basis: TimeBasis::InstrumentLocal,
            ..Default::default()
        });
        a.timestamp.pop();
        b.timestamp.pop();
        b.metadata = Some(Metadata {
            reporter_serial: Some("EXAMPLE".into()),
            time_basis: TimeBasis::InstrumentLocal,
            ..Default::default()
        });
        let converted = a.converted(Unit::Pa, Unit::Kpa, false).unwrap();
        assert!(stamp(&converted).is_ok());
        assert_eq!(
            converted
                .metadata
                .as_ref()
                .unwrap()
                .declared_clock_offset_seconds,
            Some(1)
        );
        let summary = difference(&a, &b, &options(Unit::Pa)).unwrap();
        assert!(stamp(&summary).is_err());
        let converted_summary = summary.converted(Unit::Pa, Unit::Kpa, false).unwrap();
        assert!(stamp(&converted_summary).is_err());
        a.metadata.as_mut().unwrap().origin = Origin::Manual;
        assert!(stamp(&a).is_err());
        assert!(stamp(&a.converted(Unit::Pa, Unit::Kpa, false).unwrap()).is_err());
    }

    #[test]
    fn receipt_only_times_cannot_become_measurement_times_through_conversion() {
        let mut reading = reading("receipt", 1., "Pa", 0);
        reading.source = "wrist-reporter-live".into();
        reading.raw = serde_json::json!({"timestamp_source":"pc-received"}).to_string();
        assert!(stamp(&reading).is_err());
        assert!(stamp(&reading.converted(Unit::Pa, Unit::Kpa, false).unwrap()).is_err());
        reading.raw = serde_json::json!({"timestamp_source":"reporter-local"}).to_string();
        assert!(stamp(&reading).is_ok());
    }
    #[test]
    fn balance_requires_one_distinct_mapped_reading_per_device_and_preserves_design() {
        let mut project = Project::new("Example");
        project.readings = vec![reading("a", 90., "CFM", 0), reading("b", 110., "CFM", 1)];
        for (id, reading_id) in [("p1", "a"), ("p2", "b")] {
            project.points.push(crate::model::MapPoint {
                id: id.into(),
                label: id.into(),
                system: "Example".into(),
                design_value: 100.,
                unit: "CFM".into(),
                reading_ids: vec![reading_id.into()],
                ..Default::default()
            });
        }
        let mut choices = vec![
            BalanceChoice {
                point_id: "p1".into(),
                reading_id: Some("a".into()),
            },
            BalanceChoice {
                point_id: "p2".into(),
                reading_id: Some("b".into()),
            },
        ];
        let result = balance(
            &project,
            &choices,
            Unit::Cfm,
            Basis::FieldFactors,
            None,
            "Air",
        )
        .unwrap();
        assert!(result.complete);
        assert_eq!(result.measured_subtotal, 200.);
        assert!(result.rows[0].within_tolerance.is_none());
        validate_inputs(result.result.as_ref().unwrap(), &project).unwrap();
        project.points[0].design_value = 105.;
        assert!(validate_inputs(result.result.as_ref().unwrap(), &project).is_err());
        choices[1].reading_id = None;
        let partial = balance(
            &project,
            &choices,
            Unit::Cfm,
            Basis::FieldFactors,
            Some(5.),
            "Air",
        )
        .unwrap();
        assert!(!partial.complete);
        assert!(partial.result.is_none());
        assert_eq!(partial.measured_subtotal, 90.);
        project.points[1].reading_ids.push("a".into());
        choices[1].reading_id = Some("a".into());
        assert!(
            balance(
                &project,
                &choices,
                Unit::Cfm,
                Basis::FieldFactors,
                None,
                "Air"
            )
            .is_err()
        );
    }
}
