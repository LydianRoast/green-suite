// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Optional, backward-compatible interpretation and provenance. Unknown origin
//! stays unknown. Clock corrections are annotations, never source-time edits.

use crate::units::Quantity;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Unknown,
    Measured,
    InstrumentCalculated,
    ApplicationCalculated,
    Manual,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown origin",
            Self::Measured => "Measured",
            Self::InstrumentCalculated => "Instrument calculated",
            Self::ApplicationCalculated => "Application calculated",
            Self::Manual => "Manual entry",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeBasis {
    #[default]
    Unknown,
    InstrumentLocal,
    PcUtc,
}

impl TimeBasis {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unknown time basis",
            Self::InstrumentLocal => "Reporter local time (timezone unknown)",
            Self::PcUtc => "PC time (UTC)",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Metadata {
    pub reporter_serial: Option<String>,
    pub firmware: Option<String>,
    pub download_session: Option<String>,
    pub record_group: Option<String>,
    pub record_index: Option<u32>,
    pub sensor_address: Option<u8>,
    pub channel: Option<u16>,
    pub original_numeric: Option<String>,
    pub original_unit: Option<String>,
    pub parser_version: Option<String>,
    pub checksum: Option<String>,
    pub origin: Origin,
    pub quantity: Quantity,
    pub role: String,
    pub time_basis: TimeBasis,
    pub received_at_utc: Option<String>,
    pub declared_clock_offset_seconds: Option<i64>,
    pub calibration: Option<Calibration>,
    pub derivation: Option<Derivation>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Calibration {
    pub reference: String,
    pub calibrated_on: Option<String>,
    pub due_on: Option<String>,
    pub unit: String,
    pub range_min: Option<f64>,
    pub range_max: Option<f64>,
    pub uncertainty: Option<f64>,
    pub coverage_factor: Option<f64>,
    pub notes: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Derivation {
    pub formula: String,
    pub version: String,
    pub inputs: Vec<Input>,
    pub assumptions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub reading_id: String,
    pub value: f64,
    pub unit: String,
    pub timestamp: String,
    #[serde(default)]
    pub context: BTreeMap<String, String>,
}

impl Input {
    pub fn from_reading(reading: &crate::model::Reading) -> Result<Self> {
        let mut context = BTreeMap::new();
        context.insert("source".into(), reading.source.clone());
        if let Some(metadata) = &reading.metadata {
            metadata.validate(reading.value)?;
            let mut snapshot = metadata.clone();
            snapshot.derivation = None;
            context.insert("interpretation".into(), serde_json::to_string(&snapshot)?);
            if let Some(derived) = &metadata.derivation {
                context.insert("upstream_formula".into(), derived.formula.clone());
                context.insert("upstream_version".into(), derived.version.clone());
                context.insert(
                    "upstream_assumptions".into(),
                    serde_json::to_string(&derived.assumptions)?,
                );
            }
        }
        Ok(Self {
            reading_id: reading.id.clone(),
            value: reading.value,
            unit: reading.unit.clone(),
            timestamp: reading.timestamp.clone(),
            context,
        })
    }
}

impl Metadata {
    pub fn validate(&self, value: f64) -> Result<()> {
        if let Some(number) = &self.original_numeric {
            ensure!(
                number.len() <= 80
                    && number
                        .parse::<f64>()
                        .is_ok_and(|number| number.is_finite() && number == value),
                "Original numeric text does not match the preserved reading"
            );
        }
        ensure!(self.role.len() <= 1024, "Measurement role is too long");
        ensure!(
            self.origin != Origin::ApplicationCalculated || self.derivation.is_some(),
            "Application-calculated values require saved formula provenance"
        );
        if let Some(received) = &self.received_at_utc {
            let time = chrono::DateTime::parse_from_rfc3339(received)?;
            ensure!(
                time.offset().local_minus_utc() == 0,
                "Receipt timestamp must be UTC"
            );
        }
        if let Some(cal) = &self.calibration {
            for number in [
                cal.range_min,
                cal.range_max,
                cal.uncertainty,
                cal.coverage_factor,
            ]
            .into_iter()
            .flatten()
            {
                ensure!(
                    number.is_finite(),
                    "Calibration metadata contains a non-finite value"
                );
            }
            ensure!(
                cal.uncertainty.is_none_or(|v| v >= 0.)
                    && cal.coverage_factor.is_none_or(|v| v > 0.),
                "Invalid calibration uncertainty or coverage factor"
            );
            if let (Some(min), Some(max)) = (cal.range_min, cal.range_max) {
                ensure!(min <= max, "Calibration range is reversed");
            }
            let date = |value: &Option<String>| -> Result<Option<chrono::NaiveDate>> {
                value
                    .as_deref()
                    .map(|date| {
                        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(Into::into)
                    })
                    .transpose()
            };
            if let (Some(start), Some(end)) = (date(&cal.calibrated_on)?, date(&cal.due_on)?) {
                ensure!(end >= start, "Calibration due date precedes calibration");
            }
        }
        if let Some(derived) = &self.derivation {
            ensure!(
                self.origin == Origin::ApplicationCalculated,
                "Derived results must be labeled application calculated"
            );
            ensure!(
                !derived.formula.is_empty() && !derived.version.is_empty(),
                "Derived result requires a formula and version"
            );
            ensure!(
                derived.inputs.len() <= 100_000,
                "Too many derived result inputs"
            );
            for input in &derived.inputs {
                ensure!(
                    input.value.is_finite() && !input.reading_id.is_empty(),
                    "Invalid derived result input"
                );
            }
        }
        Ok(())
    }
}

impl crate::model::Reading {
    pub fn timestamp_is_measurement_candidate(&self) -> bool {
        if matches!(self.source.as_str(), "manual" | "traverse") {
            return false;
        }
        if self.source == "wrist-reporter-live"
            && !serde_json::from_str::<serde_json::Value>(&self.raw)
                .is_ok_and(|raw| raw["timestamp_source"] == "reporter-local")
        {
            return false;
        }
        match self.metadata.as_ref() {
            Some(metadata) if metadata.origin == Origin::Manual => false,
            Some(metadata) if metadata.origin == Origin::ApplicationCalculated => {
                metadata.derivation.as_ref().is_some_and(|d| {
                    d.formula == "unit-conversion"
                        && !d.assumptions.contains_key("measurement_time_unavailable")
                })
            }
            _ => true,
        }
    }
    pub fn converted(
        &self,
        from: crate::units::Unit,
        to: crate::units::Unit,
        interpreted: bool,
    ) -> Result<Self> {
        self.converted_using(from, to, interpreted, crate::standards::Basis::default())
    }

    pub fn converted_using(
        &self,
        from: crate::units::Unit,
        to: crate::units::Unit,
        interpreted: bool,
        basis: crate::standards::Basis,
    ) -> Result<Self> {
        use crate::units::Unit;
        if let Some(known) = Unit::parse(&self.unit) {
            ensure!(
                known == from,
                "Source unit cannot be reinterpreted when it has a known definition"
            );
        } else {
            ensure!(
                interpreted,
                "An ambiguous source unit requires explicit interpretation"
            );
        }
        if let Some(metadata) = &self.metadata {
            metadata.validate(self.value)?;
            ensure!(
                metadata.quantity == Quantity::Unknown || metadata.quantity == from.quantity(),
                "Declared quantity conflicts with the selected source unit"
            );
        }
        let (value, reference) = basis.calculate(self.value, from, to)?;
        let mut assumptions = BTreeMap::new();
        assumptions.insert("from_unit".into(), from.symbol().into());
        assumptions.insert("to_unit".into(), to.symbol().into());
        if self.unit != from.symbol() {
            assumptions.insert(
                "source_unit_definition".into(),
                format!("{} = {}", self.unit, from.symbol()),
            );
        }
        assumptions.insert("calculation_rule".into(), reference.into());
        assumptions.insert("calculation_basis".into(), basis.label().into());
        if !self.timestamp_is_measurement_candidate() {
            assumptions.insert("measurement_time_unavailable".into(),"Input timestamp represents entry/calculation time, not established measurement time".into());
        }
        if Unit::parse(&self.unit).is_none() {
            assumptions.insert(
                "source_unit_interpretation".into(),
                format!("{} interpreted as {}", self.unit, from.symbol()),
            );
        }
        let derivation = Derivation {
            formula: "unit-conversion".into(),
            version: "2".into(),
            inputs: vec![Input::from_reading(self)?],
            assumptions,
        };
        let time_basis = self
            .metadata
            .as_ref()
            .map(|m| m.time_basis)
            .unwrap_or_default();
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: self.timestamp.clone(),
            group: self.group.clone(),
            sensor: self.sensor.clone(),
            kind: format!("Converted {}", self.kind),
            value,
            unit: to.symbol().into(),
            source: "Green Suite unit conversion".into(),
            raw: serde_json::to_string(&derivation)?,
            metadata: Some(Metadata {
                reporter_serial: self
                    .metadata
                    .as_ref()
                    .and_then(|m| m.reporter_serial.clone()),
                firmware: self.metadata.as_ref().and_then(|m| m.firmware.clone()),
                declared_clock_offset_seconds: self
                    .metadata
                    .as_ref()
                    .and_then(|m| m.declared_clock_offset_seconds),
                origin: Origin::ApplicationCalculated,
                quantity: to.quantity(),
                time_basis,
                role: self
                    .metadata
                    .as_ref()
                    .map(|m| m.role.clone())
                    .unwrap_or_default(),
                derivation: Some(derivation),
                ..Default::default()
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::Reading, units::Unit};
    #[test]
    fn conversion_preserves_source_and_records_interpretation_and_inputs() {
        let reading = Reading {
            id: "source".into(),
            value: -0.01,
            unit: "unassigned-pressure-label".into(),
            timestamp: "2026-01-01T10:00:00".into(),
            ..Default::default()
        };
        let before = serde_json::to_value(&reading).unwrap();
        assert!(reading.converted(Unit::InWater4C, Unit::Pa, false).is_err());
        let converted = reading.converted(Unit::InWater4C, Unit::Pa, true).unwrap();
        assert!((converted.value + 2.49082).abs() < 1e-10);
        assert_eq!(before, serde_json::to_value(&reading).unwrap());
        let metadata = converted.metadata.unwrap();
        metadata.validate(converted.value).unwrap();
        assert_eq!(metadata.origin, Origin::ApplicationCalculated);
        assert_eq!(
            metadata.derivation.as_ref().unwrap().inputs[0].reading_id,
            "source"
        );
        assert!(
            metadata
                .derivation
                .unwrap()
                .assumptions
                .contains_key("source_unit_interpretation")
        );
    }
    #[test]
    fn known_units_and_declared_quantities_cannot_be_reinterpreted() {
        let mut reading = Reading {
            id: "source".into(),
            value: 1.,
            unit: "Pa".into(),
            ..Default::default()
        };
        assert!(reading.converted(Unit::Psi, Unit::Pa, true).is_err());
        reading.metadata = Some(Metadata {
            quantity: Quantity::Velocity,
            ..Default::default()
        });
        assert!(reading.converted(Unit::Pa, Unit::Kpa, true).is_err());
    }
    #[test]
    fn calibration_and_source_precision_are_validated() {
        let mut metadata = Metadata {
            original_numeric: Some("0.0000".into()),
            ..Default::default()
        };
        assert!(metadata.validate(0.).is_ok());
        assert!(metadata.validate(1.).is_err());
        metadata.calibration = Some(Calibration {
            range_min: Some(10.),
            range_max: Some(0.),
            ..Default::default()
        });
        assert!(metadata.validate(0.).is_err());
        metadata.calibration = Some(Calibration {
            calibrated_on: Some("2026-11-12".into()),
            due_on: Some("2026-11-11".into()),
            ..Default::default()
        });
        assert!(metadata.validate(0.).is_err());
        metadata.calibration = Some(Calibration {
            uncertainty: Some(-1.),
            ..Default::default()
        });
        assert!(metadata.validate(0.).is_err());
    }
}
