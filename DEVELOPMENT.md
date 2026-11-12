# Stored-reading development plan

This document defines the development scope. Completed foundations and remaining
work are distinguished below. Current operation and verified compatibility are
documented in [README.md](README.md) and [COMPATIBILITY.md](COMPATIBILITY.md).

## Progress

- Implemented: bounded offline stored-USB capture replay using the download
  frame/record decoder, redacted summary reports, and a compatibility matrix.
- Implemented: application port ownership, same-session Reporter identity checks
  before GUI stored downloads, and explicit selection after a port disappears.
- Implemented: optional measurement provenance, source-record review, calibration
  notes, dimension-checked conversion previews and separate calculated readings.
  Native JSON/CSV round trips preserve metadata and original numeric text.
- Implemented: reviewed field-factor basis, separate physical unit definitions,
  and neutral calculation-rule/version tracking for converted results. Additional
  calculation methods remain pending validation.
- Implemented: stored-reading comparisons with explicit time/clock handling,
  stability summaries and observed-range plots, optional hold-weighted means,
  and selected-device balancing with design comparisons and optional tolerances.
  Saved results preserve input snapshots; stale previews are rejected.
- Implemented: guided traverse assignments retain unique saved-record snapshots;
  manual edits clear their links, geometry changes clear samples, and capture
  rejects missing or changed sources. Saved flow retains layout and input context.
- Implemented: hydronic sensible heat and Cv/Kv liquid valve-flow estimates with
  explicit fluid properties, operating temperature, coefficient/setting context,
  applicability confirmation and saved input provenance. Thermal calculations
  enforce all-input time pairing or explicit separate-snapshot interpretation.
- Implemented: stored dry-bulb/RH air states, humidity-ratio diagrams, humidity
  ratio, enthalpy, dew/frost point and signed moisture-removal estimates. Absolute
  barometric pressure, timing, subzero RH conventions and dry-air mass-flow
  interpretation are explicit; out-of-range condensation points are unavailable.
- Implemented: passive USB bridge/location inventory with bounded transition
  history, explicit reconnect identity checks, stale-worker identity guards and
  deliberate redacted diagnostic export. Simulations cover disconnects, changed
  COM numbers, shared bridge serials and retained readings.
- Remaining: broader protocol fixtures, additional hardware recovery/checksum
  evidence, and separately scoped destination transfers.
- Excluded: Android USB/mobile work, wireless phone links, direct sensor pairing,
  and expansion of live sensor support.

## Scope and connection model

Sensors remain linked to the Wrist Reporter. Green Suite obtains measurements
from the Reporter's stored records through USB; it does not pair with sensors.
Do not add sensor discovery, direct sensor connections, or additional live
sensor-family support as part of this plan. Existing live reception remains a
separate, limited capability; this plan does not expand its compatibility.

Wireless phone connectivity, radio adapters, and USB-to-wireless bridges are
deferred. Android USB and mobile application work are excluded. Reviewed desktop
exports to AirOne and BuildingStart remain separate integration work.
Original application code remains Rust and Perl.

Field-calculation rules require verified unit conventions and applicability
conditions. Code, the interface and saved results use neutral rule names and
versions. Preserve the selected calculation basis; do not
substitute generic conversion precision for a selected field-factor profile.
See [calculation rules](CALCULATIONS.md).

## 1. Connection management and diagnostics

- Track USB identity using available VID/PID, bridge serial, and port location.
  Treat these as connection hints: bridge identifiers can be missing or shared.
  Confirm Reporter identity using read-only metadata before a download. Do not
  silently select another instrument when a COM number changes.
- Give each port one application owner across identification, download, and
  existing live sessions. Make cancellation, disconnection, and shutdown states
  explicit. Do not restart a stateful record cursor automatically.
- Detect disappearance and reappearance; present a reconnect action with the
  previous Reporter identity and current candidate ports. Re-identify before
  continuing. Keep received data and clearly mark incomplete downloads.
- Show a concise diagnostic summary distinguishing enumeration, driver binding,
  exclusive access, serial setup, protocol timeout, malformed response, and
  checksum failure. Offer a private diagnostic export with deliberate redaction
  of device and project identifiers.
- Continue using the signed CP210x driver. Driver installation, device restarts,
  and power-policy changes remain explicit maintenance operations, not automatic
  download recovery. Developing a replacement kernel driver is not a prerequisite.

Acceptance: simulated disconnects, changed COM numbers, ambiguous USB identities,
and competing sessions cannot redirect a download or lose retained records.

## 2. Protocol validation and replay

- Add an offline replay harness for raw captures. Separate byte transport from
  framing, transaction handling, and measurement decoding so the same parser can
  be exercised without an attached Reporter.
- Cover fragmentation, escaped bytes, late responses, truncation, corruption,
  repeated records, mixed sensor channels, and incomplete transfers. Public
  fixtures must be synthetic; real captures remain private.
- Maintain a compatibility matrix distinguishing stored download, interpretation,
  checksum validation, and export comparison by Reporter firmware and module.
  Unit recognition alone must not establish module compatibility.
- Investigate undocumented stored-record checksum variants using authorized
  captures and matching original exports. Preserve unknown fields and original
  bytes. Never label an unverified checksum as verified or invent missing values.
- Resume interrupted work only through a demonstrated safe cursor operation or
  a new full download with stable-identity duplicate detection. Do not promise
  byte-position resume without protocol evidence.

Acceptance: deterministic replay results, explicit rejection reasons, preserved
partial results, and comparison of values, units, timestamps, and channel identity
against independently exported records. Hardware-dependent conclusions require
documented hardware validation.

## 3. Measurement records and provenance

- Introduce backward-compatible optional metadata for Reporter identity,
  firmware, download session, original record/group identifier, sensor address,
  channel, original numeric text, original unit, and parser version.
- Distinguish original instrument values, instrument-calculated values,
  application-calculated values, and manual entries. If their origin cannot be
  established, label it unknown. Preserve raw data separately from interpretation.
- Identify physical quantity independently of unit text. For example, pressure
  units alone cannot identify room pressure, water differential pressure, or
  barometric pressure; temperature alone cannot establish supply/return roles.
- Add dimension-checked conversions that preserve the original reading. Handle
  absolute temperatures and temperature differences separately. Require explicit
  interpretation of undocumented unit labels before conversion.
- Keep instrument-local timestamps, PC receipt times, and any user-declared
  clock correction separately. Do not infer a timezone or treat nearest readings
  as simultaneous. Pairing must expose time separation and a chosen tolerance.
- Associate calibration references, dates, units, ranges, and uncertainty with
  the relevant instrument/channel. Unknown calibration status stays unknown;
  the software must not claim certification or adjust instrument calibration.
- Store derived-result inputs, selected records, assumptions, formula version,
  and mapping to drawing devices. Repeated imports must not erase provenance.

Acceptance: legacy projects still load; metadata survives save/load and reviewed
exports; incompatible units and unsupported pairing cannot produce a result.

## 4. Stored-data analysis workflows

Keep the current simple navigation. Add contextual analysis actions from selected
readings or drawing devices, with secondary parameters behind details controls.
Use stored records, explicit role assignment, and clearly labeled derived results.

| Workflow | Planned capabilities | Required inputs and safeguards |
| --- | --- | --- |
| Pressure | Filter/coil pressure-drop comparisons; room-pressure trends; stability statistics; before/after comparisons | Assigned locations, pressure reference/sign, compatible units, valid time ordering; do not assume sequential readings were simultaneous |
| Air velocity and flow | Guided assignment of stored samples to traverse positions; terminal balancing; design-versus-measured summaries; system totals | Correct probe/measurement interpretation, duct area or documented hood correction, unique device assignments, completeness checks; preserve existing traverse geometry validation |
| Temperature | Supply/return and entering/leaving temperature differences; paired-probe comparisons; coil trends | Explicit probe roles, compatible temperature scales, time-pairing tolerance; distinguish a temperature difference from an absolute temperature |
| Temperature and humidity | Psychrometric plots; dew point; humidity ratio; enthalpy; moisture-removal analysis | Compatible temperature/RH pairs and declared barometric pressure or explicitly labeled assumption; validated equations/ranges; additional dry-air mass-flow information for moisture-removal rates |
| Hydronic | Valve balancing; pressure/flow comparison; hydronic heat-transfer calculations | Explicit fluid properties and operating temperature; flow and temperature difference for heat transfer; documented valve coefficient/curve and fluid density for pressure-derived flow |

For each workflow:

- Preview selected inputs, exclusions, missing data, and time separation before
  saving a result. Preserve signed values and zero as real measurements.
- Provide descriptive sample count, minimum, maximum, mean, and variability
  where meaningful. Use time-weighted methods only when the sampling assumptions
  are explicit. Gaps and out-of-order timestamps must be visible.
- Separate measured fields from calculated values visually and in exports.
  Never replace stored source readings with calculated or corrected values.
- Retain original design values and allow explicit acceptance tolerances.
  A calculated percentage is not an automatic assertion of TAB compliance.
- Save result provenance and attachments to the relevant drawing point or system.
  Reports should identify incomplete inputs and unverified interpretation.

Acceptance: independently checked calculation cases with documented units,
boundary/invalid-input checks, reproducible saved results, and accessible GUI
review at the supported minimum window size in both appearance modes.

## 5. Desktop destination imports

AirOne and BuildingStart transfers require explicit field/unit mappings,
authentication, duplicate detection, a preview of destination changes, and
receipts for completed or uncertain transfers. No automatic project upload is
authorized by the existence of this roadmap. These integrations remain deferred
until the destination interfaces and acceptance scope are established.

## Implementation order

1. Establish offline replay and a compatibility matrix; strengthen connection
   ownership, instrument identity checks, and reconnect diagnostics.
2. Add backward-compatible provenance and dimension-checked conversions.
3. Implement stored pressure/temperature comparisons and balancing summaries,
   extending guided use of the existing traverse tool.
4. Add validated psychrometric and hydronic calculations with input provenance.
5. Develop reviewed desktop destination transfers separately.

Every phase requires focused automated checks and headless UI review where
applicable. Hardware-dependent compatibility claims require additional physical
validation; simulations alone cannot establish them.
