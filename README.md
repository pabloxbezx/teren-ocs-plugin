> [!NOTE]
> **Development paused.** Open CAD Studio now includes a built-in **Civil** tab with its own tools for points, surfaces, alignments and profiles. I'll release one more update of Terén for Open CAD Studio 2026.47, so it keeps working for current users. After that, development is paused while I watch how the built-in Civil tools develop over the next few weeks. Then I'll decide whether Terén continues.
>
> Until then, Terén is still fully usable, especially for point import, the 3D terrain viewer and profiles. Bug reports are welcome in [Issues](https://github.com/pabloxbezx/teren-ocs-plugin/issues).

# Terén

**Survey points, terrain surfaces and contours for [Open CAD Studio](https://github.com/HakanSeven12/OpenCADStudio) – free and open source.**

[![Latest release](https://img.shields.io/github/v/release/pabloxbezx/teren-ocs-plugin?label=release)](https://github.com/pabloxbezx/teren-ocs-plugin/releases/latest)
[![Open CAD Studio 2026.46](https://img.shields.io/badge/Open%20CAD%20Studio-2026.46-2b6cb0)](https://github.com/HakanSeven12/OpenCADStudio/releases)
[![License: GPL-3.0-only](https://img.shields.io/badge/license-GPL--3.0--only-blue)](https://www.gnu.org/licenses/gpl-3.0.html)

*Česky: [README.cs.md](README.cs.md)*

Import a survey and get a TIN surface with labelled contours in a minute.
Edit points and triangles right in the drawing – everything stays in the
DWG, and every action is a single Undo step. Interface in English or Czech.

<table>
<tr>
<td><img src="docs/img/contours.png" width="400" height="260" alt="Contours with labels and survey points"></td>
<td><img src="docs/img/viewer-3d.png" width="400" height="270" alt="3D viewer – surface coloured by elevation"></td>
</tr>
</table>

## What's new in 1.3.1

- **Alignment editor** – a new docked panel (*Alignment editor* button):
  a vertex table with X, Y, curve type (no curve / circular arc / with
  transitions), R, transitions L1 and L2, parameter A, deflection angle and
  the tangent between curves – an overwritten value is written to the
  drawing shortly after you stop typing. The selected vertex is switched
  with a bar (Previous / Next, list) or by clicking its grip, it is
  highlighted in the drawing and *Show in drawing* zooms to it. Details show
  tangents T1 / T2, curve lengths, A, shift p and stations.
- **Transition curves (clothoids)** before and after the arc, symmetric or
  unsymmetric, given by length L or parameter A. Key points TS, SC, CS, ST
  with stations. The radius grip scales R and the transitions together.
- **Alignment tools** in three groups – Vertices (add, delete, move exactly
  by typed X,Y, extend), Curves (insert from settings, change R, remove, at
  all vertices) and Transitions (add, remove, to all curves). When a curve
  does not fit, the transitions are shortened first, then the radius (can be
  switched off).
- **Alignment check** in the panel: overlapping curves with the overlap in
  metres, small radius, parameter A outside R/3 to R, short tangents;
  reverse curves meeting at the transitions' inflection point are fine.
- **English layer names** – `TER-PNT-…` (points), `TER-SRF-<surface>-…`
  (surfaces), `TER-ALN-…` (alignments), `TER-PRF-…` (profile views), with
  a localized description. Layers from older versions are renamed
  automatically when the drawing is opened.
- **Sample drawing** with English names (Area 1–3, Alignment 1, Grade 1).

## What's new in 1.3.0

- **Design profile editor** – a new docked panel (*Design profile editor*
  button, or click a profile view): a vertex table with station, elevation,
  curve type, R | L | K, segment grade and length – an overwritten value is
  written to the drawing right away. Details of the selected vertex (picked
  from a list, with Previous / Next, by editing its row or by clicking it in
  the view) show only the fields that apply; the selected vertex is
  highlighted in the view and *Show in drawing* zooms to it.
- **Design profile tools** in three groups – Vertices (add, delete, raise /
  lower a vertex or the whole profile, vertex to terrain), Curves (insert and
  remove curves by clicking vertices, curves at all vertices – from the
  *Curve settings for inserting*) and Segments (change grade, change length,
  through a point, extend). *Keep profile end* decides whether the rest of
  the profile moves when a segment changes. New vertices have no curve.
- **Vertical curves by radius R, length L or K value** (K = L / grade change
  in %, as in 12d); the other two are calculated, and an entered length L is
  kept when grades change. Suggested in #2.
- **Curve types:** parabola (default), unsymmetrical (L1 before and L2 after
  the vertex) and circle. Curves at neighbouring vertices may touch without
  a tangent between them; an overlap is reported with its length.
- **Curve labels in the profile view** (R · L · K), switched off in the
  panel or the profile style. The tangent polygon is drawn only at curves,
  so the whole design profile stays visible.
- **Ribbon:** the Long profile group now mirrors Alignments – Long profile,
  Design profile, Design profile editor, a *Design profile vertices* menu and
  *Design profile properties*. The recalculation mode is set in the Prospector.

## What's new in 1.2.0

- **Smoother editing.** Fewer round trips to Open CAD Studio after every
  change; moving a point, editing an alignment or redrawing a profile view
  rewrites objects in place, so selection and grips stay.
- **Recalculate when you're done.** New *Recalculation* menu: surfaces and
  profiles update immediately, after a short pause (default), or only on
  *Recalculate now*.
- **Large drawings no longer crash** – a clear message instead.
- **Long profiles in a drawing-office look** (ISOCPEUR, numbers on the
  verticals) with three styles – simple, water and sewer (pipe bands ready
  for later), ground and excavation – and a profile appearance editor with
  live preview.
- **Alignment styles** (no-plot grey, red dash-dot, red solid, red tangents
  with blue arcs) and a new sample drawing.

## What's new in 1.1.0

- **Open CAD Studio 2026.46.** Built for the current release.
- **Alignments and long profiles – work-in-progress concept.** Draw an
  alignment from a polyline or by picking vertices, add a long profile view
  with the terrain from your surfaces, a design profile and data bands.
  Try it out, but expect features, look and storage in the drawing to
  change; the manual does not cover it yet.
- **Styles for profiles and alignments.** Profile styles (look and set of
  bands, templates for general use, water and sewer, and watercourses) and
  alignment styles (stations, labels, colours, lineweights) are stored in
  the drawing; editing a style redraws everything that uses it.
- **Faster surfaces.** Triangles are stored as a mesh instead of thousands
  of 3D faces, so large surfaces insert, save and delete much faster.

All releases: [Releases](https://github.com/pabloxbezx/teren-ocs-plugin/releases) ·
change log in Czech: [README.cs.md](README.cs.md#co-je-nového)

## Features

<table>
<tr>
<td width="400" valign="top"><img src="docs/img/import.png" width="400" height="260" alt="Point import with column roles and checks"></td>
<td valign="top">

### Point import
Text and CSV in any column order, clipboard, Excel, LandXML, Trimble
JobXML, Leica GSI and points drawn in the drawing. Format, delimiter,
encoding and S-JTSK coordinates are detected automatically; errors and
duplicates show up before anything is imported.

</td>
</tr>
<tr>
<td valign="top"><img src="docs/img/surface-card.png" width="400" height="260" alt="Prospector – surface card"></td>
<td valign="top">

### Surfaces and the Prospector
Delaunay TIN from point groups and drawing objects with breaklines,
boundaries and holes. Minor and major contours with labels, triangles and
slope arrows set by surface styles. The Prospector shows every point, group,
surface and style in the drawing, with status and the main numbers at a
glance.

</td>
</tr>
<tr>
<td valign="top"><img src="docs/img/profile-view.png" width="400" height="260" alt="Long profile view card"></td>
<td valign="top">

### Alignments and long profiles <sub>concept</sub>
Horizontal alignments with tangents and arcs, stationing and key points.
Long profile views with the terrain line, a design profile and bands above
and below the graph, drawn to the scale and exaggeration you choose.

</td>
</tr>
<tr>
<td valign="top"><img src="docs/img/alignment-style.png" width="400" height="145" alt="Alignment style – colours and lineweights"></td>
<td valign="top">

### Alignment and profile styles
**Alignment styles** set stationing intervals and format, key-point and
vertex labels, the tangent polygon, and the colour and lineweight of the
centreline, stations, key points and polygon. **Profile styles** set the
scale, vertical exaggeration, formats and the set of bands above and below
the graph, with templates for general use, water and sewer, and
watercourses. Like point and surface styles they are stored in the drawing
– built-in styles can be adjusted or reset, your own travel with the DWG.

</td>
</tr>
</table>

**Also:** point groups by plain-language rules, point styles with markers
and labels by code or group, triangle edits (swap, add or delete an edge),
moving points by grip, a 3D viewer coloured by elevation or slope, export
of surfaces to LandXML 1.2 or DXF and of points to text files.

## Try it in 2 minutes

1. In Open CAD Studio 2026.46 open the **Plugin Manager**, find *Terén* and
   click Install. After a restart the **Terén** tab appears.
2. Open the sample drawing
   [teren-sample.dwg](https://github.com/pabloxbezx/teren-ocs-plugin/raw/main/ukazka/teren-sample.dwg)
   – a small forest survey with a finished surface.
3. Open the **Prospector**, click the surface, then try **View in 3D** or
   drag a point with **Move points** on (Ctrl+Z undoes it).

Your own survey: **Import points** › pick the file › check the preview ›
Import, then **New surface** › **Existing ground (from points)**.

## Roadmap

- **Now – settle and polish.** Alignments, long profiles and their styles
  keep their current shape. The next releases fine-tune them: how profile
  views and bands look, alignment and profile styles, editing in the
  Prospector, and the manual.
- **Next – corridors.** Once alignments and profiles are stable, a corridor
  builds on them: a cross-section template swept along the alignment and
  design profile, giving the design surface of a road, path or channel.

No dates promised – feedback decides what comes first.

## Compatibility and installation

Each Terén release is built for one Open CAD Studio release; the Plugin
Manager offers the right one.

| Open CAD Studio | Terén |
|---|---|
| 2026.46 | 1.1.0 – 1.3.1 |
| 2026.45 | 1.0.2 – 1.0.9 |
| 2026.44 | 1.0.0 – 1.0.1 |

- **Manually (Windows):** download `Teren-windows.zip` from the
  [latest release](https://github.com/pabloxbezx/teren-ocs-plugin/releases/latest),
  extract it and run `nainstalovat.bat`.
- **From source:** `cargo build --release -p teren_plugin` (any stable
  Rust), then copy the library and `plugin/plugin.toml` to
  `%APPDATA%\OpenCADStudio\plugins\cz.fluvio.teren`.

Language: Prospector › Settings › Jazyk / Language (automatic, Czech,
English), applied after a restart.

## Help and feedback

- **Manual** (PDF): [English](manual/Teren-manual-en.pdf) ·
  [česky](manual/Teren-manual-cs.pdf); also attached to every release.
  In the app: Terén tab › Help, or F1 in the Prospector.
- **Found a bug or missing something?**
  [Open an issue](https://github.com/pabloxbezx/teren-ocs-plugin/issues) –
  a short description and, if possible, a small made-up survey file help
  most.

## For developers

- `src/` – the `teren` library: point import, TIN, breaklines, contours,
  groups, LandXML/DXF writers (also a small command-line tool that writes
  DXF).
- `trasy/` – alignments, profiles, bands and their styles.
- `plugin/` – the Open CAD Studio plugin (ribbon, commands, Prospector in
  egui).
- `preklady/en.txt` – translations, one line per text: Czech, tab, English.
- `manual/` – manual sources (HTML) and PDFs.

## License

GPL-3.0-only.
