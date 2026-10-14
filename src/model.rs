// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_PROJECT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_CSV_BYTES: u64 = 50 * 1024 * 1024;
const MAX_READINGS: usize = 500_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub documents: Vec<Document>,
    pub points: Vec<MapPoint>,
    pub readings: Vec<Reading>,
    pub traverses: Vec<Traverse>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub path: String,
    pub page_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MapPoint {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub system: String,
    pub document_id: String,
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub design_value: f64,
    pub unit: String,
    pub notes: String,
    pub reading_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Reading {
    pub id: String,
    pub timestamp: String,
    pub group: String,
    pub sensor: String,
    pub kind: String,
    pub value: f64,
    pub unit: String,
    pub source: String,
    pub raw: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Traverse {
    pub id: String,
    pub name: String,
    pub shape: String,
    /// Dimensions are inches; velocities are feet per minute.
    pub width: f64,
    pub height: f64,
    pub diameter: f64,
    pub rows: usize,
    pub columns: usize,
    pub velocities: Vec<Option<f64>>,
    pub point_id: Option<String>,
}

impl Default for Traverse {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: "New traverse".into(),
            shape: "rectangular".into(),
            width: 24.0,
            height: 18.0,
            diameter: 18.0,
            rows: 5,
            columns: 5,
            velocities: vec![None; 25],
            point_id: None,
        }
    }
}

pub fn data_dir() -> PathBuf {
    let path = std::env::var_os("GREEN_SUITE_DATA_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let executable_dir = std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            workspace_for_executable_dir(&executable_dir)
        });
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join(path)
    }
}

fn workspace_for_executable_dir(executable_dir: &Path) -> PathBuf {
    if executable_dir.join("Cargo.toml").is_file() {
        return executable_dir.join("workspace");
    }
    let named = |directory: &Path, expected: &str| {
        directory
            .file_name()
            .is_some_and(|part| part.to_string_lossy().eq_ignore_ascii_case(expected))
    };
    let profile_dir = if named(executable_dir, "deps") {
        executable_dir.parent().unwrap_or(executable_dir)
    } else {
        executable_dir
    };
    // Only the standard Cargo output layout implies a repository executable.
    // A standalone binary nested beneath an unrelated Cargo.toml remains standalone.
    if named(profile_dir, "debug") || named(profile_dir, "release") {
        if let Some(parent) = profile_dir.parent() {
            let target = if named(parent, "target") {
                Some(parent)
            } else {
                let is_target_triple = parent
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.contains('-')
                            && name
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                    });
                parent
                    .parent()
                    .filter(|directory| is_target_triple && named(directory, "target"))
            };
            if let Some(repository) = target
                .and_then(Path::parent)
                .filter(|directory| directory.join("Cargo.toml").is_file())
            {
                return repository.join("workspace");
            }
        }
    }
    executable_dir.join("workspace")
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: name.trim().to_string(),
            ..Self::default()
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = read_bounded(path, MAX_PROJECT_BYTES)?;
        let mut project: Self =
            serde_json::from_slice(&bytes).context("This is not a valid Green Suite project")?;
        let project_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .context("Cannot resolve the project folder")?
                .join(path)
        };
        // Relative document paths remain useful when opening a copied project folder.
        for document in &mut project.documents {
            if !document.path.is_empty() && Path::new(&document.path).is_relative() {
                document.path = project_path
                    .parent()
                    .unwrap_or(Path::new("."))
                    .join(&document.path)
                    .to_string_lossy()
                    .into_owned();
            }
        }
        project.validate()?;
        Ok(project)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self).context("Cannot serialize project")?;
        ensure!(
            bytes.len() as u64 <= MAX_PROJECT_BYTES,
            "Project exceeds the 128 MiB limit"
        );
        atomic_write(path, &bytes)
    }

    /// Copies an original PDF into managed local storage before adding its metadata.
    pub fn import_document(&mut self, source: &Path) -> Result<String> {
        ensure!(
            self.documents.len() < 500,
            "A project may contain at most 500 PDFs"
        );
        let project_id =
            Uuid::parse_str(&self.id).context("Project ID must be a UUID to import a blueprint")?;
        let pdf = crate::pdf::PdfDocument::open(source)?;
        let id = Uuid::new_v4().to_string();
        let folder = data_dir()
            .join("projects")
            .join(project_id.to_string())
            .join("blueprints");
        fs::create_dir_all(&folder).context("Cannot create blueprint storage")?;
        let destination = folder.join(format!("{id}.pdf"));
        let bytes = read_bounded(source, crate::pdf::MAX_PDF_BYTES)?;
        atomic_write(&destination, &bytes)?;
        self.documents.push(Document {
            id: id.clone(),
            name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: destination.to_string_lossy().into_owned(),
            page_count: pdf.page_count(),
        });
        Ok(id)
    }

    pub fn import_csv(&mut self, path: &Path) -> Result<usize> {
        let bytes = read_bounded(path, MAX_CSV_BYTES)?;
        let source = path.file_name().unwrap_or_default().to_string_lossy();
        let rows = parse_csv(&bytes, &source)?;
        self.merge_readings(rows)
    }

    /// Appends new records without disturbing existing point associations.
    pub fn merge_readings(&mut self, incoming: Vec<Reading>) -> Result<usize> {
        let mut ids: HashSet<String> = self.readings.iter().map(|r| r.id.clone()).collect();
        let mut keys: HashSet<String> = self.readings.iter().map(reading_key).collect();
        let mut additions = Vec::new();
        for mut reading in incoming {
            ensure!(
                reading.value.is_finite(),
                "A reading contains a non-finite value"
            );
            let key = reading_key(&reading);
            if keys.contains(&key) {
                continue;
            }
            if reading.id.is_empty() {
                reading.id = Uuid::new_v4().to_string();
            }
            // Preserve both records if a foreign import reused an ID for different data.
            if ids.contains(&reading.id) {
                reading.id = Uuid::new_v4().to_string();
            }
            ensure_id(&reading.id)?;
            ids.insert(reading.id.clone());
            keys.insert(key);
            additions.push(reading);
        }
        ensure!(
            self.readings.len() + additions.len() <= MAX_READINGS,
            "Project exceeds 500,000 readings"
        );
        let count = additions.len();
        self.readings.extend(additions);
        Ok(count)
    }

    /// Long-form export includes point labels for each associated reading.
    pub fn export_csv(&self, path: &Path) -> Result<()> {
        self.validate()?;
        let mut writer = csv::Writer::from_writer(Vec::new());
        writer.write_record([
            "id",
            "timestamp",
            "group",
            "sensor",
            "kind",
            "value",
            "unit",
            "source",
            "raw",
            "points",
            "green_suite_csv_version",
        ])?;
        let mut point_labels: HashMap<&str, Vec<&str>> = HashMap::new();
        for point in &self.points {
            for reading_id in &point.reading_ids {
                point_labels
                    .entry(reading_id)
                    .or_default()
                    .push(&point.label);
            }
        }
        for reading in &self.readings {
            let points = point_labels
                .get(reading.id.as_str())
                .map(|labels| labels.join("; "))
                .unwrap_or_default();
            writer.write_record([
                csv_text(&reading.id),
                csv_text(&reading.timestamp),
                csv_text(&reading.group),
                csv_text(&reading.sensor),
                csv_text(&reading.kind),
                reading.value.to_string(),
                csv_text(&reading.unit),
                csv_text(&reading.source),
                csv_text(&reading.raw),
                csv_text(&points),
                "1".to_string(),
            ])?;
        }
        let bytes = writer.into_inner().context("Cannot finish CSV export")?;
        atomic_write(path, &bytes)
    }

    pub fn validate(&self) -> Result<()> {
        Uuid::parse_str(&self.id).context("Project ID must be a UUID")?;
        ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 1024,
            "Project name must contain 1–1024 bytes"
        );
        ensure!(
            self.documents.len() <= 500
                && self.points.len() <= 100_000
                && self.readings.len() <= MAX_READINGS
                && self.traverses.len() <= 10_000,
            "Project contains too many items"
        );
        unique_ids(self.documents.iter().map(|x| x.id.as_str()))?;
        unique_ids(self.points.iter().map(|x| x.id.as_str()))?;
        unique_ids(self.readings.iter().map(|x| x.id.as_str()))?;
        unique_ids(self.traverses.iter().map(|x| x.id.as_str()))?;
        let reading_ids: HashSet<&str> = self.readings.iter().map(|x| x.id.as_str()).collect();
        let point_ids: HashSet<&str> = self.points.iter().map(|x| x.id.as_str()).collect();
        for doc in &self.documents {
            ensure_local_document_path(Path::new(&doc.path))?;
            // An empty path denotes an in-app illustrative sheet rather than a PDF.
            ensure!(
                (1..=10_000).contains(&doc.page_count),
                "Invalid blueprint metadata"
            );
        }
        for reading in &self.readings {
            ensure!(
                reading.value.is_finite(),
                "Reading {} contains a non-finite value",
                reading.id
            );
        }
        for point in &self.points {
            ensure!(
                point.x.is_finite()
                    && point.y.is_finite()
                    && (0.0..=1.0).contains(&point.x)
                    && (0.0..=1.0).contains(&point.y),
                "Point {} lies outside its blueprint",
                point.label
            );
            ensure!(
                point.design_value.is_finite(),
                "Invalid design value for {}",
                point.label
            );
            let document = self
                .documents
                .iter()
                .find(|d| d.id == point.document_id)
                .with_context(|| format!("Point {} references a missing blueprint", point.label))?;
            ensure!(
                point.page < document.page_count,
                "Point {} references a missing page",
                point.label
            );
            ensure!(
                point
                    .reading_ids
                    .iter()
                    .all(|id| reading_ids.contains(id.as_str())),
                "Point {} references a missing reading",
                point.label
            );
            unique_ids(point.reading_ids.iter().map(String::as_str))?;
        }
        for traverse in &self.traverses {
            ensure!(
                ["rectangular", "circular", "circle"]
                    .iter()
                    .any(|shape| traverse.shape.eq_ignore_ascii_case(shape)),
                "Unknown traverse shape"
            );
            let count = traverse
                .rows
                .checked_mul(traverse.columns)
                .context("Traverse layout is too large")?;
            ensure!(
                (1..=400).contains(&count),
                "Traverse layout must contain between 1 and 400 sample points"
            );
            ensure!(
                traverse.velocities.len() == count,
                "Traverse velocity count does not match its layout"
            );
            ensure!(
                [traverse.width, traverse.height, traverse.diameter]
                    .iter()
                    .all(|v| v.is_finite() && *v > 0.0),
                "Traverse dimensions must be positive finite inches"
            );
            ensure!(
                traverse
                    .velocities
                    .iter()
                    .flatten()
                    .all(|v| v.is_finite() && *v >= 0.0),
                "Traverse velocities must be nonnegative finite feet per minute"
            );
            ensure!(
                traverse
                    .point_id
                    .as_ref()
                    .is_none_or(|id| point_ids.contains(id.as_str())),
                "Traverse references a missing point"
            );
        }
        Ok(())
    }
}

fn ensure_local_document_path(path: &Path) -> Result<()> {
    ensure!(
        !path.as_os_str().to_string_lossy().contains('\0'),
        "Blueprint path contains an invalid character"
    );
    #[cfg(windows)]
    if let Some(std::path::Component::Prefix(prefix)) = path.components().next() {
        ensure!(
            matches!(
                prefix.kind(),
                std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
            ),
            "Project blueprint references must use local disk paths; import a shared PDF to create a local copy"
        );
    }
    Ok(())
}

fn ensure_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 256
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')),
        "Invalid item ID"
    );
    Ok(())
}

fn unique_ids<'a>(values: impl Iterator<Item = &'a str>) -> Result<()> {
    let mut seen = HashSet::new();
    for id in values {
        ensure_id(id)?;
        ensure!(seen.insert(id), "Duplicate item ID: {id}");
    }
    Ok(())
}

fn reading_key(r: &Reading) -> String {
    // JSON preserves field boundaries, unlike delimiter-concatenated keys.
    if r.id.starts_with("wr:") {
        // Device serial/group/record/sensor/channel identity is stable. A transient
        // status byte in the raw transfer must not duplicate the same measurement.
        return serde_json::to_string(&(
            &r.id,
            &r.timestamp,
            &r.group,
            &r.sensor,
            &r.kind,
            r.value,
            &r.unit,
            &r.source,
        ))
        .unwrap_or_default();
    }
    serde_json::to_string(&(
        &r.timestamp,
        &r.group,
        &r.sensor,
        &r.kind,
        r.value,
        &r.unit,
        &r.source,
        &r.raw,
    ))
    .unwrap_or_default()
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= maximum,
        "File exceeds the {} MiB limit",
        maximum / 1024 / 1024
    );
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= maximum,
        "File grew beyond the permitted size"
    );
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).context("Cannot create the destination folder")?;
    let temp = parent.join(format!(".green-suite-{}.tmp", Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Rust's Windows implementation uses MoveFileExW(REPLACE_EXISTING).
        // The old file remains intact if replacement fails (e.g. a sharing lock).
        fs::rename(&temp, path).with_context(|| format!("Cannot replace {}", path.display()))?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn normalized_header(value: &str) -> String {
    value
        .trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn column(headers: &[String], names: &[&str]) -> Option<usize> {
    headers.iter().position(|h| names.contains(&h.as_str()))
}

fn cell(row: &csv::StringRecord, col: Option<usize>, encoded: bool) -> String {
    let text = col.and_then(|index| row.get(index)).unwrap_or("");
    if encoded {
        text.strip_prefix('\'').unwrap_or(text).to_string()
    } else {
        text.trim().to_string()
    }
}

fn csv_text(text: &str) -> String {
    // Versioned exports reverse this prefix on import, including original apostrophes.
    let significant = text.trim_start_matches(char::is_whitespace);
    if text.starts_with(['\'', '\t', '\r', '\n']) || significant.starts_with(['=', '+', '-', '@']) {
        format!("'{text}")
    } else {
        text.to_string()
    }
}

fn parse_csv(bytes: &[u8], source: &str) -> Result<Vec<Reading>> {
    let text = decode_csv_text(bytes)?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let rows = reader
        .records()
        .take(MAX_READINGS + 20)
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("CSV contains invalid text or quoting")?;
    ensure!(
        rows.len() <= MAX_READINGS + 10,
        "CSV contains too many rows"
    );
    let header_index = rows
        .iter()
        .take(10)
        .position(|row| {
            let headers: Vec<_> = row.iter().map(normalized_header).collect();
            column(&headers, &["value", "values", "reading", "measurement"]).is_some()
        })
        .context(
            "CSV needs a Value or Values column (Evergreen CSV and long-form CSV are supported)",
        )?;
    let headers: Vec<_> = rows[header_index].iter().map(normalized_header).collect();
    let version_column = column(&headers, &["greensuitecsvversion"]);
    if version_column.is_some() {
        ensure!(
            [
                "id",
                "timestamp",
                "group",
                "sensor",
                "kind",
                "value",
                "unit",
                "source",
                "raw"
            ]
            .iter()
            .all(|required| headers.iter().any(|header| header.as_str() == *required)),
            "Green Suite CSV is missing a required column"
        );
    }
    let values: Vec<usize> = headers
        .iter()
        .enumerate()
        .filter(|(_, h)| ["value", "values", "reading", "measurement"].contains(&h.as_str()))
        .map(|(i, _)| i)
        .collect();
    let sensor_header = header_index.checked_sub(1).map(|i| &rows[i]);
    let default_group = Path::new(source)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let mut result = Vec::new();
    for (row_index, row) in rows.iter().enumerate().skip(header_index + 1) {
        if row.iter().all(|v| v.trim().is_empty()) {
            continue;
        }
        let encoded = version_column.is_some();
        if encoded {
            ensure!(
                version_column.and_then(|index| row.get(index)) == Some("1"),
                "Unsupported Green Suite CSV version on row {}",
                row_index + 1
            );
        }
        for &value_col in &values {
            let text = row.get(value_col).unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }
            let value: f64 = text.replace(',', "").parse().with_context(|| {
                format!("CSV row {} has a nonnumeric reading: {text}", row_index + 1)
            })?;
            ensure!(
                value.is_finite(),
                "CSV row {} contains a non-finite reading",
                row_index + 1
            );
            let mut timestamp = cell(row, column(&headers, &["timestamp", "datetime"]), encoded);
            if timestamp.is_empty() && !encoded {
                timestamp = [
                    cell(row, column(&headers, &["date"]), encoded),
                    cell(row, column(&headers, &["time"]), encoded),
                ]
                .into_iter()
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            }
            let group = cell(row, column(&headers, &["group", "groupname"]), encoded);
            let sensor = cell(
                row,
                column(&headers, &["sensor", "sensorid", "module"]),
                encoded,
            );
            let unit_col = if headers
                .get(value_col + 1)
                .is_some_and(|v| v == "unit" || v == "units")
            {
                Some(value_col + 1)
            } else {
                column(&headers, &["unit", "units"])
            };
            let unit = cell(row, unit_col, encoded);
            let kind = cell(
                row,
                column(&headers, &["kind", "type", "measurementtype"]),
                encoded,
            );
            let id = cell(row, column(&headers, &["id"]), encoded);
            let original_source = cell(row, column(&headers, &["source"]), encoded);
            let original_raw = cell(row, column(&headers, &["raw"]), encoded);
            let raw = if original_raw.is_empty() && !encoded {
                serde_json::to_string(&(value_col, row.iter().collect::<Vec<_>>()))?
            } else {
                original_raw
            };
            result.push(Reading {
                id: if id.is_empty() || values.len() > 1 {
                    Uuid::new_v4().to_string()
                } else {
                    id
                },
                timestamp,
                group: if group.is_empty() && !encoded {
                    default_group.to_string()
                } else {
                    group
                },
                sensor: if sensor.is_empty() && !encoded {
                    sensor_header
                        .and_then(|r| r.get(value_col))
                        .unwrap_or("")
                        .trim()
                        .to_string()
                } else {
                    sensor
                },
                kind: if kind.is_empty() && !encoded {
                    infer_kind(&unit).to_string()
                } else {
                    kind
                },
                value,
                unit,
                source: if original_source.is_empty() && !encoded {
                    source.to_string()
                } else {
                    original_source
                },
                raw,
            });
            ensure!(
                result.len() <= MAX_READINGS,
                "CSV contains too many measurements"
            );
        }
    }
    if result.is_empty() {
        bail!("CSV contains no numeric readings");
    }
    Ok(result)
}

fn decode_csv_text(bytes: &[u8]) -> Result<String> {
    let text = if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        ensure!(bytes.len() % 2 == 0, "CSV contains incomplete UTF-16 text");
        let little_endian = bytes[0] == 0xff;
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|b| {
                if little_endian {
                    u16::from_le_bytes([b[0], b[1]])
                } else {
                    u16::from_be_bytes([b[0], b[1]])
                }
            })
            .collect::<Vec<_>>();
        String::from_utf16(&units).context("CSV contains invalid UTF-16 text")?
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        text.to_owned()
    } else {
        // Legacy Windows uploader exports may use Windows-1252, notably °F.
        const C1: [char; 32] = [
            '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
            '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ',
            '\u{9d}', 'ž', 'Ÿ',
        ];
        bytes
            .iter()
            .map(|&byte| {
                if (0x80..=0x9f).contains(&byte) {
                    C1[(byte - 0x80) as usize]
                } else {
                    char::from(byte)
                }
            })
            .collect()
    };
    ensure!(!text.contains('\0'), "CSV contains binary data");
    Ok(text)
}

fn infer_kind(unit: &str) -> &'static str {
    let unit = unit.to_ascii_lowercase();
    if unit.contains("cfm") || unit.starts_with("cf:") || unit == "l/s" || unit == "m3/h" {
        "Airflow"
    } else if unit.contains("fpm") || unit == "m/s" {
        "Velocity"
    } else if unit.contains("gpm") {
        "Water flow"
    } else if unit.contains("pa") || unit.contains("wc") || unit.contains("psi") {
        "Pressure"
    } else if unit.contains('°') || unit == "degf" || unit == "degc" {
        "Temperature"
    } else {
        "Measurement"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!("green-suite-model-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn workspace_follows_repository_or_standalone_executable() {
        let folder = scratch();
        let repo = folder.join("repository");
        let executable_dir = repo.join("target").join("debug");
        fs::create_dir_all(&executable_dir).unwrap();
        fs::write(repo.join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        assert_eq!(
            workspace_for_executable_dir(&executable_dir),
            repo.join("workspace")
        );
        assert_eq!(workspace_for_executable_dir(&repo), repo.join("workspace"));
        for relative in [
            "target/release",
            "target/debug/deps",
            "target/x86_64-pc-windows-msvc/release/deps",
        ] {
            assert_eq!(
                workspace_for_executable_dir(&repo.join(relative)),
                repo.join("workspace"),
                "{relative}"
            );
        }
        let standalone = folder.join("standalone");
        fs::create_dir_all(&standalone).unwrap();
        assert_eq!(
            workspace_for_executable_dir(&standalone),
            standalone.join("workspace")
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn unrelated_cargo_ancestor_does_not_redirect_standalone_data() {
        let folder = scratch();
        fs::write(folder.join("Cargo.toml"), "[package]\nname='unrelated'\n").unwrap();
        for relative in [
            "apps/green-suite/bin",
            "arbitrary/debug",
            "target/not-a-profile",
            "target/random/debug",
        ] {
            let executable_dir = folder.join(relative);
            assert_eq!(
                workspace_for_executable_dir(&executable_dir),
                executable_dir.join("workspace"),
                "{relative}"
            );
        }
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn project_id_cannot_redirect_workspace_writes() {
        let mut p = Project::new("Test");
        for invalid in ["C:outside", "..", "../outside", "local-name"] {
            p.id = invalid.into();
            assert!(p.validate().is_err(), "accepted {invalid}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn loaded_blueprints_cannot_trigger_remote_or_device_access() {
        let folder = scratch();
        let path = folder.join("project.json");
        let mut p = Project::new("Test");
        p.documents.push(Document {
            id: "d1".into(),
            page_count: 1,
            ..Default::default()
        });
        for remote in [
            r"\\server\share\drawing.pdf",
            r"\\?\UNC\server\share\drawing.pdf",
            r"\\.\COM5",
            r"\\?\GLOBALROOT\Device\HarddiskVolumeShadowCopy1\drawing.pdf",
        ] {
            p.documents[0].path = remote.into();
            fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
            assert!(Project::load(&path).is_err(), "accepted {remote}");
        }
        p.documents[0].path = "blueprints/drawing.pdf".into();
        fs::write(&path, serde_json::to_vec(&p).unwrap()).unwrap();
        assert!(Path::new(&Project::load(&path).unwrap().documents[0].path).is_absolute());
        assert!(ensure_local_document_path(Path::new(r"\\?\D:\drawing.pdf")).is_ok());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn evergreen_multiple_sensors_and_units_are_preserved() {
        let rows = parse_csv(b",,,S-0,S-0,S-1,S-1\nNo.,Date,Time,Values,Units,Values,Units\n1,03/15/2024,08:38:50,251,CF:c,72.1,degF\n", "AA.csv").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            (
                &*rows[0].sensor,
                &*rows[0].group,
                rows[0].value,
                &*rows[0].unit
            ),
            ("S-0", "AA", 251.0, "CF:c")
        );
        assert_eq!(
            (&*rows[1].sensor, rows[1].value, &*rows[1].kind),
            ("S-1", 72.1, "Temperature")
        );
    }

    #[test]
    fn legacy_windows_temperature_unit_and_utf16_are_preserved() {
        let rows = parse_csv(b"value,unit\n72,\xb0F\n", "temperature.csv").unwrap();
        assert_eq!(rows[0].unit, "°F");
        let mut utf16 = vec![0xff, 0xfe];
        for unit in "value,unit\n72,°F\n".encode_utf16() {
            utf16.extend(unit.to_le_bytes());
        }
        assert_eq!(parse_csv(&utf16, "temperature.csv").unwrap()[0].unit, "°F");
    }

    #[test]
    fn invalid_csv_cannot_partially_import() {
        let path = scratch();
        let csv = path.join("invalid.csv");
        fs::write(&csv, "timestamp,value,unit\na,100,cfm\nb,NaN,cfm\n").unwrap();
        let mut p = Project::new("Test");
        assert!(p.import_csv(&csv).is_err());
        assert!(p.readings.is_empty());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn repeated_import_preserves_ids_and_associations() {
        let csv = b"timestamp,value,unit\na,100,cfm\n";
        let mut p = Project::new("Test");
        assert_eq!(
            p.merge_readings(parse_csv(csv, "test.csv").unwrap())
                .unwrap(),
            1
        );
        let id = p.readings[0].id.clone();
        assert_eq!(
            p.merge_readings(parse_csv(csv, "test.csv").unwrap())
                .unwrap(),
            0
        );
        assert_eq!(p.readings[0].id, id);
    }

    #[test]
    fn durable_save_roundtrip_and_replacement() {
        let folder = scratch();
        let path = folder.join("project.green-suite.json");
        let mut p = Project::new("First");
        p.save(&path).unwrap();
        p.name = "Second".into();
        p.save(&path).unwrap();
        assert_eq!(Project::load(&path).unwrap().name, "Second");
        p.readings.push(Reading {
            id: "r1".into(),
            value: f64::INFINITY,
            ..Default::default()
        });
        assert!(p.save(&path).is_err());
        assert_eq!(Project::load(&path).unwrap().name, "Second");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn csv_export_can_be_reimported_without_duplicate_measurements() {
        let folder = scratch();
        let path = folder.join("export.csv");
        let mut project = Project::new("Test");
        project
            .merge_readings(
                parse_csv(
                    b"timestamp,value,unit\n2026-10-14,123.5,cfm\n",
                    "initial.csv",
                )
                .unwrap(),
            )
            .unwrap();
        project.export_csv(&path).unwrap();
        assert_eq!(project.import_csv(&path).unwrap(), 0);
        assert_eq!(project.readings.len(), 1);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn spreadsheet_safe_csv_roundtrip_preserves_text_and_empty_fields() {
        let folder = scratch();
        let path = folder.join("escaped.csv");
        let mut original = Project::new("Source");
        original.readings.push(Reading {
            id: "r1".into(),
            value: 42.0,
            sensor: "  =HYPERLINK(\"example\")".into(),
            kind: "'Literal apostrophe".into(),
            unit: " unit with spaces ".into(),
            source: "+sensor".into(),
            raw: "\tleading tab".into(),
            ..Default::default()
        });
        original.readings.push(Reading {
            id: "r2".into(),
            value: 43.0,
            ..Default::default()
        });
        original.export_csv(&path).unwrap();
        let exported = fs::read_to_string(&path).unwrap();
        assert!(exported.contains("green_suite_csv_version"));
        assert!(exported.contains("'  =HYPERLINK"));
        assert!(exported.contains("''Literal apostrophe"));
        let mut imported = Project::new("Destination");
        assert_eq!(imported.import_csv(&path).unwrap(), 2);
        assert_eq!(
            serde_json::to_value(&imported.readings).unwrap(),
            serde_json::to_value(&original.readings).unwrap()
        );
        assert_eq!(original.import_csv(&path).unwrap(), 0);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn unknown_csv_escape_version_is_rejected() {
        assert!(parse_csv(b"id,timestamp,group,sensor,kind,value,unit,source,raw,green_suite_csv_version\nr1,,,,,42,,,,2\n", "future.csv").is_err());
    }

    #[test]
    fn device_identity_deduplicates_despite_transfer_status_byte() {
        let mut p = Project::new("Test");
        let mut reading = Reading {
            id: "wr:123:1:2:S0:0:0".into(),
            value: 42.0,
            source: "USB 123".into(),
            raw: "00ab".into(),
            ..Default::default()
        };
        assert_eq!(p.merge_readings(vec![reading.clone()]).unwrap(), 1);
        reading.raw = "01ab".into();
        assert_eq!(p.merge_readings(vec![reading]).unwrap(), 0);
    }

    #[test]
    fn illustrative_sheets_and_point_associations_survive_save() {
        let folder = scratch();
        let path = folder.join("project.json");
        let mut p = Project::new("Example");
        p.documents.push(Document {
            id: "example".into(),
            page_count: 1,
            ..Default::default()
        });
        p.readings.push(Reading {
            id: "r1".into(),
            value: 250.0,
            ..Default::default()
        });
        p.points.push(MapPoint {
            id: "p1".into(),
            document_id: "example".into(),
            reading_ids: vec!["r1".into()],
            ..Default::default()
        });
        p.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        assert!(loaded.documents[0].path.is_empty());
        assert_eq!(loaded.points[0].reading_ids, ["r1"]);
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn traverse_validation_matches_supported_geometry() {
        let mut p = Project::new("Test");
        p.traverses.push(Traverse::default());
        assert!(p.validate().is_ok());
        p.traverses[0].shape = "CIRCULAR".into();
        assert!(p.validate().is_ok());
        p.traverses[0].shape = "triangle".into();
        assert!(p.validate().is_err());
        p.traverses[0].shape = "rectangular".into();
        p.traverses[0].rows = 21;
        p.traverses[0].columns = 20;
        p.traverses[0].velocities = vec![None; 420];
        assert!(p.validate().is_err());
        p.traverses[0].rows = 20;
        p.traverses[0].velocities = vec![None; 399];
        assert!(p.validate().is_err());
        p.traverses[0].velocities = vec![None; 400];
        assert!(p.validate().is_ok());
    }

    #[test]
    fn point_bounds_and_dangling_links_are_rejected() {
        let mut p = Project::new("Test");
        p.documents.push(Document {
            id: "d1".into(),
            path: "blueprint.pdf".into(),
            page_count: 1,
            ..Default::default()
        });
        p.points.push(MapPoint {
            id: "p1".into(),
            document_id: "d1".into(),
            x: 1.1,
            ..Default::default()
        });
        assert!(p.validate().is_err());
        p.points[0].x = 0.5;
        p.points[0].reading_ids.push("missing".into());
        assert!(p.validate().is_err());
        p.points[0].reading_ids.clear();
        assert!(p.validate().is_ok());
    }
}
