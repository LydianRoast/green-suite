// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
//! Versioned calculation rules, not a claim of complete compliance.

use crate::units::{self, Quantity, Unit};
use anyhow::{Result, bail, ensure};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Basis {
    #[default]
    FieldFactors,
    PhysicalDefinitions,
}

impl Basis {
    pub fn rule_id(self) -> &'static str {
        match self {
            Self::FieldFactors => "field-conversions-v1",
            Self::PhysicalDefinitions => "unit-definitions-v1",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::FieldFactors => "Rounded field factors",
            Self::PhysicalDefinitions => "Physical unit definitions",
        }
    }
    pub fn calculate(self, value: f64, from: Unit, to: Unit) -> Result<(f64, &'static str)> {
        let physical = units::convert(value, from, to)?;
        if self == Self::PhysicalDefinitions {
            return Ok((physical, "unit-definitions-v1"));
        }
        if from == to {
            return Ok((value, "Identity: no unit conversion"));
        }
        let reference = "field-conversions-v1; forward factors and algebraic inverses";
        let factor = |unit: Unit| -> Result<f64> {
            Ok(match unit {
                Unit::Pa => 1.,
                Unit::Kpa => 1000.,
                Unit::Psi => 6890.,
                Unit::InWaterColumn | Unit::InWater4C => 249.082,
                Unit::Mps => 1.,
                Unit::Fpm => 0.00508,
                Unit::CubicMetresPerSecond => 1.,
                Unit::LitresPerSecond => 0.001,
                Unit::CubicMetresPerHour => 1. / 3600.,
                Unit::Cfm => 0.000472,
                Unit::UsGpm => 0.00379 / 60.,
                _ => bail!(
                    "This conversion is not available under the selected calculation basis. Choose a supported basis."
                ),
            })
        };
        let result = match from.quantity() {
            Quantity::Temperature
                if matches!(from, Unit::Celsius | Unit::Fahrenheit)
                    && matches!(to, Unit::Celsius | Unit::Fahrenheit) =>
            {
                return Ok((physical, reference));
            }
            Quantity::TemperatureDifference => {
                return Ok((
                    physical,
                    "temperature-difference-v1; no absolute-temperature offset",
                ));
            }
            Quantity::Pressure | Quantity::Velocity | Quantity::VolumeFlow => {
                value * factor(from)? / factor(to)?
            }
            Quantity::HumidityRatio => {
                return Ok((physical, "humidity-ratio-v1"));
            }
            _ => bail!(
                "This conversion is not available under the selected calculation basis. Choose a supported basis."
            ),
        };
        ensure!(result.is_finite(), "Conversion overflow");
        Ok((result, reference))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviewed_field_factors_are_distinct_from_physical_definitions() {
        let field = Basis::FieldFactors
            .calculate(1., Unit::Cfm, Unit::LitresPerSecond)
            .unwrap();
        assert!((field.0 - 0.472).abs() < 1e-12);
        let physical = Basis::PhysicalDefinitions
            .calculate(1., Unit::Cfm, Unit::LitresPerSecond)
            .unwrap();
        assert!((physical.0 - 0.4719474432).abs() < 1e-12);
        assert_ne!(field.1, physical.1);
        assert_eq!(
            Basis::FieldFactors
                .calculate(1., Unit::Psi, Unit::Kpa)
                .unwrap()
                .0,
            6.89
        );
        assert!(
            (Basis::FieldFactors
                .calculate(1., Unit::UsGpm, Unit::LitresPerSecond)
                .unwrap()
                .0
                - 3.79 / 60.)
                .abs()
                < 1e-12
        );
    }
    #[test]
    fn inverse_uses_reviewed_forward_factor_and_unsupported_rules_do_not_fall_back() {
        let field = Basis::FieldFactors;
        assert!(
            (field
                .calculate(249.082, Unit::Pa, Unit::InWaterColumn)
                .unwrap()
                .0
                - 1.)
                .abs()
                < 1e-12
        );
        assert!(field.calculate(1., Unit::InWater60F, Unit::Pa).is_err());
        assert!(field.calculate(1., Unit::Kelvin, Unit::Celsius).is_err());
        assert!(field.calculate(1., Unit::Pa, Unit::Fpm).is_err());
    }
}
