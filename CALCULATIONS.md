# Calculation basis and rule versions

The default **Rounded field factors** basis uses reviewed field-calculation
values at their specified precision. **Physical unit definitions** is a separate
selection and may produce slightly different results. The application records
the selected basis and neutral rule version in each saved calculation.

Supported rule identifiers include:

- `field-conversions-v1`: pressure, velocity, volume-flow and absolute-temperature
  conversions using reviewed forward factors and algebraic inverses.
- `temperature-difference-v1`: temperature-difference scales without an
  absolute-temperature offset.
- `humidity-ratio-v1`: grain/pound and mass-ratio conversion.
- `unit-definitions-v1`: physical definitions for the supported units.
- `liquid-sensible-heat-v1`: declared constant density and specific heat times
  volume flow times entering-minus-leaving temperature, with watts as output.
- `liquid-valve-coefficient-v1`: declared Cv or Kv and differential pressure,
  corrected for declared liquid density; the coefficient must match the setting.
- `moist-air-ratio-v1`, `moist-air-enthalpy-v1`, `moist-air-condensation-v1`
  and `moist-air-moisture-removal-v1`: ideal moist-air state calculations from
  dry bulb, RH and explicit absolute pressure; signed moisture rate additionally
  requires dry-air mass flow and distinct entering/leaving records. Saturation
  uses versioned temperature polynomials with bounded pressure inversion for
  dew/frost point. Saved results retain the convention, range, pressure basis,
  state values, enthalpy datum and input timing interpretation.

Reverse and cross-unit field conversions use explicit algebraic combinations of
the reviewed forward factors. SI prefix and time scaling are applied separately.
For US gpm, the field basis derives its factor from the rounded gallon/litre
value. The physical-definition basis uses the US liquid gallon definition.

An unsupported pair reports that the selected basis does not provide it; there
is no automatic fallback. Original values and unit labels are retained. Calculated
values do not imply improved instrument accuracy or full TAB compliance.

Calculation rule IDs, formula versions, input snapshots and assumptions make
saved results reproducible.

Acceptance limits, traverse methods, density corrections and fluid properties
require their own validated rule and applicability conditions. A shared unit
name alone does not establish those conditions. Additional methods remain
disabled until their rules are reviewed and tested. Implemented hydronic rules
require positive, finite declared properties, explicit liquid applicability,
compatible units and finite results. They do not infer a water/glycol mixture or
automatically adjust coefficients for temperature or valve position.
