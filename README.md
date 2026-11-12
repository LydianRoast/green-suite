# Green Suite

A measurement workspace for mechanical test and balance work.

## Workspace

- **Plan:** import a multipage mechanical PDF, switch sheets, pan and
  zoom, and place or reposition supply, return, exhaust, duct traverse, valve,
  and pipe markers. Each marker stores its equipment/system, design value,
  units, notes, and attached measurements.
- **Readings:** connect a WR-401, inspect its identity and inventory,
  download stored records, filter measurements, inspect source records, and
  attach readings to drawing markers. Import Evergreen CSV exports or a
  long-form CSV; export normalized measurements as CSV.
- **Live:** receive pressure measurements from a linked SNE module over
  USB, preserve the displayed decimals, inspect a recent trend and reading age,
  and explicitly capture a reading into the project or a selected drawing point.
  Initial live compatibility is WR-401 Rev. 1.04Y6 with one displayed pressure
  channel per SNE module; other module families and layouts remain unverified.
- **Traverse:** rectangular midpoint grids and circular equal-area annular
  sectors; editable velocity samples; assignment of downloaded velocity
  measurements; animated flow, sample selection, and a color field. Pause the
  animation when desired. Dimensions are inches and velocity is feet/minute;
  m/s measurements are converted when assigned. Complete traverses can be saved
  as calculated CFM readings and attached to a mapped device.
- **Local projects:** autosave, explicit Save/Save as, and reopen. Clearly
  labeled example data is available separately from actual instrument data.

## Interface

**Readings → Analysis** provides stored pressure/temperature comparisons,
trend/stability summaries, selected-device balancing, and hydronic heat-transfer
and Cv/Kv valve-flow estimates. **Air state** adds humidity-ratio diagrams,
dew/frost point, enthalpy and moisture-removal estimates from stored temperature
and humidity records. Barometric pressure, fluid properties and valve settings
are explicit.
Time pairing and
sampling gaps are explicit; missing balancing inputs withhold complete totals.
Saved calculated readings retain their selected inputs and rule versions.
See [stored-reading analysis](ANALYSIS.md) for workflows and limitations.

The compact interface uses **Plan**, **Readings**, **Traverse**, and **Live**
tabs. Drawing navigation and the point inspector appear in Plan; USB controls
appear in Readings and Live. Setup instructions and secondary details expand
when needed.

Bundled **Iosevka Slab Extended Semibold and Bold** provides serif monospace
type throughout. Evergreen and Urbane palettes combine black/white contrast
with saturated green, violet, and amber accents. Dark and Light modes are
remembered between launches. See [Interface design](DESIGN.md) for color roles
and contrast checks. Measurement displays retain recorded numeric precision,
and incomplete USB downloads remain clearly identified.

## Run and build

Requires a Rust toolchain supporting edition 2024 and the pinned dependencies
in `Cargo.lock` (tested with Rust 1.96 on Windows 10 x64). On Windows the
MSVC C++ build tools and Windows SDK are needed to link the native application.

```text
cargo run --locked
cargo test --locked
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

The built Windows program is `target/release/green-suite.exe`. Connect the
Wrist Reporter with a **mini-USB data cable**. The Silicon Labs CP210x driver
must expose a serial port. Select the port, click **Connect**, then
**Download records**. Downloads do not erase instrument memory or change firmware.

Close other software using that serial port before connecting. A port may
change when an instrument is connected through a different USB socket.

### USB recovery

The native app tracks passive port arrivals/departures and bridge changes.
Select a current port and use **Reconnect** to verify the previous Reporter;
**USB details** provides bridge hints and a redacted diagnostic export.
Received readings remain in the project. Reconnecting starts a new identification;
a subsequent download starts from the beginning and never resumes a record cursor.

Stored downloads and live reception share the same 230400-baud, 8N1 setup,
with flow control, DTR and RTS disabled. Connection failures distinguish a
missing port, denied exclusive access, and a Windows device/driver failure.
The diagnostic handle check sends no Reporter protocol commands.

- **Port absent / Win32 2 or 1167:** refresh the port list and check the device
  in Windows Device Manager. A previously assigned COM number can be stale.
- **Access denied / Win32 5 or 32:** check applications holding the port and
  account access. This message does not prove another program owns the port.
- **Device I/O failure / Win32 31 or 1117:** inspect USB status, driver, cable
  and device power rather than repeatedly attempting a record download.
- **Device Descriptor Request Failed / Windows Code 43:** Windows has not
  successfully identified the USB device, so the CP210x serial driver cannot
  yet provide a usable port. Reconnect with a known-good data cable and USB
  port. Updating the serial driver alone cannot guarantee recovery.

Use a signed CP210x VCP package compatible with the Windows version from
[Silicon Labs](https://www.silabs.com/software-and-tools/usb-to-uart-bridge-vcp-drivers).
Green Suite does not silently install drivers, restart USB devices, reset hubs,
or change Windows security settings.

Idempotent device metadata queries retry at most three times on I/O timeouts,
clearing stale input between attempts. Invalid responses and non-timeout
failures stop immediately. Group/record cursor commands are attempted once;
they are never automatically replayed. This avoids silently skipping records.

Serial setup clears a lingering transmit break and waits 500 milliseconds before
the first metadata query. `--probe COM5 --out ".\diagnostics"` saves a new raw
diagnostic capture, including failures; it never replaces an existing capture.
Successful USB enumeration alone does not prove the instrument will answer queries.

`cargo build --locked` produces `target/debug/green-suite.exe`. This profile
uses modest optimization, omits debug symbols, and disables incremental output.

### Updating a local installation

Updates are manual; the application does not download or install its own updates.
Run the checks above, build the chosen profile, and preserve a rollback copy of
the installed executable before replacing it. Close the application before
replacing an executable that is in use. Keep launch shortcuts pointed at the
chosen installed executable and verify it matches the completed build.

An executable installed in the repository root uses the repository's `workspace`
directory, regardless of its filename. Keep that directory when updating to
preserve projects, appearance preferences, and private captures. Build output
and installed executables are excluded from Git; source commits and executable
deployment are separate steps.

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
Stored-record retrieval uses read/query commands. Stateful record reads are not
blindly retried, because doing so could skip records.
An interrupted transfer retains partial data and reports incompleteness.
Unknown fields and unverified checksums are surfaced in warnings, and raw
packets are preserved for investigation. No wireless support is implemented.

The stored-data pipeline is memory → USB queries → Rust decoding → raw
capture and normalized measurements → Readings and drawing attachments.
Live reception uses a separate serial session, described below. Evergreen
documents USB real-time logging in its
[User Guide, page 19](https://img1.wsimg.com/blobby/go/a3cb8563-340e-41b0-af11-2e1965d0fece/downloads/Wrist%20Reporter%20User%20Guide.pdf?ver=1733769186267#page=19).

## Live pressure reception

Turn the pressure modules off before identifying the Reporter. If it was in a
previous live session, restart it while the modules remain off. In Green Suite,
select the USB port and click **Connect**. If identification times out, turn the
modules off, restart the Reporter, and retry **Connect** before turning the
modules on.

Once the Reporter is identified, turn the pressure modules on, pair them if
needed, and wait for their normal readings on the Reporter. Open **Live**
and click **Start live reception**. The live session fixes the linked module set,
enters the Reporter's RTU mode, and selects its history screen. USB downloads and
port changes are unavailable while reception owns the port.

The GUI displays validated readings and their original numeric text, including
negative pressure and trailing decimal places. The wire unit is preserved
(for example, `inW`). A channel becomes stale after 15 seconds without a valid
update. Update cadence comes from the instrument; the application does not
manufacture samples or promise a fixed interval. Each trend retains at most
180 samples. **Capture reading** saves the current valid value; optional
attachment links it to the selected drawing point. Capturing does not write a
record back to the instrument.

**Stop receiving closes the PC port; it does not restore the Reporter mode.**
For normal use after a session, turn the sensors off, turn the Reporter off and
back on, then turn the sensors on and let them link. This follows the vendor's
[recovery procedure, page 20](https://img1.wsimg.com/blobby/go/a3cb8563-340e-41b0-af11-2e1965d0fece/downloads/Wrist%20Reporter%20User%20Guide.pdf?ver=1733769186267#page=20).
No undocumented unlock, reset, erase, calibration, or firmware command is sent.

Live capture files are private `workspace/live-captures/*.raw.ndjson` files.
Each session uses a new file and stops at a 32 MiB limit. Rejected frames remain
in the raw capture but cannot update a displayed or captured measurement.
Device timestamps remain instrument-local; PC receipt times are stored
separately. If the display queue falls behind, the raw log retains the received
frames and the GUI reports dropped display updates.

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
green-suite.exe --probe COM5 --out ".\diagnostics"
green-suite.exe --download COM5 --out ".\downloads"
green-suite.exe --project ".\survey.green-suite.json"
green-suite.exe --pdf-check ".\drawing.pdf"
green-suite.exe --demo
green-suite.exe --demo --view traverse --screenshot ".\preview.png"
green-suite.exe --preview-ui ".\preview.png" --demo --view live --theme dark --palette evergreen --size 1000x600
green-suite.exe --view live
green-suite.exe --capture-live COM5 --seconds 30 --out ".\live-captures"
```

`--capture-live` performs the same mode-changing live session for a bounded
duration (1–3600 seconds), preserves raw frames, and reports its latest validated
sample. It requires the same instrument recovery procedure as the GUI.

`--preview-ui` renders the actual interface to a PNG using the CPU, without
opening a window, using a GPU, accessing USB, or reading the runtime workspace.
`--demo` uses explicitly labeled example data. Choose `--theme light|dark`,
`--palette evergreen|urbane`, and `--size WIDTHxHEIGHT`; omit `--demo` for an
empty workspace preview. The separate `--screenshot` option opens a native
window before capturing it.

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

Source-record review includes optional measurement provenance, calibration notes,
and unit-conversion previews. Saving a conversion preserves its inputs and creates
a separate calculated reading. See [measurement records](MEASUREMENTS.md) for unit
conventions, metadata exports, and the distinction between absolute temperatures
and temperature differences.

Stored USB captures can now be checked offline with
`green-suite.exe --replay-usb capture.raw.ndjson --report replay-summary.json`.
The report omits source values and identifiers and never opens a device. See
[compatibility and replay](COMPATIBILITY.md) for evidence levels, limitations,
and the connection ownership and Reporter identity safeguards.

[Stored-reading development plan](DEVELOPMENT.md) covers connection management,
protocol replay and validation, measurement provenance, unit conversions, and
pressure, airflow, temperature, psychrometric, and hydronic analysis workflows.
It distinguishes implemented features from remaining validation and deferred
work. Sensors remain paired to the Wrist Reporter;
additional live sensor support and wireless phone connectivity are outside this
plan.

AirOne/BuildingStart integration is deferred and is not implemented or verified.
The proposed desktop workflow is to download WR-401 records over USB, review
them locally, and import selected values into a supported destination. Work
requires documented destination formats or APIs, explicit unit and field
mappings, authentication review, duplicate handling, and recovery from
interrupted or offline transfers. Acceptance requires a verified import into
each destination. Android USB and mobile application work are excluded.

## Ownership

See [LICENSE](LICENSE), [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md), and
the dependency notices. The proprietary notice applies to original Green Suite
code. Evergreen-created/owned material and third-party
libraries retain their respective ownership and license terms. No Evergreen
executable, library, firmware image, or manual is included in the repository.
