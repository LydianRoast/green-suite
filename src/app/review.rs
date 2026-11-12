// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.
use super::*;
use crate::{
    measurement::{Calibration, Metadata},
    units::Unit,
};

#[derive(Default)]
pub(super) struct ReviewState {
    reading_id: String,
    metadata: Metadata,
    source_unit: Option<Unit>,
    target_unit: Option<Unit>,
    interpreted: bool,
    expanded: bool,
    basis: crate::standards::Basis,
}

impl GreenSuite {
    pub(super) fn review_measurement(&mut self, ui: &mut Ui, reading: &Reading) {
        if self.measurement_review.reading_id != reading.id {
            self.measurement_review = ReviewState {
                reading_id: reading.id.clone(),
                metadata: reading.metadata.clone().unwrap_or_default(),
                source_unit: Unit::parse(&reading.unit),
                ..Default::default()
            };
        }
        let mut save_metadata = false;
        let mut save_conversion = None;
        let review = &mut self.measurement_review;
        CollapsingHeader::new("Measurement details")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(metadata) = &reading.metadata {
                    ui.label(metadata.origin.label());
                    ui.label(format!(
                        "{} · {}",
                        metadata.quantity.label(),
                        metadata.time_basis.label()
                    ));
                    if let Some(serial) = &metadata.reporter_serial {
                        ui.label(format!("Reporter: {serial}"));
                    }
                    if let Some(firmware) = &metadata.firmware {
                        ui.label(format!("Firmware: {firmware}"));
                    }
                    if let Some(session) = &metadata.download_session {
                        ui.label(format!("Download session: {session}"));
                    }
                    if let Some(index) = metadata.record_index {
                        ui.label(format!(
                            "Stored record: {index} · Sensor: {} · Channel: {}",
                            metadata
                                .sensor_address
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "Unknown".into()),
                            metadata
                                .channel
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "Unknown".into())
                        ));
                    }
                    if let Some(checksum) = &metadata.checksum {
                        ui.label(format!("Record checksum: {checksum}"));
                    }
                    if let Some(received) = &metadata.received_at_utc {
                        ui.label(format!("PC received: {received}"));
                    }
                    if let Some(original) = &metadata.original_numeric {
                        ui.label(format!(
                            "Original: {original} {}",
                            metadata.original_unit.as_deref().unwrap_or("")
                        ));
                    }
                    if let Some(derived) = &metadata.derivation {
                        ui.label(format!(
                            "Formula: {} · version {}",
                            derived.formula, derived.version
                        ));
                        ui.collapsing(format!("{} calculation inputs",derived.inputs.len()),|ui| {
                            ScrollArea::vertical().id_salt("calculation-inputs").max_height(160.).show_rows(ui,24.,derived.inputs.len(),|ui,range| {
                                for index in range {let input=&derived.inputs[index];
                                    let text=format!("{}: {} {} · {}",input.reading_id,calculated_value(input.value),input.unit,input.timestamp);
                                    ui.add(Label::new(&text).truncate()).on_hover_text(text);
                                }
                            });
                        });
                        for (name, value) in &derived.assumptions {
                            let end=value.char_indices().nth(400).map(|(i,_)|i).unwrap_or(value.len());
                            ui.label(format!("{name}: {}{}",&value[..end],if end<value.len(){"… (full value in source record)"}else{""}));
                        }
                    }
                } else {
                    ui.label("Legacy record: detailed provenance is unavailable.");
                }
                ui.collapsing("Role and calibration notes", |ui| {
                    ui.label("Measurement role / location");
                    ui.text_edit_singleline(&mut review.metadata.role);
                    let mut corrected=review.metadata.declared_clock_offset_seconds.is_some();
                    if ui.checkbox(&mut corrected,"Declare a clock correction").changed() {review.metadata.declared_clock_offset_seconds=corrected.then_some(0);}
                    if let Some(seconds)=&mut review.metadata.declared_clock_offset_seconds {
                        ui.horizontal(|ui|{ui.label("Seconds added during analysis");ui.add(DragValue::new(seconds).speed(1.));});
                        ui.label("This annotates analysis time; it does not change the source timestamp or instrument clock.");
                    }
                    let mut has_calibration = review.metadata.calibration.is_some();
                    if ui
                        .checkbox(&mut has_calibration, "Record a calibration reference")
                        .changed()
                    {
                        review.metadata.calibration = has_calibration.then(|| Calibration {
                            unit: reading.unit.clone(),
                            ..Default::default()
                        });
                    }
                    if let Some(cal) = &mut review.metadata.calibration {
                        ui.label("Reference (user supplied; verification is not implied)");
                        ui.text_edit_singleline(&mut cal.reference);
                        ui.horizontal_wrapped(|ui| {
                            optional_date(ui, "Calibrated", &mut cal.calibrated_on);
                            optional_date(ui, "Due", &mut cal.due_on);
                        });
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Unit");
                            ui.add(TextEdit::singleline(&mut cal.unit).desired_width(90.));
                            optional_number(ui, "Range minimum", &mut cal.range_min);
                            optional_number(ui, "Range maximum", &mut cal.range_max);
                        });
                        ui.horizontal_wrapped(|ui| {
                            optional_number(ui, "Uncertainty (±)", &mut cal.uncertainty);
                            optional_number(ui, "Coverage factor", &mut cal.coverage_factor);
                        });
                        ui.add(TextEdit::multiline(&mut cal.notes).hint_text("Calibration notes"));
                    }
                    ui.label("Absent dates, ranges and uncertainty remain unknown.");
                    save_metadata = ui.button("Save measurement notes").clicked();
                });
            });
        CollapsingHeader::new("Convert units")
            .default_open(review.expanded)
            .show(ui, |ui| {
                ComboBox::from_id_salt("conversion-basis")
                    .selected_text(review.basis.label())
                    .show_ui(ui, |ui| {
                        for basis in [
                            crate::standards::Basis::FieldFactors,
                            crate::standards::Basis::PhysicalDefinitions,
                        ] {
                            ui.selectable_value(&mut review.basis, basis, basis.label());
                        }
                    });
                let known = Unit::parse(&reading.unit);
                if let Some(known) = known {
                    ui.label(format!("Source: {}", known.symbol()));
                } else {
                    ui.label("This unit label has no confirmed conversion definition.");
                    ui.checkbox(
                        &mut review.interpreted,
                        "I will explicitly interpret the source unit",
                    );
                    ui.add_enabled_ui(review.interpreted, |ui| {
                        ComboBox::from_id_salt("source-conversion-unit")
                            .selected_text(
                                review
                                    .source_unit
                                    .map(Unit::symbol)
                                    .unwrap_or("Choose definition"),
                            )
                            .show_ui(ui, |ui| {
                                for &unit in Unit::ALL {
                                    ui.selectable_value(
                                        &mut review.source_unit,
                                        Some(unit),
                                        unit.symbol(),
                                    );
                                }
                            });
                    });
                }
                if let Some(from) = review.source_unit {
                    if review
                        .target_unit
                        .is_none_or(|unit| unit.quantity() != from.quantity())
                    {
                        review.target_unit = Some(from);
                    }
                    ComboBox::from_id_salt("target-conversion-unit")
                        .selected_text(review.target_unit.unwrap().symbol())
                        .show_ui(ui, |ui| {
                            for &unit in Unit::ALL {
                                if unit.quantity() == from.quantity() {
                                    ui.selectable_value(
                                        &mut review.target_unit,
                                        Some(unit),
                                        unit.symbol(),
                                    );
                                }
                            }
                        });
                    let to = review.target_unit.unwrap();
                    let converted = if review.basis == crate::standards::Basis::FieldFactors {
                        reading.converted(from, to, review.interpreted)
                    } else {
                        reading.converted_using(from, to, review.interpreted, review.basis)
                    };
                    match converted {
                        Ok(converted) => {
                            ui.strong(format!(
                                "{} {}",
                                calculated_value(converted.value),
                                converted.unit
                            ))
                            .on_hover_text(format!("Stored numeric value: {}", converted.value));
                            ui.label("Saving keeps the source and records the calculation inputs.");
                            if ui
                                .add_enabled(from != to, Button::new("Save converted reading"))
                                .clicked()
                            {
                                save_conversion = Some(converted);
                            }
                        }
                        Err(error) => {
                            ui.label(
                                RichText::new(error.to_string()).color(palette(ui.ctx()).warning),
                            );
                        }
                    }
                }
            });
        if save_metadata {
            match self.measurement_review.metadata.validate(reading.value) {
                Ok(()) => {
                    if let Some(stored) = self
                        .project
                        .readings
                        .iter_mut()
                        .find(|stored| stored.id == reading.id)
                    {
                        stored.metadata = Some(self.measurement_review.metadata.clone());
                        self.source_details = Some(stored.clone());
                        self.changed();
                        self.message("Measurement notes saved.", false);
                    }
                }
                Err(error) => self.message(error.to_string(), true),
            }
        }
        if let Some(converted) = save_conversion {
            match self.project.merge_readings(vec![converted.clone()]) {
                Ok(0) => self.message("This converted reading is already stored.", false),
                Ok(_) => {
                    self.selected_reading = Some(converted.id.clone());
                    self.changed();
                    self.message(
                        "Converted reading saved with its source and assumptions.",
                        false,
                    );
                }
                Err(error) => self.message(error.to_string(), true),
            }
        }
    }
}

impl GreenSuite {
    pub(super) fn preview_source_review(&mut self) {
        if let Some(reading) = self.project.readings.first() {
            self.source_details = Some(reading.clone());
            self.measurement_review = ReviewState {
                reading_id: reading.id.clone(),
                metadata: reading.metadata.clone().unwrap_or_default(),
                source_unit: Unit::parse(&reading.unit),
                target_unit: Some(Unit::LitresPerSecond),
                expanded: true,
                ..Default::default()
            };
        }
    }
}

fn optional_date(ui: &mut Ui, label: &str, value: &mut Option<String>) {
    let mut enabled = value.is_some();
    if ui.checkbox(&mut enabled, label).changed() {
        *value = enabled.then(String::new);
    }
    if let Some(value) = value {
        ui.add(
            TextEdit::singleline(value)
                .hint_text("YYYY-MM-DD")
                .desired_width(105.),
        );
    }
}
fn optional_number(ui: &mut Ui, label: &str, value: &mut Option<f64>) {
    let mut enabled = value.is_some();
    if ui.checkbox(&mut enabled, label).changed() {
        *value = enabled.then_some(0.);
    }
    if let Some(value) = value {
        ui.add(DragValue::new(value).speed(0.01));
    }
}
