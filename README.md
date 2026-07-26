# SexLab Scene Builder

An UI build with Tauri to create .SLSB files which are used by [SexLab P+](https://github.com/Scrabx3/SexLabpp) to read animation data.

## OStim import: scene grouping

OStim packs are many JSON nodes linked by `navigations` / `destination`. On import, SLSB builds the undirected navigation graph and turns each **weakly connected component** into one editor `Scene`.

That matches SexLab++: stage-graph edges can only point at stages **inside the same scene**. Splitting a densely linked pack (e.g. Bloo) by folder would look tidier in the editor but would break or drop those hops in `.slr` export. Folder names are kept as `ostim_folder:` on stages so the editor can open a **virtual canvas** per folder (only those stages are mounted; cross-folder links show as teal dotted bridge edges to portal stubs). The underlying `Scene.graph` stays whole for export.

Cross-pack / vanilla links stay as `ostim_nav*` tags and round-trip on OStim export; they are not SexLab stage edges.

## OStim pack folders (authoring)

On disk, packs look like:

```text
SKSE/Plugins/OStim/scenes/
  Back/poseA.json
  Lay/poseB.json
  Standing/…
```

Those subfolders are **organizational splits**, not separate SexLab scenes. In SLSB:

| Concept | Meaning |
|--------|---------|
| `ostim_folder:Back` on a stage | That stage’s disk folder; drives **Canvas: Back** and export path |
| Canvas folder view | Editor-only subset; graph edges across folders stay in one scene |
| Teal dotted edge → portal | Hop to another pack folder; click opens that canvas |
| **+ Folder** / stage **OStim pack folder** field | Create or assign splits when building a new pack |
| **Add Stage** while a canvas folder is open | New stage inherits that folder tag |
| Right-click stage / empty canvas | Quick pack actions (move folder, new folder, open canvas, add stage) |

Prefer per-stage folders (stage editor or **Set folder** on the focused node). A scene-level `ostim_folder:` tag is only an export fallback when a stage has none.

## Recommended IDE Setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
