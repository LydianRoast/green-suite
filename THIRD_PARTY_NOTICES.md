# Third-party notices

Original Green Suite code is copyright 2026 Kevin R. Whiteman, all rights
reserved. Dependency licenses continue to apply to their respective code.
The exact resolved dependency versions are recorded in `Cargo.lock`.

[THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt) contains the license texts
and notices collected for all 197 packages in the resolved Cargo dependency
graph for `x86_64-pc-windows-msvc`, including transitive and build dependencies.
Each entry identifies the package, version, declared license expression, and
source. Nested font and asset license files are included. Where a published
crate omits its repository's license files, the collection includes those files
from the exact upstream commit recorded in the crate's `.cargo_vcs_info.json`.
License expressions below are copied from package metadata, including older
slash notation; the original license texts govern the respective components.

| Direct dependency | Locked version | Declared license expression |
| --- | --- | --- |
| anyhow | 1.0.104 | MIT OR Apache-2.0 |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| csv | 1.4.0 | Unlicense/MIT |
| eframe | 0.33.3 | MIT OR Apache-2.0 |
| hayro | 0.8.0 | Apache-2.0 OR MIT |
| image | 0.25.10 | MIT OR Apache-2.0 |
| rfd | 0.15.4 | MIT |
| serde | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| serialport | 4.10.1 | MPL-2.0 |
| uuid | 1.28.0 | Apache-2.0 OR MIT |

Green Suite also bundles unmodified **Source Serif 4 Semibold and Bold** static
TrueType fonts from Adobe's [4.005R release](https://github.com/adobe-fonts/source-serif/releases/tag/4.005R),
version **4.005**, pinned to upstream commit
`2823e993c53fca27c5c8749f529b56a5a7c77b6b`.
The files are `assets/fonts/SourceSerif4-Semibold.ttf` and
`assets/fonts/SourceSerif4-Bold.ttf`. Copyright 2014–2023 Adobe; the Reserved
Font Name is “Source”. These font files retain the **SIL Open Font License 1.1**,
reproduced with Adobe's notice in
[assets/fonts/OFL-SourceSerif4.md](assets/fonts/OFL-SourceSerif4.md) and in the
aggregate license collection. They are not covered by Green Suite's
proprietary source-code license. Their original upstream paths are
[`TTF/SourceSerif4-Semibold.ttf`](https://github.com/adobe-fonts/source-serif/blob/2823e993c53fca27c5c8749f529b56a5a7c77b6b/TTF/SourceSerif4-Semibold.ttf)
and [`TTF/SourceSerif4-Bold.ttf`](https://github.com/adobe-fonts/source-serif/blob/2823e993c53fca27c5c8749f529b56a5a7c77b6b/TTF/SourceSerif4-Bold.ttf).

The embedded `epaint_default_fonts` package declares
`(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0`; its font-specific OFL,
Ubuntu Font License, and emoji font notices are included in the collection.
Hayro's embedded Foxit font, CGATS color-profile, and Adobe CMap notices are
also included. These font and asset terms are separate from Green Suite's
proprietary source-code license.

The unmodified MPL-covered `serialport` 4.10.1 source is available in its
[versioned crate archive](https://crates.io/api/v1/crates/serialport/4.10.1/download)
and [upstream repository](https://github.com/serialport/serialport-rs).
The full MPL-2.0 text is included in the license collection. Green Suite's
all-rights-reserved notice does not replace the rights granted for any
third-party component.

This collection covers the Windows dependency resolution recorded in the
current `Cargo.lock`; regenerate it when dependency versions or build targets
change. Standard Rust toolchain and Windows system components retain their
own licenses. No Evergreen binary, source file, or firmware is bundled.

Evergreen Telemetry and Wrist Reporter are names belonging to their respective
owners. Green Suite is an independent project. Referencing device behavior
does not imply endorsement or affiliation.
