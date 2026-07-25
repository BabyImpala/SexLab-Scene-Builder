//! Convert between OStim scene JSON nodes and SLSB `Scene`s.
//!
//! Mapping policy (import):
//! - Build the directed navigation graph (navigations + transition `destination` / `origin`).
//! - Each **weakly connected component** → one SLSB `Scene` with a branching stage graph.
//! - Each OStim node (looping **or** transition) → one `Stage`.
//! - OStim `speeds[]` stay on that stage (default speed → anim event; extras in `ostim_speed:` tags).
//! - Transition nodes keep a single graph edge to their `destination`.
//!
//! Export:
//! - One OStim JSON per stage (`ostim_id` tag), with `destination` for transitions
//!   and `navigations` from the stage graph.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use log::warn;
use serde_json::Value;

use crate::project::define::{Node, Offset, Sex};
use crate::project::ostim::events::{
    derive_anim_base, ostim_actor_event, sanitize_ostim_id, animation_base_from_event,
};
use crate::project::ostim::mapping::{
    action_to_tags, infer_race_key, ostim_furniture_to_slsb, slsb_furniture_to_ostim, tags_to_actions,
};
use crate::project::position::Position;
use crate::project::position_info::PositionInfo;
use crate::project::scene::Scene;
use crate::project::stage::{Extra as StageExtra, Stage};
use crate::project::NanoID;

#[derive(Debug, Default)]
pub struct OstimImportSummary {
    /// SLSB scenes created (one per connected component).
    pub scenes_imported: usize,
    /// OStim JSON nodes folded into those scenes.
    pub nodes_grouped: usize,
    /// Of which were transition nodes.
    pub transitions_included: usize,
    pub files_read: usize,
    /// autoTransitions edges materialized into the stage graph.
    pub auto_transitions_linked: usize,
    /// autoTransitions destinations missing from the pack.
    pub auto_transitions_missing: usize,
}

#[derive(Debug, Clone)]
struct NavEdge {
    from: String,
    to: String,
    priority: i64,
    description: String,
    icon: String,
    border: String,
}

pub fn find_ostim_scenes_dir(path: &Path) -> Result<PathBuf, String> {
    if path.is_file() {
        return Err("Expected an OStim pack folder or scenes directory".into());
    }
    let candidates = [
        path.join("SKSE/Plugins/OStim/scenes"),
        path.join("SKSE/Plugins/OStim/Scenes"),
        path.to_path_buf(),
    ];
    for c in candidates {
        if c.is_dir() {
            return Ok(c);
        }
    }
    Err(format!(
        "Could not find OStim scenes under {}",
        path.display()
    ))
}

pub fn collect_scene_jsons(scenes_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    visit_jsons(scenes_dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn visit_jsons(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            visit_jsons(&path, out)?;
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
    Ok(())
}

pub fn scene_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("scene")
        .to_string()
}

pub fn parse_ostim_file(path: &Path) -> Result<(String, Value), String> {
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let value: Value =
        serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    Ok((scene_id_from_path(path), value))
}

pub fn is_transition(value: &Value) -> bool {
    value
        .get("destination")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Import OStim scenes grouped by navigation connected components.
pub fn import_ostim_scenes(
    pack_or_scenes: &Path,
) -> Result<(String, String, IndexMap<NanoID, Scene>, OstimImportSummary), String> {
    let scenes_dir = find_ostim_scenes_dir(pack_or_scenes)?;
    let files = collect_scene_jsons(&scenes_dir)?;
    if files.is_empty() {
        return Err(format!("No OStim scene JSON files in {}", scenes_dir.display()));
    }

    let mut summary = OstimImportSummary {
        files_read: files.len(),
        ..Default::default()
    };
    let mut pack_name = String::new();
    let mut raw: IndexMap<String, Value> = IndexMap::new();

    for path in &files {
        let (ostim_id, value) = parse_ostim_file(path)?;
        if pack_name.is_empty() {
            if let Some(mp) = value.get("modpack").and_then(|v| v.as_str()) {
                pack_name = mp.trim().to_string();
            }
        }
        warn_dropped_ostim_fields(&ostim_id, &value);
        raw.insert(ostim_id, value);
    }

    let mut edges = collect_edges(&raw);
    let auto_stats = append_auto_transition_edges(&raw, &mut edges);
    summary.auto_transitions_linked = auto_stats.0;
    summary.auto_transitions_missing = auto_stats.1;
    if summary.auto_transitions_missing > 0 {
        warn!(
            "OStim import: {} autoTransition destination(s) missing from pack",
            summary.auto_transitions_missing
        );
    }
    let components = connected_components(raw.keys().cloned().collect(), &edges);
    let mut scenes = IndexMap::new();

    for (ci, component) in components.into_iter().enumerate() {
        let scene = component_to_scene(&component, &raw, &edges, ci)?;
        summary.nodes_grouped += component.len();
        summary.transitions_included += component
            .iter()
            .filter(|id| raw.get(*id).map(is_transition).unwrap_or(false))
            .count();
        scenes.insert(scene.id.clone(), scene);
        summary.scenes_imported += 1;
    }

    if scenes.is_empty() {
        return Err("No OStim scenes found".into());
    }
    if pack_name.is_empty() {
        pack_name = pack_or_scenes
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("OStimPack")
            .to_string();
    }
    pack_name = sanitize_ostim_id(&pack_name, "OStimPack");

    Ok((pack_name, String::new(), scenes, summary))
}

fn collect_edges(raw: &IndexMap<String, Value>) -> Vec<NavEdge> {
    let mut edges = Vec::new();
    for (sid, value) in raw {
        if let Some(dest) = value.get("destination").and_then(|v| v.as_str()) {
            if !dest.is_empty() {
                edges.push(NavEdge {
                    from: sid.clone(),
                    to: dest.to_string(),
                    priority: 0,
                    description: String::new(),
                    icon: String::new(),
                    border: String::new(),
                });
            }
            // Transition authored with origin: edge origin → this transition
            if let Some(origin) = value.get("origin").and_then(|v| v.as_str()) {
                if !origin.is_empty() {
                    edges.push(NavEdge {
                        from: origin.to_string(),
                        to: sid.clone(),
                        priority: value
                            .get("priority")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0),
                        description: value
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim_start_matches('$')
                            .to_string(),
                        icon: value
                            .get("icon")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        border: value
                            .get("border")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    });
                }
            }
        }
        if let Some(navs) = value.get("navigations").and_then(|v| v.as_array()) {
            for nav in navs {
                let prio = nav.get("priority").and_then(|v| v.as_i64()).unwrap_or(0);
                let desc = nav
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim_start_matches('$')
                    .to_string();
                let icon = nav
                    .get("icon")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let border = nav
                    .get("border")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some(dest) = nav.get("destination").and_then(|v| v.as_str()) {
                    if !dest.is_empty() {
                        edges.push(NavEdge {
                            from: sid.clone(),
                            to: dest.to_string(),
                            priority: prio,
                            description: desc,
                            icon,
                            border,
                        });
                    }
                } else if let Some(origin) = nav.get("origin").and_then(|v| v.as_str()) {
                    // Nav option added to origin leading to this scene
                    if !origin.is_empty() {
                        edges.push(NavEdge {
                            from: origin.to_string(),
                            to: sid.clone(),
                            priority: prio,
                            description: desc,
                            icon,
                            border,
                        });
                    }
                }
            }
        }
    }
    edges
}

fn auto_transition_priority(kind: &str) -> i64 {
    match kind.trim().to_ascii_lowercase().as_str() {
        "climax" | "orgasm" => 3000,
        _ => 2000,
    }
}

fn push_auto_transition_map(
    from: &str,
    map: &serde_json::Map<String, Value>,
    id_set: &HashSet<&str>,
    edges: &mut Vec<NavEdge>,
    linked: &mut usize,
    missing: &mut usize,
) {
    for (kind, dest_v) in map {
        let Some(dest) = dest_v.as_str().filter(|s| !s.is_empty()) else {
            continue;
        };
        if !id_set.contains(dest) {
            *missing += 1;
            warn!(
                "OStim '{from}': autoTransitions.{kind} → '{dest}' not in pack"
            );
            continue;
        }
        let already = edges
            .iter()
            .any(|e| e.from == from && e.to == dest);
        if already {
            continue;
        }
        edges.push(NavEdge {
            from: from.to_string(),
            to: dest.to_string(),
            priority: auto_transition_priority(kind),
            description: kind.clone(),
            icon: String::new(),
            border: String::new(),
        });
        *linked += 1;
    }
}

/// Materialize actor/scene `autoTransitions` into nav edges. Returns (linked, missing).
fn append_auto_transition_edges(
    raw: &IndexMap<String, Value>,
    edges: &mut Vec<NavEdge>,
) -> (usize, usize) {
    let id_set: HashSet<&str> = raw.keys().map(|s| s.as_str()).collect();
    let mut linked = 0usize;
    let mut missing = 0usize;
    for (sid, value) in raw {
        if let Some(map) = value.get("autoTransitions").and_then(|v| v.as_object()) {
            push_auto_transition_map(sid, map, &id_set, edges, &mut linked, &mut missing);
        }
        if let Some(actors) = value.get("actors").and_then(|a| a.as_array()) {
            for actor in actors {
                if let Some(map) = actor.get("autoTransitions").and_then(|v| v.as_object()) {
                    push_auto_transition_map(sid, map, &id_set, edges, &mut linked, &mut missing);
                }
            }
        }
    }
    (linked, missing)
}

fn connected_components(ids: Vec<String>, edges: &[NavEdge]) -> Vec<Vec<String>> {
    let mut undirected: HashMap<String, HashSet<String>> = HashMap::new();
    for id in &ids {
        undirected.entry(id.clone()).or_default();
    }
    for e in edges {
        if undirected.contains_key(&e.from) && undirected.contains_key(&e.to) {
            undirected.get_mut(&e.from).unwrap().insert(e.to.clone());
            undirected.get_mut(&e.to).unwrap().insert(e.from.clone());
        }
    }

    let mut visited = HashSet::new();
    let mut components = Vec::new();
    let mut sorted_ids = ids;
    sorted_ids.sort();
    for start in sorted_ids {
        if !visited.insert(start.clone()) {
            continue;
        }
        let mut comp = Vec::new();
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(u) = q.pop_front() {
            comp.push(u.clone());
            if let Some(neis) = undirected.get(&u) {
                let mut neis: Vec<_> = neis.iter().cloned().collect();
                neis.sort();
                for v in neis {
                    if visited.insert(v.clone()) {
                        q.push_back(v);
                    }
                }
            }
        }
        comp.sort();
        components.push(comp);
    }
    components.sort_by_key(|c| std::cmp::Reverse(c.len()));
    components
}

fn component_to_scene(
    component: &[String],
    raw: &IndexMap<String, Value>,
    edges: &[NavEdge],
    index: usize,
) -> Result<Scene, String> {
    let mut scene = Scene::default();
    let id_set: HashSet<&str> = component.iter().map(|s| s.as_str()).collect();

    let mut stage_for_ostim: HashMap<String, NanoID> = HashMap::new();
    let mut stages: Vec<Stage> = Vec::new();

    for (i, ostim_id) in component.iter().enumerate() {
        let value = raw
            .get(ostim_id)
            .ok_or_else(|| format!("Missing OStim node {ostim_id}"))?;
        let stage = ostim_node_to_stage(ostim_id, value, i)?;
        stage_for_ostim.insert(ostim_id.clone(), stage.id.clone());
        stages.push(stage);
    }

    // Leave all nodes at the default (40,40). App.jsx detects stacked coords and
    // applies computeLayeredPositions so branching OStim graphs don't form a line.
    let mut graph: HashMap<NanoID, Node> = HashMap::new();
    for stage in &stages {
        graph.insert(stage.id.clone(), Node::default());
    }

    let mut nav_text_parts: HashMap<NanoID, Vec<String>> = HashMap::new();
    for edge in edges {
        if !id_set.contains(edge.from.as_str()) || !id_set.contains(edge.to.as_str()) {
            continue;
        }
        let Some(from_id) = stage_for_ostim.get(&edge.from) else {
            continue;
        };
        let Some(to_id) = stage_for_ostim.get(&edge.to) else {
            continue;
        };
        if let Some(node) = graph.get_mut(from_id) {
            if !node.dest.iter().any(|d| d == to_id) {
                node.dest.push(to_id.clone());
            }
        }
        // Encode full nav UX for export round-trip
        let mut enc = format!("{}:{}:{}", edge.priority, edge.to, edge.description);
        if !edge.icon.is_empty() || !edge.border.is_empty() {
            enc.push_str(&format!(":{}:{}", edge.icon, edge.border));
        }
        nav_text_parts
            .entry(from_id.clone())
            .or_default()
            .push(enc);
    }

    for stage in &mut stages {
        if let Some(parts) = nav_text_parts.get(&stage.id) {
            stage.extra.nav_text = parts.join(";");
        }
    }

    disambiguate_duplicate_stage_names(&mut stages);

    // Root: prefer looping idle, else first looping, else first node
    let root_ostim = pick_root(component, raw);
    scene.root = stage_for_ostim
        .get(&root_ostim)
        .cloned()
        .unwrap_or_else(|| stages[0].id.clone());

    let root_value = raw.get(&root_ostim);
    let root_name = root_value
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or(&root_ostim)
        .trim_start_matches('$');
    scene.name = if component.len() == 1 {
        root_name.to_string()
    } else {
        format!("{root_name} [{} nodes]", component.len())
    };

    // Furniture: majority among looping nodes
    let mut furn_counts: HashMap<String, usize> = HashMap::new();
    for id in component {
        let Some(v) = raw.get(id) else { continue };
        if is_transition(v) {
            continue;
        }
        let f = v
            .get("furniture")
            .and_then(|x| x.as_str())
            .unwrap_or("none")
            .to_string();
        *furn_counts.entry(f).or_default() += 1;
    }
    let best_furn = furn_counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(f, _)| f)
        .unwrap_or_else(|| "none".into());
    let (furni_types, allow_bed) = ostim_furniture_to_slsb(&best_furn);
    scene.furniture.furni_types = furni_types;
    scene.furniture.allow_bed = allow_bed;
    if best_furn != "none" {
        scene.furniture.ostim_type = best_furn.clone();
    }
    if let Some(off) = root_value.and_then(|v| v.get("offset")) {
        scene.furniture.offset = read_offset(off);
    }

    // Scene tags: union of looping tags + group marker
    let mut tags = Vec::new();
    tags.push(format!("ostim_group:{index}"));
    if best_furn != "none" {
        tags.push(format!("ostim_furniture:{best_furn}"));
    }
    for id in component {
        let Some(v) = raw.get(id) else { continue };
        if is_transition(v) {
            continue;
        }
        if let Some(arr) = v.get("tags").and_then(|t| t.as_array()) {
            for t in arr {
                if let Some(s) = t.as_str() {
                    if !tags.iter().any(|x| x.eq_ignore_ascii_case(s)) {
                        tags.push(s.to_string());
                    }
                }
            }
        }
        if let Some(actions) = v.get("actions").and_then(|a| a.as_array()) {
            for action in actions {
                if let Some(ty) = action.get("type").and_then(|t| t.as_str()) {
                    for t in action_to_tags(ty) {
                        if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                            tags.push(t.to_string());
                        }
                    }
                    let actor = action.get("actor").and_then(|x| x.as_u64()).unwrap_or(0);
                    let target = action
                        .get("target")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(actor);
                    let performer = action
                        .get("performer")
                        .and_then(|x| x.as_u64())
                        .unwrap_or(actor);
                    let full = format!("action:{ty}:{actor}:{target}:{performer}");
                    if !tags.iter().any(|x| x == &full) {
                        tags.push(full);
                    }
                }
            }
        }
    }
    scene.tags = tags;

    // Climax: stages tagged climax / name contains Climax / leaf looping with climax tag
    for stage in &mut stages {
        let is_climax = stage.tags.iter().any(|t| {
            let l = t.to_ascii_lowercase();
            l == "climax" || l.contains("climax") || l == "orgasm"
        }) || stage.name.to_ascii_lowercase().contains("climax");
        if is_climax {
            for pos in &mut stage.positions {
                pos.climax = true;
                pos.extra.climax = true;
            }
        }
    }
    // Also: stages that are destinations of priority>=3000 navigations
    for edge in edges {
        if edge.priority < 3000 {
            continue;
        }
        if !id_set.contains(edge.to.as_str()) {
            continue;
        }
        if let Some(stage_id) = stage_for_ostim.get(&edge.to) {
            if let Some(stage) = stages.iter_mut().find(|s| &s.id == stage_id) {
                if !stage_is_transition(stage) {
                    for pos in &mut stage.positions {
                        pos.climax = true;
                        pos.extra.climax = true;
                    }
                }
            }
        }
    }

    scene.stages = stages;
    scene.graph = graph;

    // PositionInfo from root stage
    if let Some(root_stage) = scene.get_stage(&scene.root) {
        scene.positions = root_stage
            .positions
            .iter()
            .map(|p| PositionInfo {
                sex: p.sex.clone(),
                race: p.race.clone(),
                scale: p.scale,
                submissive: false,
                vampire: false,
                dead: false,
                add_cum: 0,
            })
            .collect();
    } else if let Some(first) = scene.stages.first() {
        scene.positions = first
            .positions
            .iter()
            .map(|p| PositionInfo {
                sex: p.sex.clone(),
                race: p.race.clone(),
                scale: p.scale,
                submissive: false,
                vampire: false,
                dead: false,
                add_cum: 0,
            })
            .collect();
    }

    Ok(scene)
}

fn pick_root(component: &[String], raw: &IndexMap<String, Value>) -> String {
    let looping: Vec<&String> = component
        .iter()
        .filter(|id| raw.get(*id).map(|v| !is_transition(v)).unwrap_or(false))
        .collect();
    for id in &looping {
        if let Some(tags) = raw.get(*id).and_then(|v| v.get("tags")).and_then(|t| t.as_array())
        {
            if tags
                .iter()
                .any(|t| t.as_str() == Some("idle"))
            {
                return (*id).clone();
            }
        }
    }
    if let Some(id) = looping.first() {
        return (*id).clone();
    }
    component[0].clone()
}

/// One OStim JSON node → one SLSB stage (default speed as event; extra speeds tagged).
fn ostim_node_to_stage(ostim_id: &str, value: &Value, layout_index: usize) -> Result<Stage, String> {
    let speeds = value
        .get("speeds")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("Scene '{ostim_id}' missing speeds"))?;
    if speeds.is_empty() {
        return Err(format!("Scene '{ostim_id}' has empty speeds"));
    }
    let actors = value
        .get("actors")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if actors.is_empty() {
        return Err(format!("Scene '{ostim_id}' has no actors"));
    }

    let default_speed = value
        .get("defaultSpeed")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;
    let default_speed = default_speed.min(speeds.len() - 1);

    let length_sec = value.get("length").and_then(|v| v.as_f64()).unwrap_or(0.0);
    let fixed_len_ms = if length_sec > 0.0 {
        (length_sec * 1000.0).round() as f32
    } else {
        0.0
    };

    let display_name = value
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(ostim_id)
        .trim_start_matches('$')
        .to_string();

    let mut tags: Vec<String> = value
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    tags.push(format!("ostim_id:{ostim_id}"));
    if is_transition(value) {
        if !tags.iter().any(|t| t.eq_ignore_ascii_case("transition")) {
            tags.push("transition".into());
        }
        if let Some(dest) = value.get("destination").and_then(|v| v.as_str()) {
            tags.push(format!("ostim_dest:{dest}"));
        }
    }

    if let Some(actions) = value.get("actions").and_then(|v| v.as_array()) {
        for action in actions {
            if let Some(ty) = action.get("type").and_then(|v| v.as_str()) {
                for t in action_to_tags(ty) {
                    if !tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                        tags.push(t.to_string());
                    }
                }
                let actor = action.get("actor").and_then(|x| x.as_u64()).unwrap_or(0);
                let target = action
                    .get("target")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(actor);
                let performer = action
                    .get("performer")
                    .and_then(|x| x.as_u64())
                    .unwrap_or(actor);
                let full = format!("action:{ty}:{actor}:{target}:{performer}");
                if !tags.iter().any(|x| x == &full) {
                    tags.push(full);
                }
            }
        }
    }

    for key in [
        "fadeOnEntry",
        "scaleOffsetWithFurniture",
        "noRandomSelection",
    ] {
        if let Some(v) = value.get(key) {
            let enc = if let Some(b) = v.as_bool() {
                b.to_string()
            } else {
                v.to_string()
            };
            tags.push(format!("ostim_{key}:{enc}"));
        }
    }

    // Extra speeds beyond default
    for (si, speed) in speeds.iter().enumerate() {
        if si == default_speed {
            continue;
        }
        let anim = speed
            .get("animation")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let pb = speed
            .get("playbackSpeed")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);
        let ds = speed
            .get("displaySpeed")
            .and_then(|v| v.as_f64())
            .unwrap_or((si + 1) as f64);
        tags.push(format!("ostim_speed:{anim}|{pb}|{ds}"));
    }

    let def = &speeds[default_speed];
    let animation = def
        .get("animation")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("Scene '{ostim_id}' default speed missing animation"))?;
    let playback = def
        .get("playbackSpeed")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let display = def
        .get("displaySpeed")
        .and_then(|v| v.as_f64())
        .unwrap_or((default_speed + 1) as f64);

    let scene_tag_hints: Vec<String> = value
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let mut stage = Stage {
        id: NanoID::new_nanoid(),
        name: format!("{display_name}|pb:{playback}|ds:{display}"),
        positions: Vec::with_capacity(actors.len()),
        tags,
        extra: StageExtra {
            fixed_len: if is_transition(value) {
                fixed_len_ms.max(1.0)
            } else {
                fixed_len_ms
            },
            nav_text: String::new(),
            sound: String::new(),
        },
    };
    let _ = layout_index;

    for (ai, actor) in actors.iter().enumerate() {
        let mut pos = Position::new(None);
        let anim_index = actor
            .get("animationIndex")
            .and_then(|v| v.as_u64())
            .unwrap_or(ai as u64) as usize;
        pos.event = vec![ostim_actor_event(animation, anim_index)];
        pos.offset = actor.get("offset").map(read_offset).unwrap_or_default();
        if let Some(sos) = actor.get("sosBend").and_then(|v| v.as_i64()) {
            pos.schlong = sos.clamp(i8::MIN as i64, i8::MAX as i64) as i8;
            pos.tags.push(format!("ostim_sos:{sos}"));
        }
        pos.sex = intended_sex_to_sex(actor.get("intendedSex").and_then(|v| v.as_str()));
        pos.race = infer_race_key(actor, &scene_tag_hints);
        pos.scale = actor
            .get("scale")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as f32;
        if let Some(atags) = actor.get("tags").and_then(|v| v.as_array()) {
            for t in atags.iter().filter_map(|t| t.as_str()) {
                if !pos.tags.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                    pos.tags.push(t.to_string());
                }
            }
        }
        if actor.get("noStrip").and_then(|v| v.as_bool()) == Some(true) {
            pos.strip_data = crate::project::define::Stripping::nothing();
        }
        if let Some(v) = actor.get("lookUp").and_then(|v| v.as_i64()) {
            pos.look_up = v.clamp(-100, 100) as i32;
        } else if let Some(v) = actor.get("lookDown").and_then(|v| v.as_i64()) {
            pos.look_up = (-v).clamp(-100, 100) as i32;
        }
        if pos.look_up != 0 {
            pos.tags.push(format!("ostim_lookUp:{}", pos.look_up));
        }
        if let Some(v) = actor.get("lookLeft").and_then(|v| v.as_i64()) {
            pos.look_left = v.clamp(-100, 100) as i32;
        } else if let Some(v) = actor.get("lookRight").and_then(|v| v.as_i64()) {
            pos.look_left = (-v).clamp(-100, 100) as i32;
        }
        if pos.look_left != 0 {
            pos.tags.push(format!("ostim_lookLeft:{}", pos.look_left));
        }
        if let Some(idx) = actor.get("animationIndex").and_then(|v| v.as_u64()) {
            if idx as usize != ai {
                pos.animation_index = Some(idx as u32);
                pos.tags.push(format!("ostim_animIndex:{idx}"));
            }
        }
        if let Some(expr) = actor
            .get("expressionOverride")
            .and_then(|v| v.as_str())
        {
            pos.expression_override = expr.to_string();
            if !expr.is_empty() {
                pos.tags.push(format!("ostim_expr:{expr}"));
            }
        }
        if let Some(eq) = actor.get("equipObjects").and_then(|v| v.as_object()) {
            let names: Vec<String> = eq
                .iter()
                .filter(|(_, v)| v.as_bool() == Some(true))
                .map(|(k, _)| k.clone())
                .collect();
            if !names.is_empty() {
                pos.equip_objects = names.join(" ");
                pos.anim_obj = names.join(" ");
                for name in &names {
                    pos.tags.push(format!("ostim_equip:{name}"));
                }
            }
        }
        if actor.get("feetOnGround").and_then(|v| v.as_bool()) == Some(true) {
            pos.tags.push("ostim_feetOnGround:true".into());
        }
        stage.positions.push(pos);
    }

    Ok(stage)
}

#[cfg(test)]
pub fn ostim_json_to_scene(ostim_id: &str, value: &Value) -> Result<Scene, String> {
    let mut raw = IndexMap::new();
    raw.insert(ostim_id.to_string(), value.clone());
    let edges = collect_edges(&raw);
    component_to_scene(&[ostim_id.to_string()], &raw, &edges, 0)
}

fn intended_sex_to_sex(s: Option<&str>) -> Sex {
    match s.map(|x| x.to_ascii_lowercase()).as_deref() {
        Some("female") => Sex {
            male: false,
            female: true,
            futa: false,
        },
        Some("male") => Sex {
            male: true,
            female: false,
            futa: false,
        },
        _ => Sex {
            male: true,
            female: true,
            futa: false,
        },
    }
}

fn sex_to_intended(sex: &Sex) -> Option<&'static str> {
    if sex.female && !sex.male {
        Some("female")
    } else if sex.male && !sex.female {
        Some("male")
    } else {
        None
    }
}

fn read_offset(v: &Value) -> Offset {
    Offset {
        x: v.get("x").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        y: v.get("y").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        z: v.get("z").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
        r: v.get("r").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
    }
}

fn write_offset(off: &Offset) -> Value {
    serde_json::json!({ "x": off.x, "y": off.y, "z": off.z, "r": off.r })
}

fn parse_playback_from_stage_name(name: &str) -> (f64, f64) {
    let mut pb = 1.0;
    let mut ds = 1.0;
    for part in name.split('|') {
        if let Some(rest) = part.strip_prefix("pb:") {
            if let Ok(v) = rest.parse::<f64>() {
                pb = v;
            }
        } else if let Some(rest) = part.strip_prefix("ds:") {
            if let Ok(v) = rest.parse::<f64>() {
                ds = v;
            }
        }
    }
    (pb, ds)
}

fn stage_ostim_id(stage: &Stage) -> Option<String> {
    stage
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("ostim_id:").map(|s| s.to_string()))
}

/// OStim often reuses the same display name for different transition clips
/// (e.g. forward vs reverse). Append destination / id so the graph is readable.
fn disambiguate_duplicate_stage_names(stages: &mut [Stage]) {
    let mut indexes_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, stage) in stages.iter().enumerate() {
        indexes_by_name
            .entry(stage.name.clone())
            .or_default()
            .push(i);
    }
    for idxs in indexes_by_name.values() {
        if idxs.len() < 2 {
            continue;
        }
        for &i in idxs {
            let dest = stages[i]
                .tags
                .iter()
                .find_map(|t| t.strip_prefix("ostim_dest:"))
                .map(|s| s.to_string());
            let oid = stage_ostim_id(&stages[i]);
            let suffix = dest.or(oid);
            if let Some(suf) = suffix {
                stages[i].name = format!("{} [{}]", stages[i].name, suf);
            }
        }
    }
}

fn stage_is_transition(stage: &Stage) -> bool {
    stage
        .tags
        .iter()
        .any(|t| t.eq_ignore_ascii_case("transition"))
        || stage
            .tags
            .iter()
            .any(|t| t.starts_with("ostim_dest:"))
}

fn extract_ostim_id(scene: &Scene) -> String {
    if let Some(id) = scene
        .stages
        .iter()
        .find_map(|s| stage_ostim_id(s))
    {
        return sanitize_ostim_id(&id, &scene.id.0);
    }
    for tag in &scene.tags {
        if let Some(id) = tag.strip_prefix("ostim_id:") {
            return sanitize_ostim_id(id, &scene.id.0);
        }
    }
    let fallback = if scene.name.is_empty() {
        format!("Scene_{}", scene.id.0)
    } else {
        scene.name.clone()
    };
    let events: Vec<&str> = scene
        .stages
        .iter()
        .flat_map(|s| s.positions.iter())
        .filter_map(|p| p.event.first().map(|e| e.as_str()))
        .collect();
    let base = derive_anim_base(events, &sanitize_ostim_id(&fallback, &scene.id.0));
    sanitize_ostim_id(&base, &scene.id.0)
}

fn linear_stages(scene: &Scene) -> Result<Vec<&Stage>, String> {
    if scene.stages.is_empty() {
        return Err(format!("Scene '{}' has no stages", scene.name));
    }
    let branching = scene.graph.values().any(|n| n.dest.len() > 1);
    if branching {
        return Err("branching".into());
    }
    let mut ordered = Vec::new();
    let mut current = Some(scene.root.clone());
    let mut seen = HashSet::new();
    while let Some(id) = current {
        if !seen.insert(id.clone()) {
            break;
        }
        let stage = scene
            .stages
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("Missing stage {}", id.0))?;
        ordered.push(stage);
        current = scene
            .graph
            .get(&id)
            .and_then(|n| n.dest.first().cloned());
    }
    if ordered.len() != scene.stages.len() {
        return Err("disconnected".into());
    }
    Ok(ordered)
}

fn stages_share_anim_base(stages: &[&Stage]) -> Option<String> {
    let mut base: Option<String> = None;
    for stage in stages {
        for pos in &stage.positions {
            let Some(event) = pos.event.first() else {
                continue;
            };
            let Some(id) = animation_base_from_event(event) else {
                return None;
            };
            match &base {
                None => base = Some(id),
                Some(existing) if existing == &id => {}
                Some(_) => return None,
            }
        }
    }
    base
}

/// Export one SLSB scene to one or more OStim JSON documents: (scene_id, json).
pub fn scene_to_ostim_files(scene: &Scene, modpack: &str) -> Result<Vec<(String, Value)>, String> {
    if scene.has_warnings || scene.stages.is_empty() {
        return Ok(vec![]);
    }

    // Grouped OStim import: every stage has ostim_id → one JSON per stage (preserves graph)
    let all_have_ostim_id = scene.stages.iter().all(|s| stage_ostim_id(s).is_some());
    if all_have_ostim_id || scene.graph.values().any(|n| n.dest.len() > 1) {
        let stages: Vec<&Stage> = scene.stages.iter().collect();
        return export_stages_as_ostim_graph(scene, &stages, modpack);
    }

    match linear_stages(scene) {
        Ok(stages) => {
            if let Some(base) = stages_share_anim_base(&stages) {
                // Classic SexLab intensity chain → one OStim scene with speeds
                let id = extract_ostim_id(scene);
                let json = build_looping_ostim(&id, scene, &stages, &base, modpack, true)?;
                Ok(vec![(id, json)])
            } else {
                export_stages_as_ostim_graph(scene, &stages, modpack)
            }
        }
        Err(_) => {
            let stages: Vec<&Stage> = scene.stages.iter().collect();
            export_stages_as_ostim_graph(scene, &stages, modpack)
        }
    }
}

fn export_stages_as_ostim_graph(
    scene: &Scene,
    stages: &[&Stage],
    modpack: &str,
) -> Result<Vec<(String, Value)>, String> {
    let mut out = Vec::new();
    let mut id_for_stage: HashMap<NanoID, String> = HashMap::new();

    for (i, stage) in stages.iter().enumerate() {
        let sid = stage_ostim_id(stage).unwrap_or_else(|| {
            let base = stage
                .positions
                .first()
                .and_then(|p| p.event.first())
                .and_then(|e| animation_base_from_event(e))
                .unwrap_or_else(|| format!("Stage_{}", i + 1));
            sanitize_ostim_id(&base, &stage.id.0)
        });
        let anim = stage
            .positions
            .first()
            .and_then(|p| p.event.first())
            .and_then(|e| animation_base_from_event(e))
            .map(|b| sanitize_ostim_id(&b, &sid))
            .unwrap_or_else(|| sid.clone());
        id_for_stage.insert(stage.id.clone(), sid.clone());

        let speeds = speeds_for_stage(stage, &anim);
        let mut json = build_ostim_json(&sid, scene, stage, speeds, modpack)?;

        if stage_is_transition(stage) {
            // Prefer explicit ostim_dest, else first graph dest
            let dest = stage
                .tags
                .iter()
                .find_map(|t| t.strip_prefix("ostim_dest:").map(|s| s.to_string()))
                .or_else(|| {
                    scene
                        .graph
                        .get(&stage.id)
                        .and_then(|n| n.dest.first())
                        .and_then(|d| id_for_stage.get(d).cloned())
                });
            if let Some(dest) = dest {
                json["destination"] = Value::String(dest);
                let mut tags = json
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .cloned()
                    .unwrap_or_default();
                if !tags.iter().any(|t| t.as_str() == Some("transition")) {
                    tags.push(Value::String("transition".into()));
                }
                json["tags"] = Value::Array(tags);
            }
        }

        out.push((sid, json));
    }

    // Second pass: ensure transition destinations resolve after all ids known
    for (sid, json) in &mut out {
        if json.get("destination").is_some() {
            continue;
        }
        let Some(stage) = stages.iter().find(|s| {
            stage_ostim_id(s).as_deref() == Some(sid.as_str())
                || (stage_ostim_id(s).is_none() && id_for_stage.get(&s.id).map(|x| x.as_str()) == Some(sid.as_str()))
        }) else {
            continue;
        };
        if !stage_is_transition(stage) {
            continue;
        }
        if let Some(dest_stage) = scene.graph.get(&stage.id).and_then(|n| n.dest.first()) {
            if let Some(dest_id) = id_for_stage.get(dest_stage) {
                json["destination"] = Value::String(dest_id.clone());
            }
        }
    }

    // Navigations for non-transition stages from graph + nav_text metadata
    for stage in stages {
        if stage_is_transition(stage) {
            continue;
        }
        let Some(from_id) = id_for_stage.get(&stage.id) else {
            continue;
        };
        let Some(node) = scene.graph.get(&stage.id) else {
            continue;
        };

        let mut navs_from_text = parse_nav_text(&stage.extra.nav_text);
        let mut used_dests = HashSet::new();
        let mut navs = Vec::new();

        for dest in &node.dest {
            let Some(to_id) = id_for_stage.get(dest) else {
                continue;
            };
            used_dests.insert(to_id.clone());
            if let Some(meta) = navs_from_text
                .iter()
                .find(|m| m.dest == *to_id)
                .cloned()
            {
                navs.push(nav_to_json(&meta));
            } else {
                let dest_climax = stages.iter().any(|s| {
                    id_for_stage.get(&s.id).map(|x| x.as_str()) == Some(to_id.as_str())
                        && s.positions.iter().any(|p| p.climax)
                });
                navs.push(serde_json::json!({
                    "destination": to_id,
                    "description": to_id,
                    "priority": if dest_climax { 3000 } else { 1000 },
                }));
            }
        }
        // Keep any nav_text entries whose dest wasn't in graph (external packs)
        for meta in navs_from_text.drain(..) {
            if used_dests.contains(&meta.dest) {
                continue;
            }
            navs.push(nav_to_json(&meta));
        }

        if navs.is_empty() {
            continue;
        }
        for (sid, json) in &mut out {
            if sid == from_id {
                json["navigations"] = Value::Array(navs);
                break;
            }
        }
    }

    Ok(out)
}

#[derive(Clone)]
struct NavMeta {
    dest: String,
    priority: i64,
    description: String,
    icon: String,
    border: String,
}

fn parse_nav_text(text: &str) -> Vec<NavMeta> {
    let mut out = Vec::new();
    if text.is_empty() {
        return out;
    }
    for part in text.split(';') {
        // prio:dest:desc[:icon:border]
        let bits: Vec<&str> = part.splitn(5, ':').collect();
        if bits.len() < 2 {
            continue;
        }
        let prio = bits[0].parse::<i64>().unwrap_or(0);
        let dest = bits[1].to_string();
        if dest.is_empty() {
            continue;
        }
        let description = bits.get(2).unwrap_or(&"").to_string();
        let icon = bits.get(3).unwrap_or(&"").to_string();
        let border = bits.get(4).unwrap_or(&"").to_string();
        out.push(NavMeta {
            dest,
            priority: prio,
            description,
            icon,
            border,
        });
    }
    out
}

fn nav_to_json(meta: &NavMeta) -> Value {
    let mut obj = serde_json::json!({
        "destination": meta.dest,
        "description": if meta.description.is_empty() { meta.dest.as_str() } else { meta.description.as_str() },
        "priority": meta.priority,
    });
    if !meta.icon.is_empty() {
        obj["icon"] = Value::String(meta.icon.clone());
    }
    if !meta.border.is_empty() {
        obj["border"] = Value::String(meta.border.clone());
    }
    obj
}

fn speeds_for_stage(stage: &Stage, default_anim: &str) -> Vec<Value> {
    let (pb, ds) = parse_playback_from_stage_name(&stage.name);
    let mut speeds = vec![serde_json::json!({
        "animation": default_anim,
        "playbackSpeed": pb,
        "displaySpeed": ds,
    })];
    for tag in &stage.tags {
        if let Some(rest) = tag.strip_prefix("ostim_speed:") {
            let parts: Vec<&str> = rest.split('|').collect();
            if parts.len() >= 3 {
                let anim = if parts[0].is_empty() {
                    default_anim
                } else {
                    parts[0]
                };
                let pb: f64 = parts[1].parse().unwrap_or(1.0);
                let ds: f64 = parts[2].parse().unwrap_or(1.0);
                speeds.push(serde_json::json!({
                    "animation": anim,
                    "playbackSpeed": pb,
                    "displaySpeed": ds,
                }));
            }
        }
    }
    speeds
}

fn build_looping_ostim(
    scene_id: &str,
    scene: &Scene,
    stages: &[&Stage],
    anim_base: &str,
    modpack: &str,
    encode_playback: bool,
) -> Result<Value, String> {
    let mut speeds = Vec::new();
    for (i, stage) in stages.iter().enumerate() {
        let (mut pb, mut ds) = parse_playback_from_stage_name(&stage.name);
        if !encode_playback || (pb == 1.0 && !stage.name.contains("pb:")) {
            pb = 1.0 + 0.2 * i as f64;
            ds = (i + 1) as f64;
        }
        speeds.push(serde_json::json!({
            "animation": anim_base,
            "playbackSpeed": pb,
            "displaySpeed": ds,
        }));
    }
    build_ostim_json(scene_id, scene, stages[0], speeds, modpack)
}

fn build_ostim_json(
    scene_id: &str,
    scene: &Scene,
    stage_for_actors: &Stage,
    speeds: Vec<Value>,
    modpack: &str,
) -> Result<Value, String> {
    let length_sec = if stage_for_actors.extra.fixed_len > 0.0 {
        stage_for_actors.extra.fixed_len / 1000.0
    } else {
        2.0
    };

    let display_name = stage_for_actors
        .name
        .split('|')
        .next()
        .filter(|s| !s.is_empty() && !s.starts_with("pb:"))
        .unwrap_or(scene_id);

    let mut actors = Vec::new();
    for (i, info) in scene.positions.iter().enumerate() {
        let pos = stage_for_actors.positions.get(i);
        let mut actor = serde_json::Map::new();
        if let Some(sex) = sex_to_intended(&info.sex) {
            actor.insert("intendedSex".into(), Value::String(sex.into()));
        }
        let schlong = pos.map(|p| p.schlong).unwrap_or(0);
        if schlong != 0 {
            actor.insert("sosBend".into(), serde_json::json!(schlong as i64));
        }
        if (info.scale - 1.0).abs() > f32::EPSILON {
            actor.insert("scale".into(), serde_json::json!(info.scale));
        }
        if let Some(p) = pos {
            if p.offset.x != 0.0 || p.offset.y != 0.0 || p.offset.z != 0.0 || p.offset.r != 0.0 {
                actor.insert("offset".into(), write_offset(&p.offset));
            }
            if !p.tags.is_empty() {
                actor.insert(
                    "tags".into(),
                    Value::Array(p.tags.iter().cloned().map(Value::String).collect()),
                );
            }
            if p.look_up != 0 {
                actor.insert("lookUp".into(), serde_json::json!(p.look_up));
            }
            if p.look_left != 0 {
                actor.insert("lookLeft".into(), serde_json::json!(p.look_left));
            }
            if let Some(idx) = p.animation_index {
                actor.insert("animationIndex".into(), serde_json::json!(idx));
            }
            if !p.expression_override.trim().is_empty() {
                actor.insert(
                    "expressionOverride".into(),
                    Value::String(p.expression_override.trim().to_string()),
                );
            }
            // Equip objects: prefer explicit field; fall back to anim_obj tokens as author hint
            let equip = if !p.equip_objects.trim().is_empty() {
                p.equip_objects.clone()
            } else if !p.anim_obj.trim().is_empty() {
                p.anim_obj.clone()
            } else {
                String::new()
            };
            if !equip.is_empty() {
                let mut map = serde_json::Map::new();
                for tok in equip.split(|c: char| c == ',' || c.is_whitespace()) {
                    let t = tok.trim();
                    if !t.is_empty() {
                        map.insert(t.to_string(), Value::Bool(true));
                    }
                }
                if !map.is_empty() {
                    actor.insert("equipObjects".into(), Value::Object(map));
                }
            }
            if p.strip_data.is_nothing() {
                actor.insert("noStrip".into(), Value::Bool(true));
            }
        }
        actors.push(Value::Object(actor));
    }

    let mut tags: Vec<String> = stage_for_actors
        .tags
        .iter()
        .filter(|t| {
            !t.starts_with("ostim_id:")
                && !t.starts_with("ostim_dest:")
                && !t.starts_with("ostim_speed:")
                && !t.starts_with("ostim_furniture:")
                && !t.starts_with("ostim_fadeOnEntry:")
                && !t.starts_with("ostim_scaleOffsetWithFurniture:")
                && !t.starts_with("ostim_noRandomSelection:")
                && !t.starts_with("nav:")
                && !t.starts_with("action:")
                && !t.starts_with("ostim_group:")
        })
        .cloned()
        .collect();
    if stage_for_actors.positions.iter().any(|p| p.climax)
        && !tags.iter().any(|t| t.eq_ignore_ascii_case("climax"))
    {
        tags.push("climax".into());
    }

    let mut combined_tags: Vec<String> = stage_for_actors
        .tags
        .iter()
        .chain(scene.tags.iter())
        .cloned()
        .collect();
    combined_tags.extend(tags.iter().cloned());
    let mut actions = tags_to_actions(&combined_tags, actors.len());
    if actions.is_empty() {
        actions = tags_to_actions(&tags, actors.len());
    }

    let furniture = if !scene.furniture.ostim_type.trim().is_empty() {
        scene.furniture.ostim_type.trim().to_ascii_lowercase()
    } else {
        slsb_furniture_to_ostim(&scene.furniture.furni_types, scene.furniture.allow_bed)
    };

    let mut root = serde_json::json!({
        "name": display_name,
        "modpack": modpack,
        "length": length_sec,
        "speeds": speeds,
        "actors": actors,
        "tags": tags,
        "actions": actions,
    });

    if furniture != "none" {
        root["furniture"] = Value::String(furniture);
    }
    let foff = &scene.furniture.offset;
    if foff.x != 0.0 || foff.y != 0.0 || foff.z != 0.0 || foff.r != 0.0 {
        root["offset"] = write_offset(foff);
    }

    let _ = scene_id;
    Ok(root)
}

/// Collect unique animations for animlist generation from exported JSON.
pub fn animations_from_ostim_json(json: &Value) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let oneshot = json
        .get("destination")
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false)
        || json
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|a| a.iter().any(|x| x.as_str() == Some("transition")))
            .unwrap_or(false);
    if let Some(speeds) = json.get("speeds").and_then(|v| v.as_array()) {
        for speed in speeds {
            if let Some(anim) = speed.get("animation").and_then(|v| v.as_str()) {
                if seen.insert(anim.to_string()) {
                    out.push((anim.to_string(), oneshot));
                }
            }
        }
    }
    out
}

pub fn warn_dropped_ostim_fields(ostim_id: &str, value: &Value) {
    // Fields we deliberately leave out of SLSB IR (no safe slot / unused by SexLab++).
    for key in ["compatScenes", "sourceSound", "hudIcon"] {
        if value.get(key).is_some() {
            warn!("OStim scene '{ostim_id}': field '{key}' not represented in SLSB IR");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_transitions_become_graph_edges() {
        let mut raw = IndexMap::new();
        raw.insert(
            "Sex".into(),
            serde_json::json!({
                "name": "Sex", "length": 2,
                "speeds": [{ "animation": "SexAnim", "playbackSpeed": 1, "displaySpeed": 1 }],
                "actors": [{
                    "intendedSex": "male",
                    "autoTransitions": { "climax": "Climax" }
                }, { "intendedSex": "female" }],
                "actions": [],
                "navigations": []
            }),
        );
        raw.insert(
            "Climax".into(),
            serde_json::json!({
                "name": "Climax", "length": 2,
                "tags": ["climax"],
                "speeds": [{ "animation": "ClimaxAnim" }],
                "actors": [{ "intendedSex": "male" }, { "intendedSex": "female" }],
                "actions": []
            }),
        );
        let edges = {
            let mut e = collect_edges(&raw);
            let _ = append_auto_transition_edges(&raw, &mut e);
            e
        };
        assert!(
            edges.iter().any(|e| e.from == "Sex" && e.to == "Climax" && e.priority == 3000),
            "expected climax autoTransition edge: {edges:?}"
        );
        let scene = component_to_scene(&["Climax".into(), "Sex".into()], &raw, &edges, 0).unwrap();
        let sex_stage = scene
            .stages
            .iter()
            .find(|s| s.tags.iter().any(|t| t == "ostim_id:Sex"))
            .unwrap();
        let climax_stage = scene
            .stages
            .iter()
            .find(|s| s.tags.iter().any(|t| t == "ostim_id:Climax"))
            .unwrap();
        assert!(scene
            .graph
            .get(&sex_stage.id)
            .unwrap()
            .dest
            .iter()
            .any(|d| d == &climax_stage.id));
    }

    #[test]
    fn single_node_still_imports() {
        let json: Value = serde_json::json!({
            "name": "Lovemaking: Bed Cowgirl",
            "modpack": "Moon Lovemaking Compendium",
            "length": 2,
            "speeds": [
                { "animation": "MLCBedCowgirl", "playbackSpeed": 1, "displaySpeed": 1 },
                { "animation": "MLCBedCowgirl", "playbackSpeed": 1.2, "displaySpeed": 2 }
            ],
            "furniture": "singlebed",
            "tags": ["cowgirl"],
            "actors": [
                { "intendedSex": "male", "sosBend": 6, "tags": ["lyingback"] },
                { "intendedSex": "female", "tags": ["kneeling"] }
            ],
            "actions": [
                { "type": "vaginalsex", "actor": 0, "target": 1, "performer": 1 }
            ],
            "navigations": [
                { "destination": "Other", "description": "Go", "priority": 1000 }
            ]
        });
        let scene = ostim_json_to_scene("MLCBedCowgirl", &json).unwrap();
        assert_eq!(scene.stages.len(), 1);
        assert!(scene.stages[0].tags.iter().any(|t| t.starts_with("ostim_speed:")));
        assert_eq!(scene.stages[0].positions[0].event[0], "MLCBedCowgirl_0");
        assert!(scene.stages[0].positions[0].tags.iter().any(|t| t == "ostim_sos:6"));
        assert!(scene.stages[0]
            .tags
            .iter()
            .any(|t| t == "action:vaginalsex:0:1:1"));
    }

    #[test]
    fn groups_connected_nav_graph() {
        let mut raw = IndexMap::new();
        raw.insert(
            "Idle".into(),
            serde_json::json!({
                "name": "Idle", "length": 2,
                "tags": ["idle"],
                "speeds": [{ "animation": "IdleAnim", "playbackSpeed": 1, "displaySpeed": 1 }],
                "actors": [{ "intendedSex": "male" }, { "intendedSex": "female" }],
                "actions": [],
                "navigations": [
                    { "destination": "GoSex", "description": "Start", "priority": 2000 }
                ]
            }),
        );
        raw.insert(
            "GoSex".into(),
            serde_json::json!({
                "name": "Go to Sex", "length": 1.5,
                "destination": "Sex",
                "tags": ["transition"],
                "speeds": [{ "animation": "GoSexAnim" }],
                "actors": [{ "intendedSex": "male" }, { "intendedSex": "female" }],
                "actions": []
            }),
        );
        raw.insert(
            "Sex".into(),
            serde_json::json!({
                "name": "Sex", "length": 2,
                "tags": ["cowgirl"],
                "speeds": [
                    { "animation": "SexAnim", "playbackSpeed": 1, "displaySpeed": 1 },
                    { "animation": "SexAnim", "playbackSpeed": 1.4, "displaySpeed": 2 }
                ],
                "actors": [{ "intendedSex": "male" }, { "intendedSex": "female" }],
                "actions": [{ "type": "vaginalsex", "actor": 0, "target": 1 }],
                "navigations": [
                    { "destination": "GoIdle", "description": "Return", "priority": -1000 }
                ]
            }),
        );
        raw.insert(
            "GoIdle".into(),
            serde_json::json!({
                "name": "Return", "length": 1,
                "destination": "Idle",
                "tags": ["transition"],
                "speeds": [{ "animation": "GoIdleAnim" }],
                "actors": [{ "intendedSex": "male" }, { "intendedSex": "female" }],
                "actions": []
            }),
        );
        // Unrelated singleton
        raw.insert(
            "Other".into(),
            serde_json::json!({
                "name": "Other", "length": 2,
                "speeds": [{ "animation": "OtherAnim" }],
                "actors": [{ "intendedSex": "female" }],
                "actions": []
            }),
        );

        let edges = collect_edges(&raw);
        let comps = connected_components(raw.keys().cloned().collect(), &edges);
        assert_eq!(comps.len(), 2);
        let big = comps.iter().find(|c| c.len() == 4).unwrap();
        let scene = component_to_scene(big, &raw, &edges, 0).unwrap();
        assert_eq!(scene.stages.len(), 4);
        assert!(scene.graph.values().any(|n| n.dest.len() >= 1));
        // Idle should branch to GoSex
        let idle_stage = scene
            .stages
            .iter()
            .find(|s| stage_ostim_id(s).as_deref() == Some("Idle"))
            .unwrap();
        let idle_node = scene.graph.get(&idle_stage.id).unwrap();
        assert_eq!(idle_node.dest.len(), 1);

        let files = scene_to_ostim_files(&scene, "Test").unwrap();
        assert_eq!(files.len(), 4);
        let go_sex = files.iter().find(|(id, _)| id == "GoSex").unwrap();
        assert_eq!(
            go_sex.1.get("destination").and_then(|v| v.as_str()),
            Some("Sex")
        );
        let idle = files.iter().find(|(id, _)| id == "Idle").unwrap();
        let navs = idle.1.get("navigations").and_then(|v| v.as_array()).unwrap();
        assert!(navs.iter().any(|n| n.get("destination").and_then(|d| d.as_str()) == Some("GoSex")));
    }

    #[test]
    fn imports_real_mlc_pack_grouped() {
        let root = PathBuf::from(
            "/mnt/Data/Coding/Animations/OStim/Lovemaking Compendium for OStim Standalone",
        );
        if !root.exists() {
            return;
        }
        let (pack_name, _, scenes, summary) = import_ostim_scenes(&root).unwrap();
        assert!(
            summary.scenes_imported < 50,
            "expected grouped components, got {} scenes",
            summary.scenes_imported
        );
        assert!(summary.nodes_grouped >= 300);
        assert!(summary.transitions_included > 100);
        assert!(!pack_name.is_empty());
        // Largest scene should be multi-stage with branching
        let largest = scenes.values().max_by_key(|s| s.stages.len()).unwrap();
        assert!(largest.stages.len() > 10);
        assert!(
            largest.graph.values().any(|n| n.dest.len() > 1),
            "expected branching in largest component"
        );
    }
}
