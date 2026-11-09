# Terén – terrain plugin for Open CAD Studio

*Česky: [README.cs.md](README.cs.md)*

Survey points, TIN surfaces, breaklines, boundaries and holes, contours,
a 3D viewer and surface export to LandXML / DXF – directly in an
Open CAD Studio drawing. The interface is in Czech or English. A short
manual (PDF, Czech and English) is in the `manual` folder and attached to
each release.

## Features

- **Points** – import from text files (PNEZD, PENZD, PYXZD, NEZ, ENZ…, the
  format and delimiter are detected automatically, with a preview), point
  styles (markers incl. custom blocks, labels that move aside when they
  overlap), point groups by code / number / import file, manual points,
  survey check of suspicious elevations.
- **Surfaces** – Delaunay TIN from point groups and drawing objects,
  breaklines, boundaries and holes, max. edge length, manual triangle edits
  (swap / add / delete edge, delete triangle, add / remove point) stored with
  the surface, automatic recalculation.
- **Display** – contours (minor / major, labels), triangles, outline, slope
  arrows; surface styles; 3D viewer with colours by elevation or slope.
- **Prospector** – a window with the selection, a tree of everything in the
  drawing (points, groups, surfaces, styles, settings), help and a wizard.
- **Export** – surface to LandXML 1.2 or DXF (3D faces), points to text files.
- Everything is stored in the drawing (DWG/DXF); each action is a single
  undo step.

Czech survey conventions are supported (S-JTSK coordinates, PYXZD files,
Windows-1250 encoding).

## Installation

Each Terén release is built for one Open CAD Studio release (the host checks
that both sides use the same drawing data format):

| Open CAD Studio | Terén |
|---|---|
| 2026.45 | 1.0.2 |
| 2026.44 | 1.0.0 – 1.0.1 |

- **Plugin Manager** in Open CAD Studio: find *Terén* and click Install.
- **Manually (Windows):** download `Teren-windows.zip` from Releases,
  extract it and run `nainstalovat.bat`. The plugin is copied to
  `%APPDATA%\OpenCADStudio\plugins\cz.fluvio.teren`.
- **From source:** `cargo build --release -p teren_plugin` (any stable
  Rust) and copy the library together with `plugin/plugin.toml` into the
  plugins folder.

The language is set in Prospector › Settings › Jazyk / Language (automatic,
Czech, English) and takes effect after a restart. Translations live in
`preklady/en.txt` (one line per text: Czech, tab, English) – other languages
can be added the same way.

## Sample drawing

`ukazka/teren-sample.dwg` – a small forest survey with an existing-ground
surface: survey points with styles (trees, poles, manholes…), breaklines,
outline, contours and labels. Open it with the Terén plugin installed and
use the Prospector to explore the points, groups, surface and styles.

## Repository layout

- `src/` – the `teren` library: point import, TIN, breaklines, contours,
  groups, LandXML/DXF writers, translations (also a small command-line tool).
- `plugin/` – the Open CAD Studio plugin (ribbon, commands, Prospector in egui).
- `preklady/` – translations.
- `manual/` – manual sources (HTML) and PDFs.

## License

GPL-3.0-only. Author: Ing. Pavel Janouš.
