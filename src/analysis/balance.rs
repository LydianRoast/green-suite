// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;
pub struct BalanceChoice {
    pub point_id: String,
    pub reading_id: Option<String>,
}
pub struct BalanceRow {
    pub label: String,
    pub design: Option<f64>,
    pub measured: Option<f64>,
    pub percent: Option<f64>,
    pub within_tolerance: Option<bool>,
    pub issue: Option<String>,
}
pub struct BalanceResult {
    pub rows: Vec<BalanceRow>,
    pub measured_subtotal: f64,
    pub design_subtotal: f64,
    pub complete: bool,
    pub result: Option<Reading>,
}
pub fn balance(
    project: &Project,
    choices: &[BalanceChoice],
    target: Unit,
    basis: Basis,
    tolerance: Option<f64>,
    service: &str,
) -> Result<BalanceResult> {
    ensure!(
        !choices.is_empty() && choices.len() <= 400,
        "Select between 1 and 400 devices"
    );
    ensure!(
        target.quantity() == Quantity::VolumeFlow,
        "Balancing summaries require a volume-flow unit"
    );
    ensure!(
        matches!(service, "Air" | "Water"),
        "Explicitly choose Air or Water service"
    );
    ensure!(
        tolerance.is_none_or(|t| t.is_finite() && t >= 0.),
        "Acceptance tolerance must be finite and nonnegative"
    );
    let mut rows = Vec::new();
    let mut inputs = Vec::new();
    let mut used = HashSet::new();
    let mut points = HashSet::new();
    let mut measured_subtotal = 0.;
    let mut design_subtotal = 0.;
    for choice in choices {
        ensure!(
            points.insert(&choice.point_id),
            "A device cannot be included twice"
        );
        let point = project
            .points
            .iter()
            .find(|p| p.id == choice.point_id)
            .context("A selected device no longer exists")?;
        let mut row = BalanceRow {
            label: point.label.clone(),
            design: None,
            measured: None,
            percent: None,
            within_tolerance: None,
            issue: None,
        };
        row.design = Unit::parse(&point.unit)
            .and_then(|from| basis.calculate(point.design_value, from, target).ok())
            .map(|v| v.0)
            .filter(|v| v.is_finite() && *v > 0.);
        let reading = choice
            .reading_id
            .as_ref()
            .and_then(|id| project.readings.iter().find(|r| &r.id == id));
        if let Some(reading) = reading {
            ensure!(
                point.reading_ids.contains(&reading.id),
                "Selected reading is no longer mapped to its device"
            );
            ensure!(
                used.insert(&reading.id),
                "The same reading was selected for multiple devices"
            );
            match value_in(reading, target, basis) {
                Ok(value) => {
                    row.measured = Some(value);
                    inputs.push(reading);
                    measured_subtotal += value;
                }
                Err(error) => row.issue = Some(error.to_string()),
            }
        } else {
            row.issue = Some("Choose one mapped reading".into());
        }
        if let Some(design) = row.design {
            design_subtotal += design;
        } else if row.issue.is_none() {
            row.issue = Some("A positive design flow with a supported unit is required".into());
        }
        if let (Some(actual), Some(design)) = (row.measured, row.design) {
            let percent = actual / design * 100.;
            ensure!(percent.is_finite(), "Design percentage overflow");
            row.percent = Some(percent);
            row.within_tolerance = tolerance.map(|t| (percent - 100.).abs() <= t);
        }
        rows.push(row);
    }
    ensure!(
        measured_subtotal.is_finite() && design_subtotal.is_finite(),
        "System total overflow"
    );
    let complete = rows.iter().all(|r| r.issue.is_none());
    let result = if complete {
        let designs = choices
            .iter()
            .map(|c| {
                let p = project.points.iter().find(|p| p.id == c.point_id).unwrap();
                (
                    &p.id,
                    &p.label,
                    p.design_value,
                    &p.unit,
                    &p.system,
                    &c.reading_id,
                )
            })
            .collect::<Vec<_>>();
        let notes=BTreeMap::from([("unit_rule_version".into(),basis.rule_id().into()),("calculation_basis".into(),basis.label().into()),("service".into(),service.into()),("design_total".into(),design_subtotal.to_string()),
            ("acceptance_tolerance_percent".into(),tolerance.map(|t|t.to_string()).unwrap_or_else(||"Not specified".into())),("device_designs".into(),serde_json::to_string(&designs)?),
            ("time_alignment".into(),"Individually selected device snapshots; no simultaneous system snapshot is implied".into())]);
        let first_system = &project
            .points
            .iter()
            .find(|p| p.id == choices[0].point_id)
            .unwrap()
            .system;
        let common_system = choices.iter().all(|choice| {
            project
                .points
                .iter()
                .find(|p| p.id == choice.point_id)
                .is_some_and(|p| &p.system == first_system)
        });
        Some(derived(
            if common_system && !first_system.is_empty() {
                first_system
            } else {
                "Selected device summary"
            },
            "Selected device flow total",
            measured_subtotal,
            target,
            "device-balance-summary-v1",
            &inputs,
            notes,
        )?)
    } else {
        None
    };
    Ok(BalanceResult {
        rows,
        measured_subtotal,
        design_subtotal,
        complete,
        result,
    })
}
