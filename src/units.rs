// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Explicit dimensions and unit definitions. inW means inches of water column;
//! GPM means gallons per minute. inWC uses a reviewed field conversion factor;
//! reference-temperature water columns and US liquid gallons also have explicit
//! physical definitions. standards.rs selects the field-factor basis separately.
//! Rule versions distinguish rounded factors from physical definitions.
//! Conversions do not imply improved instrument accuracy.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    #[default]
    Unknown,
    Pressure,
    Velocity,
    VolumeFlow,
    Temperature,
    TemperatureDifference,
    RelativeHumidity,
    SpecificEnthalpy,
    HumidityRatio,
    MassFlow,
    Power,
}

impl Quantity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "Unclassified",
            Self::Pressure => "Pressure",
            Self::Velocity => "Velocity",
            Self::VolumeFlow => "Volume flow",
            Self::Temperature => "Temperature",
            Self::TemperatureDifference => "Temperature difference",
            Self::RelativeHumidity => "Relative humidity",
            Self::SpecificEnthalpy => "Specific enthalpy",
            Self::HumidityRatio => "Humidity ratio",
            Self::MassFlow => "Mass flow",
            Self::Power => "Power",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Pa,
    Kpa,
    Psi,
    InWaterColumn,
    InWater4C,
    InWater60F,
    InMercury0C,
    Mps,
    Fpm,
    CubicMetresPerSecond,
    LitresPerSecond,
    CubicMetresPerHour,
    Cfm,
    UsGpm,
    Celsius,
    Fahrenheit,
    Kelvin,
    DeltaC,
    DeltaF,
    DeltaK,
    RhPercent,
    KjPerKg,
    JPerKg,
    KgPerKg,
    GrainsPerPound,
    KgPerSecond,
    Watt,
    Kilowatt,
}

impl Unit {
    pub const ALL: &'static [Self] = &[
        Self::Pa,
        Self::Kpa,
        Self::Psi,
        Self::InWaterColumn,
        Self::InWater4C,
        Self::InWater60F,
        Self::InMercury0C,
        Self::Mps,
        Self::Fpm,
        Self::CubicMetresPerSecond,
        Self::LitresPerSecond,
        Self::CubicMetresPerHour,
        Self::Cfm,
        Self::UsGpm,
        Self::Celsius,
        Self::Fahrenheit,
        Self::Kelvin,
        Self::DeltaC,
        Self::DeltaF,
        Self::DeltaK,
        Self::RhPercent,
        Self::KjPerKg,
        Self::JPerKg,
        Self::KgPerKg,
        Self::GrainsPerPound,
        Self::KgPerSecond,
        Self::Watt,
        Self::Kilowatt,
    ];
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Pa => "Pa",
            Self::Kpa => "kPa",
            Self::Psi => "psi",
            Self::InWaterColumn => "inWC",
            Self::InWater4C => "inH2O (4°C)",
            Self::InWater60F => "inH2O (60°F)",
            Self::InMercury0C => "inHg (0°C)",
            Self::Mps => "m/s",
            Self::Fpm => "FPM",
            Self::CubicMetresPerSecond => "m³/s",
            Self::LitresPerSecond => "L/s",
            Self::CubicMetresPerHour => "m³/h",
            Self::Cfm => "CFM",
            Self::UsGpm => "US gpm",
            Self::Celsius => "°C",
            Self::Fahrenheit => "°F",
            Self::Kelvin => "K",
            Self::DeltaC => "Δ°C",
            Self::DeltaF => "Δ°F",
            Self::DeltaK => "ΔK",
            Self::RhPercent => "%RH",
            Self::KjPerKg => "kJ/kg",
            Self::JPerKg => "J/kg",
            Self::KgPerKg => "kg/kg",
            Self::GrainsPerPound => "gr/lb",
            Self::KgPerSecond => "kg/s",
            Self::Watt => "W",
            Self::Kilowatt => "kW",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|unit| unit.symbol() == text.trim())
            .or_else(|| match text.trim() {
                "PSI" => Some(Self::Psi),
                "inW" => Some(Self::InWaterColumn),
                "GPM" | "gpm" => Some(Self::UsGpm),
                "fpm" => Some(Self::Fpm),
                "cfm" => Some(Self::Cfm),
                "m3/s" => Some(Self::CubicMetresPerSecond),
                "m3/h" => Some(Self::CubicMetresPerHour),
                "C" | "°Cd" | "°Cw" => Some(Self::Celsius),
                "F" | "°Fd" | "°Fw" => Some(Self::Fahrenheit),
                _ => None,
            })
    }
    pub fn quantity(self) -> Quantity {
        match self {
            Self::Pa
            | Self::Kpa
            | Self::Psi
            | Self::InWaterColumn
            | Self::InWater4C
            | Self::InWater60F
            | Self::InMercury0C => Quantity::Pressure,
            Self::Mps | Self::Fpm => Quantity::Velocity,
            Self::CubicMetresPerSecond
            | Self::LitresPerSecond
            | Self::CubicMetresPerHour
            | Self::Cfm
            | Self::UsGpm => Quantity::VolumeFlow,
            Self::Celsius | Self::Fahrenheit | Self::Kelvin => Quantity::Temperature,
            Self::DeltaC | Self::DeltaF | Self::DeltaK => Quantity::TemperatureDifference,
            Self::RhPercent => Quantity::RelativeHumidity,
            Self::KjPerKg | Self::JPerKg => Quantity::SpecificEnthalpy,
            Self::KgPerKg | Self::GrainsPerPound => Quantity::HumidityRatio,
            Self::KgPerSecond => Quantity::MassFlow,
            Self::Watt | Self::Kilowatt => Quantity::Power,
        }
    }
    fn factor(self) -> f64 {
        match self {
            Self::Pa
            | Self::Mps
            | Self::CubicMetresPerSecond
            | Self::Kelvin
            | Self::DeltaC
            | Self::DeltaK
            | Self::RhPercent
            | Self::JPerKg
            | Self::KgPerKg
            | Self::KgPerSecond
            | Self::Watt => 1.,
            Self::Kpa | Self::KjPerKg | Self::Kilowatt => 1000.,
            Self::Psi => 6894.757,
            Self::InWaterColumn | Self::InWater4C => 249.082,
            Self::InWater60F => 248.84,
            Self::InMercury0C => 3386.389,
            Self::Fpm => 0.3048 / 60.,
            Self::LitresPerSecond => 0.001,
            Self::CubicMetresPerHour => 1. / 3600.,
            Self::Cfm => 0.028316846592 / 60.,
            Self::UsGpm => 0.003785411784 / 60.,
            Self::Celsius => 1.,
            Self::Fahrenheit | Self::DeltaF => 5. / 9.,
            Self::GrainsPerPound => 1. / 7000.,
        }
    }
    fn offset(self) -> f64 {
        match self {
            Self::Celsius => 273.15,
            Self::Fahrenheit => 273.15 - 32. * 5. / 9.,
            _ => 0.,
        }
    }
}

pub fn convert(value: f64, from: Unit, to: Unit) -> Result<f64> {
    ensure!(value.is_finite(), "Conversion requires a finite value");
    ensure!(
        from.quantity() == to.quantity(),
        "Incompatible physical quantities; absolute temperatures and temperature differences are separate"
    );
    let base = value * from.factor() + from.offset();
    ensure!(base.is_finite(), "Conversion overflow");
    if from.quantity() == Quantity::Temperature {
        ensure!(base >= 0., "Temperature is below absolute zero");
    }
    if from.quantity() == Quantity::RelativeHumidity {
        ensure!(
            (0.0..=100.).contains(&base),
            "Relative humidity must be between 0 and 100 percent"
        );
    }
    let result = (base - to.offset()) / to.factor();
    ensure!(result.is_finite(), "Conversion overflow");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absolute_and_difference_temperatures_cannot_be_confused() {
        assert!((convert(32., Unit::Fahrenheit, Unit::Celsius).unwrap()).abs() < 1e-10);
        assert!((convert(18., Unit::DeltaF, Unit::DeltaC).unwrap() - 10.).abs() < 1e-10);
        assert!(convert(18., Unit::DeltaF, Unit::Celsius).is_err());
        assert!(convert(-274., Unit::Celsius, Unit::Kelvin).is_err());
    }
    #[test]
    fn pressure_velocity_and_volume_flow_reference_cases() {
        assert!((convert(1., Unit::InWater4C, Unit::Pa).unwrap() - 249.082).abs() < 1e-9);
        assert!((convert(1., Unit::InWater60F, Unit::Pa).unwrap() - 248.84).abs() < 1e-9);
        assert!((convert(100., Unit::Fpm, Unit::Mps).unwrap() - 0.508).abs() < 1e-12);
        assert!(
            (convert(1., Unit::Cfm, Unit::LitresPerSecond).unwrap() - 0.4719474432).abs() < 1e-12
        );
        assert!(
            (convert(1., Unit::UsGpm, Unit::LitresPerSecond).unwrap() - 0.0630901964).abs() < 1e-12
        );
        assert!(convert(1., Unit::Pa, Unit::Fpm).is_err());
    }
    #[test]
    fn ambiguous_labels_and_nonfinite_values_are_not_guessed() {
        for label in ["CF:c", "DP", "inHg"] {
            assert!(Unit::parse(label).is_none());
        }
        assert!(convert(f64::NAN, Unit::Pa, Unit::Pa).is_err());
        assert!(convert(f64::MAX, Unit::Kpa, Unit::Pa).is_err());
        assert!(convert(101., Unit::RhPercent, Unit::RhPercent).is_err());
        assert_eq!(convert(-1., Unit::Pa, Unit::Pa).unwrap(), -1.);
        assert_eq!(Unit::parse("inW"), Some(Unit::InWaterColumn));
        assert_eq!(Unit::parse("GPM"), Some(Unit::UsGpm));
    }
    #[test]
    fn all_units_roundtrip_within_their_dimension() {
        for &from in Unit::ALL {
            for &to in Unit::ALL {
                if from.quantity() == to.quantity() {
                    let converted = convert(20., from, to).unwrap();
                    assert!(
                        (convert(converted, to, from).unwrap() - 20.).abs() < 1e-8,
                        "{from:?} -> {to:?}"
                    );
                }
            }
        }
    }
}
