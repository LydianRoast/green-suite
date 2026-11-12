# Measurement records and conversions

Each source value, unit label, timestamp and raw record remains available.
Optional `metadata` adds Reporter/firmware identity, download session, original
group and record position, sensor/channel, original numeric text, parser version,
checksum status, time basis, quantity, role, calibration notes and derivation.
Older projects without metadata still load; unavailable provenance stays unknown.

Stored downloads populate source identity and exact numeric text. Instrument
receipt time is recorded separately in UTC; the recorded instrument timestamp
remains local without an inferred timezone. Downloading an instrument-calculated
field does not make it a direct measurement. Origin stays unknown unless its
classification is established. Application calculations and manual entries are
explicitly identified.

## Review and conversions

In Readings, open a source record and expand **Measurement details** to review
provenance. **Role and calibration notes** records the measurement location/role
and optional calibration reference, dates, range, uncertainty and coverage factor.
Absent values stay unknown; a user-supplied reference does not imply certificate
verification or modify instrument calibration.

Expand **Convert units** for a preview. Saving creates a separate calculated
reading with an input snapshot, formula version, original record ID and conversion
assumptions. It does not replace the source reading or silently attach a second
value to a drawing point. Explicitly map it when appropriate.

The default calculation basis is the reviewed rounded-field-factor profile.
The UI also offers a separately identified physical-unit-definition basis.
The selected basis and neutral rule version are saved with the calculation. See
[calculation rules](CALCULATIONS.md) for coverage and limitations.

Conversions require matching physical quantities. Absolute temperatures and
temperature differences are separate dimensions. Temperature offsets are never
applied to a temperature difference. Below-absolute-zero temperatures, non-finite
values and overflow are rejected. Relative humidity must lie between 0 and 100%.
The source numeric precision is preserved, but extra calculated digits do not
improve the measurement's accuracy.

## Unit conventions

- `inW` and `inWC` mean inches of water column. The field conversion factor is
  **249.082 Pa per inch**. A separately named 60°F unit
  uses **248.84 Pa per inch**. The chosen convention is an application conversion
  definition, not a claim about undocumented sensor compensation.
- `GPM` and `gpm` mean gallons per minute. The UI names the application's US
  convention `US gpm`. The field basis uses a rounded gallon/litre factor;
  the physical-definition basis uses the US liquid gallon (0.003785411784 m³).
- Velocity supports FPM and m/s. Volume flow supports CFM, L/s, m³/s, m³/h and
  US gpm. Volume conversion does not perform air-density or standard-condition
  correction and does not identify a reading as air or water by itself.
- Supported pressure units include Pa, kPa, psi and explicitly referenced water
  and mercury columns. An unqualified `inHg` label needs explicit interpretation.
- `CF:c`, `DP`, and other unrecognized labels require an explicit definition
  before conversion. No hidden conversion factor is inferred from their spelling.

Each basis uses reviewed, versioned definitions described in
[calculation rules](CALCULATIONS.md). Factors retain the precision of the selected
rule. Neither basis implies improved instrument accuracy.

## Storage and export

Native project JSON retains metadata. Native CSV exports include an additive
`metadata_json` column using the existing version-1 text escaping. Reimporting
through the native application preserves it. Existing CSV readers may ignore
the extra column; older exports without it remain readable.

Repeated imports preserve existing provenance and drawing associations. A
matching legacy record without metadata can be enriched; an incoming record
cannot replace already stored metadata. Invalid imports are rejected before
applying enrichment. The optional Perl utility remains a legacy flat-field
export; use native CSV or project JSON when full metadata is required.

Clock-correction annotations are stored separately from source timestamps. Time
pairing, uncertainty propagation, psychrometric calculations and hydronic
calculations remain later development work; unit conversion alone does not
establish those capabilities.
