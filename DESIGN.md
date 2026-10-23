# Interface design

Green Suite uses an editorial instrument-workspace layout: a bold serif
masthead, numbered workspace tabs, fine rules, square panels, and an original
concentric-scale engraving. Drawings and measurements remain the main content.
The visual reference is CARI's [Frasurbane](https://cari.institute/aesthetics/frasurbane)
description of layered serif typography, warm earthy tones, paper, architecture,
and classical scientific imagery. No CARI artwork is bundled.

## Color sources

The following colors were verified in the CSS of the
[Evergreen Telemetry website](https://evergreentelemetry.com/) on 2026-10-22.
They are observed website colors, not a claim of an official brand manual.

| Website role | Color |
| --- | --- |
| Primary green | `#00955A` |
| Deep green | `#01613B` |
| Charcoal | `#51565A` |
| Near-black | `#0F1728` |
| Pale neutral | `#EDF0EF` |
| White | `#FFFFFF` |
| Gold highlight | `#FEC84B` |

Primary green marks the instrument emblem. Deep green supports white action
labels with greater contrast. The Evergreen scheme uses the website's neutral
family, with derived tints and dark surfaces for readable interface states.

## Urbane

The secondary Urbane scheme pairs Evergreen green with muted complementary
plum: their approximate hues are 156° and 335°. Reduced saturation, parchment,
and warm neutrals give this palette a warmer character. These are derived design
colors, not Evergreen website tokens.

| Interface role | Light | Dark |
| --- | --- | --- |
| Workspace paper | `#F2E8D5` | `#262521` |
| Panel | `#E9DEC8` | `#32312B` |
| Main text | `#302B29` | `#F2E8D5` |
| Muted text | `#635545` | `#C1B69F` |
| Plum accent | `#784A5D` | `#CBA3B3` |
| Rules and borders | `#88745A` | `#978366` |
| Warning text | `#774713` | `#D6B477` |
| Sage chart endpoint | `#A4B899` | `#526040` |

Dark-mode surfaces and accent tints adapt the palette for contrast. Both schemes
retain deep green for primary actions. Color is accompanied by text, outlines,
and selection state to make meaning clear.

## Interaction and verification

The header palette menu and direct Dark/Light button persist independently.
All text roles use the bundled Source Serif 4 Semibold or Bold faces. The
drawing canvas preserves the PDF's original colors. The traverse chart uses
the same appearance roles as the rest of the workspace, including visible
selection rings and a pause control.

Automated checks cover text contrast in all four scheme/mode combinations,
action-label contrast, chart contrast, and preference keys. Visual checks should
cover the drawing, readings, traverse, and empty-project screens at the minimum
window size as well as larger sizes. Firmware identity and stored-record
downloads are distinct from live sensor monitoring; the USB section is labeled
accordingly.
