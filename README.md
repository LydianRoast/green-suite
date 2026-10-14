# Green Suite

A measurement workspace for mechanical test and balance work.

## Workspace

- **Blueprint map:** import a multipage mechanical PDF, switch sheets, pan and
  zoom, and place or reposition supply, return, exhaust, duct traverse, valve,
  and pipe markers. Each marker stores its equipment/system, design value,
  units, notes, and attached measurements.
- **Reading explorer:** connect a WR-401, inspect its identity and inventory,
  download stored records, filter measurements, inspect source records, and
  attach readings to drawing markers. Import Evergreen CSV exports or a
  long-form CSV; export normalized measurements as CSV.
- **Traverse lab:** rectangular midpoint grids and circular equal-area annular
  sectors; editable velocity samples; assignment of downloaded velocity
  measurements; animated flow, sample selection, and a color field. Pause the
  animation when desired. Dimensions are inches and velocity is feet/minute;
  m/s measurements are converted when assigned. Complete traverses can be saved
  as calculated CFM readings and attached to a mapped device.
- **Local projects:** autosave, explicit Save/Save as, and reopen. Clearly
  labeled example data is available separately from actual instrument data.

## Interface

The interface provides Dark and Light palettes, keyboard focus, scrollable
details, and direct access to drawings and readings. The header and traverse
diagram adapt to smaller windows. Measurement displays retain recorded numeric
precision, and incomplete USB downloads remain clearly identified.

## Run and build

Requires a Rust toolchain supporting edition 2024 and the pinned dependencies
in `Cargo.lock` (tested with Rust 1.96 on Windows 10 x64). On Windows the
MSVC C++ build tools and Windows SDK are needed to link the native application.

```text
cargo run --locked
cargo test --locked
cargo build --locked --release
```

The built Windows program is `target/release/green-suite.exe`. Connect the
Wrist Reporter with a **mini-USB data cable**. The Silicon Labs CP210x driver
must expose a serial port. Select the port, click **Connect**, then
**Download**. Downloads do not erase instrument memory or change firmware.

Close other software using that serial port before connecting. A port may
change when an instrument is connected through a different USB socket.

`cargo build --locked` produces `target/debug/green-suite.exe`. This profile
uses modest optimization, omits debug symbols, and disables incremental output.

## Data and project files

Development builds store projects, imported PDF copies, and USB packet captures
in the repository's `workspace` folder.
An executable beside `Cargo.toml` uses that same location; a standalone
executable uses a `workspace` folder beside itself. `GREEN_SUITE_DATA_DIR`
overrides this location; relative overrides
resolve against the launch directory. A project is a `.green-suite.json` file.
Imported PDFs are copied into the project's managed `blueprints` directory.

**Save as saves the project JSON, not a portable PDF bundle.** The JSON retains
paths to the copied PDFs. Preserve those PDF files when backing up or moving
a project. Measurements and map associations are contained in the JSON.
Private measurements, PDFs, captures, build output, and runtime workspace
files are excluded from Git.

CSV measurements have `id,timestamp,group,sensor,kind,value,unit,source,raw`
fields. Exports also include point labels and a `green_suite_csv_version`
marker. Formula-leading text receives an apostrophe prefix for spreadsheet
safety; Green Suite reverses its own versioned escaping on reimport, preserving
empty fields, whitespace, and original apostrophes. Dates decoded from WR-401
packets are instrument-local timestamps;
the instrument does not supply a timezone. Units reported by the device are
preserved, including vendor abbreviations. A device's record count is not a
measurement count: one stored record can contain multiple measurements.

## USB implementation

The Rust implementation opens the CP210x virtual COM port at **230400 baud,
8 data bits, no parity, 1 stop bit**, with hardware/software flow control and
DTR/RTS disabled. It implements framed and escaped queries, metadata reads,
group enumeration, and record retrieval. Record fields are decoded in Rust;
Evergreen's installed executable is not loaded or distributed.

Initial compatibility is WR-401 **Rev. 1.04Y6**. Other revisions are unverified.
Only the read/query commands needed by the explorer are implemented. Stateful
record reads are not blindly retried, because doing so could skip records.
An interrupted transfer retains partial data and reports incompleteness.
Unknown fields and unverified checksums are surfaced in warnings, and raw
packets are preserved for investigation. No wireless support is implemented.

## Traverse interpretation

These are equal-area sampling layouts, not an assertion of compliance with a
particular TAB standard or project specification. Use the layout required for
the actual duct and measurement procedure. Circular cells are annular sectors,
with radial sample positions at their area midpoints. The colored field and
animated particles illustrate the entered samples; they are not a CFD model.
Flow is withheld until all cells contain finite, nonnegative velocity values.
CFM equals the mean normal velocity in feet/minute multiplied by duct area in
square feet. A saved result includes the traverse inputs in its source record.

## Command line

The same Rust binary supports non-destructive diagnostics and downloads:

```text
green-suite.exe --probe COM5
green-suite.exe --download COM5 --out ".\downloads"
green-suite.exe --project ".\survey.green-suite.json"
green-suite.exe --pdf-check ".\drawing.pdf"
green-suite.exe --demo
green-suite.exe --demo --view traverse --screenshot ".\preview.png"
```

The optional `tools/export-readings.pl` converts a saved project to normalized
Green Suite CSV version 1 using only Perl core modules:

```text
perl tools/export-readings.pl survey.green-suite.json --output readings.csv
```

`--output` writes UTF-8 directly and avoids shell redirection changing the
encoding on Windows. It refuses to overwrite an existing file. Exports include
mapped device labels and reversible spreadsheet formula escaping, matching the
Rust GUI's CSV format. Run `perl tools/export-readings.pl --self-test` to check
the utility. Perl is not required to run the Rust GUI.

## Planned integrations

Android USB import and AirOne/BuildingStart integration are deferred and are not
implemented or verified. Their implementation requires a separately approved
scope and documented destination formats or APIs.

The proposed workflow is to download WR-401 records through Android USB host
access, review the readings locally, and import selected values into a supported
destination. Work requires CP210x transport and lifecycle validation, explicit
unit and field mappings, authentication review, duplicate handling, and recovery
from interrupted or offline transfers. Acceptance requires a demonstrated USB
download on supported hardware and a verified import into each destination.

Original application code remains Rust and Perl. This roadmap does not add
wireless support or change the project's ownership terms.

## Ownership

See [LICENSE](LICENSE), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), and
the dependency notices. The proprietary notice applies to Kevin R. Whiteman's
original Green Suite code. Evergreen-created/owned material and third-party
libraries retain their respective ownership and license terms. No Evergreen
executable, library, firmware image, or manual is included in the repository.
