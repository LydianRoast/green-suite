# Interface design

Green Suite is a compact technical workspace. Four direct tabs—**Plan**,
**Readings**, **Traverse**, and **Live**—keep the active task in view. The header
holds project and appearance controls. Drawing navigation and the point
inspector appear only in Plan; USB controls appear in Readings and Live.
Setup instructions, source data, and secondary details expand when needed.

The layout uses square controls, clear outlines, restrained spacing, and a
consistent type scale. Drawings and measurements fill the main workspace.
The drawing canvas preserves the PDF's original colors. Live values retain
their reported precision, units, age, and status; stored records and live
reception remain separate operations.

## Typography

All interface text uses bundled **Iosevka Slab Extended Semibold and Bold**.
This serif monospace provides aligned numeric columns and a technical rhythm,
with weights 600 and 700 for legibility. The unmodified static fonts are
distributed under SIL Open Font License 1.1; see
[third-party notices](THIRD_PARTY_NOTICES.md). No font installation is needed.

## Color

Both palettes use black, white, and neutral gray surfaces. Accents are saturated;
there are no pastel panel or chart fills. Color is accompanied by labels,
outlines, and selection state.

The Evergreen palette retains the greens observed in the
[Evergreen Telemetry website](https://evergreentelemetry.com/) CSS on 2026-10-22:
brand green `#00955A` and deep action green `#01613B`. These are observed website
colors, not a claim of an official brand manual. Other foreground colors are
adapted for high contrast.

Urbane uses vivid violet/magenta against the same neutral surfaces, with green
and amber supporting accents. The palette and Dark/Light mode are remembered
independently.

| Role | Light | Dark |
| --- | --- | --- |
| Workspace | `#FFFFFF` | `#000000` |
| Panel | `#FFFFFF` | `#050505` |
| Main text | `#000000` | `#FFFFFF` |
| Secondary text | `#303030` | `#DDDDDD` |
| Rules | `#000000` | `#FFFFFF` |
| Evergreen accent | `#004127` | `#00EF83` |
| Urbane accent | `#4B007D` | `#FF00FF` |
| Warning text | `#700000` | `#FFCC00` |

Evergreen action buttons use `#01613B`; Urbane action buttons use `#4B007D`.
Both carry white labels. Charts use a neutral intensity scale with saturated
outlines and selection marks.

## Verification

Automated checks require main text to reach at least **12:1** contrast on the
workspace, panel, selection, and alternate surfaces; secondary text and action
labels must reach **7:1**. Other semantic text roles meet **4.5:1**, and chart
labels meet **7:1**. Tests also verify neutral surfaces and stable preference
keys across both palettes and modes.

The CPU-only `--preview-ui` command renders the actual interface to PNG without
a native window, GPU, USB access, or runtime workspace reads. `--demo` supplies
explicitly labeled example data. Review all four views in both palettes and
modes, including the 1000 × 600 minimum size, for clipping and visual hierarchy.
