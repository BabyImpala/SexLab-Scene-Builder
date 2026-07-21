# FNIS Behavior Interop Spec (SLSB clean-room generator)

Black-box observations from shipped AnimList + `FNIS_*_Behavior.hkx` pairs under `SLAL Packs`, cross-checked against Nemesis `b` templates and AnimlistTransitionTool structure. No FNIS for Modders code is used.

## Ship format (unchanged)

| Artifact | Path |
|----------|------|
| AnimList | `meshes/actors/<race_path>/animations/<pack>/FNIS_<pack>[_<race>]_List.txt` |
| Behavior | `meshes/actors/<race_path>/behaviors/FNIS_<pack>[_<race>]_Behavior.hkx` |
| Wolf exception | `.../canine/behaviors wolf/FNIS_<pack>_wolf_Behavior.hkx` |

Skip generating Behavior for `*_canine_List.txt` (alias list only; dog/wolf carry graphs).

Players continue to run **Pandora** on these FNIS-format packs.

## v1 AnimList support (P+ / SLSB)

Lines: `b [-options] <AnimEvent> <AnimFile.hkx> [AnimObject ...]`

| Option | Effect in graph |
|--------|-----------------|
| `md` | P+ default. Simple clip→state graph (no `BSIsActiveModifier` / `hkbModifierGenerator`) |
| `o` | AnimObjects: enter `AnimObjLoad`+`AnimObjDraw` with string payloads; exit `AnimObjectUnequip` |
| `a` | Clip `mode` = `MODE_SINGLE_PLAY` |
| `Tn` | With `a`: empty `hkbClipTriggerArray` on the clip (FNIS fixed-length marker) |

**Out of v1:** classic `s`/`+` sequences, FootIK `AVbHumanoidFootIKDisable`, non-`md` IsActive wrappers, gender splits, ping-pong `p`.

## Graph skeleton (`b -md`)

```
hkRootLevelContainer
└─ hkbBehaviorGraph "FNISBehavior.hkb"
   ├─ rootGenerator → hkbStateMachine "FNIS_RootBehavior"
   │  └─ state "FNISIdles" → hkbStateMachine "FNISIdlesBehavior"
   │     └─ per anim: StateInfo(eventName) → ClipGenerator
   └─ data → events / variables
```

Shared blend: `hkbBlendingTransitionEffect` `FNIS_06sec_BlendTransition` (0.6s).

Wildcard transitions on `FNISIdlesBehavior`: one per anim event → `toStateId`, flags
`FLAG_DISABLE_CONDITION|FLAG_IS_GLOBAL_WILDCARD|FLAG_IS_LOCAL_WILDCARD`.

### Fixed event name table (indices)

| Id | Name |
|----|------|
| 0 | **Race-specific** default (see below) |
| 1 | `AnimObjectUnequip` (draugr/falmer: `"1"`) |
| 2 | AnimObjLoad |
| 3 | AnimObjDraw |
| 4 | HeadTrackingOff |
| 5 | HeadTrackingOn |
| 6 | StartAnimatedCamera |
| 7 | EndAnimatedCamera |
| 8 | IdleChairSitting |
| 9 | IdleChairGetUp |
| 10 | FNISreserve1 |
| 11+ | per-line AnimEvent |

Event 0 is taken from the vanilla race behavior default FNIS embeds. Observed map (Billy SLP):

| Event 0 | Races |
|---------|-------|
| `IdleForceDefaultState` | `character` |
| `FNISDefault` | `chaurus`, `dwarvenspider` |
| `forceFurnExit` | `draugr`, dwarven centurions, `dlc02/hmdaedra` |
| `idleReturnToDefault` | `vampirelord`, `werewolfbeast` |
| `ReturnToDefault` | `frostbitespider` |
| `ReturnDefaultState` | `ambient/hare`, `slaughterfish` |
| `Reset` | `dragon` |
| `returnToDefault` | all other creature races in corpus |

Variables: `bAnimationDriven` (bool), `IsFNIS` (int32). Attribute: `AttrWM`.

### Per-anim naming

- State / event: AnimEvent from list (e.g. `5a3fB_B_MatingP1_A1_S1`)
- Clip `name`: `{pack}_{fileStem}` (e.g. `Billyy_Human_B_MatingP1_A1_S1`)
- Clip `animationName`: `Animations\{pack}\{file}` (Windows backslashes)

### Enter / exit notifies

**No AnimObjects**

- Enter: event 4 (HeadTrackingOff)
- Exit: event 5 (HeadTrackingOn)

**With AnimObjects (`o`)**

- Enter: 4, then for each AO: (2 + payload), (3 + payload)
- Exit: 5, then 1 (AnimObjectUnequip)

### Acyclic (`a` / `a,Tn`)

- `mode` = `MODE_SINGLE_PLAY`
- `triggers` → empty `hkbClipTriggerArray` when `Tn` present (or whenever `a` is set)

## OSS cross-check

- Nemesis `b` templates: same clip / modifier / idle nesting concepts; FNIS custom graph is a self-contained injectible project, not a Nemesis patch folder.
- AnimlistTransitionTool: Nemesis patch output — reference for transitions/variables only; SLSB does **not** ship that format.
- Pandora: consumes FNIS AnimList + Behavior.hkx via existing FNIS mod path / graph injection.

## HKX toolchain

XML graph authored by SLSB → packed SSE `hk_2010.2.0-r1` in-process via the [serde-hkx](https://github.com/SARDONYX-sard/serde-hkx) `serde_hkx_features` crate (MIT OR Apache-2.0; `tag = 1.0.1`). Requires Rust **1.95+**. Conversion runs on a dedicated large-stack thread to avoid the known debug-build stack overflow.

## Exact-match smoke test

Against SLAL Packs reference Behaviors:

```bash
python3 research/behavior_smoke_test.py --corpus '/mnt/Data/Coding/SLAL Packs/Billy SLP'
```

Or:

```bash
SLSB_BEHAVIOR_CORPUS='/mnt/Data/Coding/SLAL Packs/Billy SLP' \
  cargo test --manifest-path src-tauri/Cargo.toml --bin scene_builder billy_slp_corpus_byte_identical -- --nocapture
```

Fixture unit tests (always on): `chaurus_matches_reference_hkx`, `lesbiandd_matches_reference_hkx`.

