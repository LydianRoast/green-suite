# Stored-reading analysis

Open **Readings → Analysis**. All inputs are existing project records. No sensor
connection, instrument command, service access or upload is performed by analysis.
Choose the calculation basis explicitly and use **Preview calculation** before
**Save calculated reading**. The result is a separate, clearly calculated record.

## Comparisons

Choose input A, input B, their roles and an output unit. The result is A minus B.
Pressure, temperature, velocity and volume-flow comparisons require compatible
quantities. A temperature difference is saved in Δ°C, Δ°F or ΔK rather than an
absolute-temperature unit.

Paired comparisons require complete date/time fields and a chosen maximum time
separation. Clock comparability is established from explicit timezone information
or the same identified Reporter's local clock; otherwise user confirmation is
required. A timezone is never inferred from a naive instrument timestamp.

**Before/after snapshots** does not imply a simultaneous measurement. Unknown
time separation requires its own explicit acceptance, distinct from confirmation
that clocks are comparable. The selected roles, sign, clock decisions, tolerance
and known or unknown separation are retained with the result.

Source-record **Role and calibration notes** can hold a declared clock correction
in seconds. Analysis adds that correction without changing the source timestamp
or instrument clock. Unit conversions preserve the source clock correction.
Entry, receipt-only and calculation-generation timestamps are not treated as
measurement times; explicitly accepted snapshot comparisons remain available.

## Trends and stability

Choose a series by group, sensor, unit and available Reporter identity. Statistics
use stored order and include measured zeros and signed values. The default gives
each selected reading equal weight and reports count, minimum, maximum, mean,
sample standard deviation, unavailable measurement times, backward steps,
repeated timestamps and the largest established sampling gap. One sample has
no sample standard deviation. Temperature dispersion uses difference units.

The observed-range plot uses at most 600 buckets and retains the observed minimum
and maximum within each bucket. Statistics use all selected records, up to
100,000. Comparable monotonic timestamps supply the elapsed-time axis; otherwise
the plot uses stored record order and labels the time axis unverified. Blank
intervals are not interpolated.

The optional time-weighted mean requires complete, comparable, strictly increasing
timestamps and an explicit maximum hold interval. It assumes that each value is
held until the next sample. The final sample has no duration and contributes no
held interval. A missing time, duplicate time, backward step or excessive gap
withholds that calculation rather than inventing samples.

## Balancing summaries

Choose a system, service and output flow unit. A label filter can define a named
subset. Include up to 400 devices and explicitly select one mapped reading per
included device. No latest-reading selection or simultaneous system snapshot is
assumed. A single reading cannot be counted against multiple devices.

Each row shows design flow, selected flow and percent of design. Positive design
values and supported, compatible units are required. An optional project
acceptance tolerance controls the within/outside labels; there is no automatic
assertion of standards compliance. Original design values remain unchanged.

Missing or unsupported inputs are shown per device. Known subtotals are labeled
incomplete, and a complete total cannot be saved until all included rows are
resolved. A saved total identifies the selected devices, design snapshots,
measurement inputs, service, units, calculation basis and configured tolerance.
Reading selectors identify calculated, manual or unknown origins explicitly.

## Saving and provenance

Input snapshots retain record IDs, original values and units, timestamps and
interpretation context, including applicable clock and calibration annotations.
Neutral rule/version identifiers describe the calculation. Native project JSON
and native CSV retain this metadata. Calculated result timestamps identify the
time of calculation in UTC, separately from input measurement times.

Changing form parameters invalidates the preview. Changing input records,
annotations, device designs or mappings requires a new preview before saving.
The optional attachment selector permits compatible drawing-point quantities;
the default saves to the project only. Large input lists are virtualized in
source-record review, and long raw displays are bounded while stored data is
preserved.

Display formatting removes floating-point presentation noise; saved numeric
values retain their full representation. Derived statistics and configured
tolerances do not establish instrument accuracy or propagate calibration
uncertainty.

## Air state and moisture removal

Choose Air state in the Analysis window. Assign a dry-bulb temperature and RH
record to state A, then select humidity ratio, enthalpy, or dew/frost point.
Supply absolute barometric pressure through a selected record or a declared
value in Pa. Mark a declared assumption explicitly; no standard-atmosphere
pressure is inserted automatically. Gauge/differential pressure is unsuitable.
A selected pressure record participates in the time-span checks. A declared
pressure is treated as constant, without an invented observation timestamp.

The ideal moist-air model supports dry bulb from −100 to 200 °C and absolute
pressure from 20 to 120 kPa, with water-vapour partial pressure below total
pressure. At/below 0.01 °C, input RH must be explicitly confirmed as relative to
ice equilibrium. Supercooled-water RH interpretations are unsupported there.
The condensation calculation uses ice below that boundary (frost point) and
water above (dew point). Zero RH remains zero, with no finite condensation point.
An inferred condensation point below −100 °C is unavailable rather than clipped
to the model boundary; humidity ratio and enthalpy remain calculable.

The diagram shows dry bulb versus humidity ratio in g/kg dry air, with RH
contours and selected A/B state markers. Curves break where a state would require
vapour pressure at or above total pressure. Hovering reports chart coordinates;
the chart does not invent intermediate measured observations. It is an air-state
diagram, without wet-bulb or enthalpy contour overlays.

Moisture-removal rate requires distinct entering (A) and leaving (B) temperature/RH
records and explicitly confirmed dry-air mass flow. The equation is
`kg_water/s = kg_dry_air/s × (W_entering − W_leaving)`.
Positive indicates water removed from air; negative indicates water added.
Zero flow is valid. This estimate does not prove condensate collection, account
for leakage/mixing or integrate a total amount of water over time. A volume-flow
or moist-air mass-flow reading cannot silently substitute for dry-air mass flow.

All selected records must meet the configured time span and comparable-clock
conditions unless separate snapshots are accepted. Entry/receipt/calculation
times cannot establish measurement pairing. Saved results preserve each input,
pressure provenance/assumption, roles, model limits, enthalpy reference and units.
Humidity ratio and enthalpy use a dry-air mass basis. The enthalpy approximation
uses a 0 °C dry-air sensible reference and a 2,501,000 J/kg vapour offset.
Enhancement factors, uncertainty propagation, wet-bulb inversion and nonideal
high-pressure gas calculations are unsupported.

## Hydronic estimates

Choose Hydronic in the Analysis window, then Heat transfer or Valve flow.
No fluid properties are filled automatically. Declare the fluid/mixture,
operating temperature and density, and confirm that the properties and
single-phase liquid assumption suit the operating conditions.

Heat transfer uses selected liquid flow, entering temperature and leaving
temperature, plus declared specific heat in J/(kg·K). It calculates
`W = density × volume flow × specific heat × (entering − leaving)` using SI
quantities. Positive means heat lost by the fluid; negative means gained.
Zero flow and zero temperature difference are valid. Negative flow requires
review of the actual direction and temperature roles. The preview shows the
input mean temperature; properties must be appropriate to the temperature range.

All three measurement times must be comparable and fall within the configured
span, unless separate snapshots are explicitly accepted. A snapshot result is
an estimate of a declared operating point, rather than an instantaneous measured
rate. Entry, receipt and calculation timestamps cannot establish time pairing.
Stored derived flow can be used with explicit snapshot interpretation.

Valve flow uses one record explicitly assigned as differential pressure across
the valve. Supply the valve identity, size, actual setting and corresponding
coefficient; a fully-open coefficient must not be assumed for another setting.
Cv uses US gpm at 1 psi with a reference density of 999.016 kg/m³; Kv uses m³/h
at 1 bar with a reference density of 1000 kg/m³. The estimate scales with the
square root of pressure drop divided by relative density. Negative pressure
drop requires review of port roles; it is never silently made positive.
Zero pressure drop returns zero estimated flow and does not verify shutoff.

Confirm turbulent, non-choked, non-cavitating liquid conditions and that no
piping correction is required. Gas/steam sizing, automatic valve-curve
interpolation, latent heat, energy integration and propagated measurement
uncertainty are unsupported. The application makes no physical valve changes.
Saved estimates retain selected input snapshots, fluid properties, timing
interpretation, coefficient/setting, units and neutral formula versions.

## Guided traverse capture

Use saved velocity readings in the Traverse view to assign a record to a position.
Assign + next advances to the next unfilled position. One record cannot fill
multiple positions; distinct records with equal values are permitted. Manual
table edits clear that position's saved-record link. Applying a new geometry or
clearing the samples removes the links as well.

Capture requires every position to have a valid velocity and every linked source
to remain present and unchanged. A changed source must be reassigned, or replaced
with an explicitly manual entry. Existing projects retain their manual/legacy
values. A saved calculated flow records the layout, velocities, manual/legacy
position count, conversion rule and linked input snapshots. Stored samples are
not assumed simultaneous, and the result timestamp is calculation time.
