# Stored USB compatibility

Compatibility is specific to an operation, firmware, and available evidence.
Recognizing a unit or parsing a synthetic frame does not validate a sensor family.
No sensor connects directly to Green Suite.

| Firmware / data | Stored download | Interpretation evidence | Checksum status | Export comparison |
| --- | --- | --- | --- | --- |
| WR-401 Rev. 1.04Y6: identity and inventory | Hardware identification and full stored download verified | Model, serial, firmware, group and record counts | Supported metadata checksums required | Not a measurement export |
| Rev. 1.04Y6: temperature and pressure records | Hardware download verified | Private records and unit/channel interpretation; module family not inferred from units | Some stored-record checksums remain undocumented; raw packets retained | Private native-export regression available as an opt-in test; no public captures |
| Velocity and airflow record formats | Decoder present | Synthetic frame regression; does not establish all module/firmware combinations | Supported variants recognized; other variants explicitly unverified | Broader hardware/native-export validation required |
| Humidity, dew point, water-flow unit labels | Generic stored-field decoder present | Unit classification only; module-specific compatibility unverified | Requires capture-specific validation | Additional hardware/export comparison required |
| Other firmware, module families or unknown unit labels | Unverified | Retain unknown fields/raw data; avoid automatic interpretation | Unverified until demonstrated | Required before claiming support |

## Offline replay

`green-suite.exe --replay-usb capture.raw.ndjson --report replay-summary.json`
uses the same frame reader and record decoder as stored USB downloads. It opens
no device, sends no protocol commands, and creates no GUI. The optional report
file must not already exist. The default report contains counts and bounded
diagnostic codes; identifiers, group labels, measurements, and source packet
bytes are omitted. Treat even a summary as private operational information.

Replay checks outgoing read-command forms, request and response checksums,
response matching, logged-payload consistency, frame boundaries, record layout,
and record-cursor context. It reports repeated exchanges without deleting or
deduplicating measurements. An identical packet can represent a legitimate
repeated observation. Unverified record checksums are counted explicitly;
unverified metadata checksums are rejected, matching the live download policy.

Malformed/truncated inputs produce diagnostic codes and a failing CLI exit
status. The capture's reported completeness is shown separately: replay of
received bytes cannot prove that all instrument records were captured or that
the instrument clock was correct. Physical link recovery, undocumented checksum
variants, late replies crossing transaction boundaries, and missing responses
still require additional evidence. Synthetic tests establish parser behavior,
not hardware compatibility.

Input limits are 128 MiB per capture and 32 KiB per NDJSON line. Reported issue
details are capped at 100 while total issue counts continue to accumulate.

The replay implementation has also been exercised against an existing private
stored download. It decoded the captured exchanges and preserved explicit
warnings for undocumented record checksums. This is an offline parser check,
not a new physical connection or a resolution of the USB hardware issue.

## Connection ownership and identity

Identification, stored downloads, and existing live reception share application
port ownership. An operation cannot acquire a port already owned in this
process; Windows exclusive handles continue to guard against other processes.
Ownership ends after the handle closes, including failures during serial setup.

GUI downloads re-read Reporter metadata on the opened session and compare its
serial with the previously identified Reporter before starting the stored-record
cursor. Refreshing an absent port clears selection rather than silently choosing
another instrument. Select and identify a replacement explicitly.

The native workspace polls passive OS inventory on a background worker every
two seconds. Available VID/PID, bridge serial and Windows location are hints;
missing or shared values do not establish Reporter identity. A disappearance or
changed binding clears the selection and identified state. Reappearance never
selects a port automatically. Reconnect verifies the previous Reporter serial;
identifying a different Reporter is a separate explicit action. Late identity
results cannot restore identified state after the selection/binding changes.
An enumeration failure retains the last successful list and exposes the error
state. Received project readings are preserved through inventory transitions.

USB details offers a redacted JSON summary, also available headlessly through
`green-suite.exe --usb-inventory --report usb-summary.json`. It opens no serial
handle and sends no protocol commands. The summary includes availability flags,
VID/PID, anonymous port aliases and up to 64 recent transition events, omitting
port names, bridge/Reporter serials, locations, free-form errors, protocol bytes
and project data. Export never replaces an existing file. Preview rendering does
not enable OS polling. Synthetic transition tests do not establish physical USB
recovery, driver health or a safe resumable cursor operation.
