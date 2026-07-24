use log::info;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufReader, BufWriter, Write},
    mem::size_of,
    path::{Path, PathBuf},
    vec,
};
use tauri_plugin_dialog::DialogExt;

use crate::{
    project::{
        define::{Node, Sex},
        position::Position,
        serialize::{make_fnis_lines, make_fnis_lines_slal_sequence, map_race_to_folder},
        slanim_source::{objects_to_anim_obj, parse_slanim_source_file, SourceAnim},
    },
    racekeys::{map_legacy_to_racekey, map_racekey_to_legacy},
};

use super::{scene::Scene, serialize::EncodeBinary, stage::Stage, NanoID};

const VERSION: u8 = 4; // current version

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Slsb,
    Slal,
    Both,
}

#[derive(Debug, Default, Clone)]
pub struct EnrichSummary {
    pub files: usize,
    pub animations_in_source: usize,
    pub scenes_enriched: usize,
    pub positions_updated: usize,
    pub unmatched_ids: Vec<String>,
}

impl EnrichSummary {
    pub fn message(&self) -> String {
        let mut msg = format!(
            "Source files: {}\nAnimations in source: {}\nScenes enriched: {}\nPositions updated: {}",
            self.files,
            self.animations_in_source,
            self.scenes_enriched,
            self.positions_updated
        );
        if !self.unmatched_ids.is_empty() {
            let preview: Vec<_> = self
                .unmatched_ids
                .iter()
                .take(12)
                .cloned()
                .collect();
            msg.push_str(&format!(
                "\nUnmatched animation ids ({}): {}",
                self.unmatched_ids.len(),
                preview.join(", ")
            ));
            if self.unmatched_ids.len() > 12 {
                msg.push_str(", …");
            }
        }
        msg
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Package {
    #[serde(default)]
    pub version: u8,
    #[serde(skip)]
    pub pack_path: PathBuf,

    pub pack_name: String,
    pub pack_author: String,
    pub prefix_hash: NanoID,
    pub scenes: HashMap<NanoID, Scene>,
}

impl Package {
    pub fn new() -> Self {
        Self {
            version: VERSION, // current version
            pack_path: Default::default(),
            pack_name: Default::default(),
            pack_author: "Unknown".into(),
            prefix_hash: NanoID::new_prefix(),
            scenes: HashMap::new(),
        }
    }

    pub fn from_file(file: std::fs::File) -> Result<Package, String> {
        serde_json::from_reader(BufReader::new(file))
            .map_err(|e| e.to_string())
            .and_then(|mut package: Package| {
                if package.version < VERSION {
                    package.update_to_latest_version()?;
                }
                info!("Loaded project {}", package.pack_name);
                Ok(package)
            })
    }

    fn update_to_latest_version(&mut self) -> Result<(), String> {
        for (_, scene) in &mut self.scenes {
            if let Err(e) = scene.update_to_latest_version(self.version) {
                return Err(format!("Failed to update scene {}: {}", scene.id.0, e));
            }
        }
        self.version = VERSION;
        Ok(())
    }

    pub fn reset(&mut self) -> &Self {
        *self = Self::new();
        self
    }

    pub fn save_scene(&mut self, scene: Scene) -> &Scene {
        let id = scene.id.clone();
        info!("Saving or inserting Scene: {} / {}", id.0, scene.name);
        self.scenes.insert(id.clone(), scene);
        self.scenes.get(&id).unwrap()
    }

    pub fn discard_scene(&mut self, id: &NanoID) -> Option<Scene> {
        self.scenes.remove(id).map(|s| {
            info!("Deleting Scene: {} / {}", id.0, s.name);
            s
        })
    }

    pub fn get_scene(&self, id: &NanoID) -> Option<&Scene> {
        self.scenes.get(id)
    }

    pub fn get_scene_mut(&mut self, id: &NanoID) -> Option<&mut Scene> {
        self.scenes.get_mut(id)
    }

    pub fn get_stage(&self, id: &NanoID) -> Option<&Stage> {
        for (_, scene) in &self.scenes {
            let stage = scene.get_stage(id);
            if stage.is_some() {
                return stage;
            }
        }
        None
    }

    pub fn load_project(&mut self, app: &tauri::AppHandle) -> Result<(), String> {
        let path = app
            .dialog()
            .file()
            .add_filter("SexLab Project", &["slsb.json"])
            .blocking_pick_file()
            .ok_or("No path to load project from".to_string())?
            .into_path()
            .map_err(|e| e.to_string())?;
        *self = Package::from_file(fs::File::open(&path).map_err(|e| e.to_string())?)?;
        self.set_project_name_from_path(&path);
        self.pack_path = path.into();
        Ok(())
    }

    pub fn save_project(&mut self, save_as: bool, app: &tauri::AppHandle) -> Result<(), String> {
        let path = if save_as || !self.pack_path.exists() || self.pack_path.is_dir() {
            app.dialog()
                .file()
                .set_title("Save Project")
                .set_file_name(&self.pack_name)
                .add_filter("SexLab Project", &["slsb.json"])
                .blocking_save_file()
                .ok_or("No path to save project to".to_string())?
                .into_path()
                .map_err(|e| e.to_string())?
        } else {
            self.pack_path.clone()
        };

        self.set_project_name_from_path(&path);
        self.write(path)
    }

    pub fn write(&mut self, path: PathBuf) -> Result<(), String> {
        let file = fs::File::create(&path).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(BufWriter::new(file), self)
            .map_err(|e| e.to_string())?;
        self.pack_path = path;
        println!("Saved project {}", self.pack_name);
        Ok(())
    }

    pub fn load_slal(&mut self, app: &tauri::AppHandle) -> Result<(), String> {
        let path = app
            .dialog()
            .file()
            .set_title("Import SLAL")
            .add_filter("SLAL JSON", &["json"])
            .blocking_pick_file()
            .ok_or("No path to load slal file from".to_string())?
            .into_path()
            .map_err(|e| e.to_string())?;

        Package::from_slal(path).map(|prjct| *self = prjct)
    }

    pub fn enrich_from_slanim_source(
        &mut self,
        app: &tauri::AppHandle,
    ) -> Result<EnrichSummary, String> {
        let paths = app
            .dialog()
            .file()
            .set_title("Enrich from SLAnim source")
            .add_filter("SLAnim source", &["txt"])
            .blocking_pick_files()
            .ok_or("No SLAnim source file selected".to_string())?;

        let mut paths_buf = Vec::new();
        for p in paths {
            paths_buf.push(p.into_path().map_err(|e| e.to_string())?);
        }
        self.enrich_from_slanim_paths(&paths_buf)
    }

    pub fn enrich_from_slanim_paths(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<EnrichSummary, String> {
        let mut summary = EnrichSummary {
            files: paths.len(),
            ..Default::default()
        };
        let mut source_anims: Vec<SourceAnim> = Vec::new();
        for path in paths {
            let parsed = parse_slanim_source_file(path)?;
            summary.animations_in_source += parsed.len();
            source_anims.extend(parsed);
        }

        for anim in &source_anims {
            let mut matched = false;
            for scene in self.scenes.values_mut() {
                if !scene_matches_source(scene, anim) {
                    continue;
                }
                matched = true;
                let updated = apply_source_objects(scene, anim);
                if updated > 0 {
                    summary.scenes_enriched += 1;
                    summary.positions_updated += updated;
                }
            }
            if !matched {
                summary.unmatched_ids.push(anim.id.clone());
            }
        }
        Ok(summary)
    }

    pub fn from_slal(path: PathBuf) -> Result<Package, String> {
        let file = fs::File::open(&path).map_err(|e| e.to_string())?;

        let slal: serde_json::Value =
            serde_json::from_reader(BufReader::new(file)).map_err(|e| e.to_string())?;

        let mut prjct = Package::new();
        prjct.version = 0; // SLAL files are always version 0
        prjct.pack_name = slal["name"]
            .as_str()
            .ok_or("Missing name attribute")?
            .into();

        let anims = slal["animations"]
            .as_array()
            .ok_or("Missing animations attribute")?;
        for animation in anims {
            let mut scene = Scene::default();
            scene.name = animation["name"]
                .as_str()
                .ok_or("Missing name attribute")?
                .into();
            let crt_race = animation["creature_race"].as_str().unwrap_or_default();
            let actors = animation["actors"]
                .as_array()
                .ok_or("Missing actors attribute")?;

            // initialize stages and copy information for every position into the respective stage
            for (n, position) in actors.iter().enumerate() {
                let sex = position["type"].as_str().unwrap_or("male").to_lowercase();
                let events = position["stages"]
                    .as_array()
                    .ok_or("Missing stages attribute")?;
                let actor_add_cum = position
                    .get("add_cum")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;
                let mut actor_strap_on = false;

                if scene.stages.is_empty() {
                    for _ in 0..events.len() {
                        scene.stages.push(Stage::new(&scene));
                    }
                    if scene.stages.is_empty() {
                        return Err("Scene has no stages".into());
                    }
                    for stage in &mut scene.stages {
                        stage.positions = vec![Position::new(None); actors.len()];
                    }
                }
                for (i, evt) in events.iter().enumerate() {
                    let edit_position = &mut scene.stages[i].positions[n];
                    edit_position.event =
                        vec![evt["id"].as_str().ok_or("Missing id attribute")?.into()];
                    edit_position.add_cum = actor_add_cum;
                    // SLAL stage offsets (Skyrim: X=side, Y=forward, Z=up)
                    if let Some(v) = evt.get("side").and_then(|v| v.as_f64()) {
                        edit_position.offset.x = v as f32;
                    }
                    if let Some(v) = evt.get("forward").and_then(|v| v.as_f64()) {
                        edit_position.offset.y = v as f32;
                    }
                    if let Some(v) = evt.get("up").and_then(|v| v.as_f64()) {
                        edit_position.offset.z = v as f32;
                    }
                    if let Some(v) = evt.get("rotate").and_then(|v| v.as_f64()) {
                        edit_position.offset.r = v as f32;
                    }
                    if evt.get("open_mouth").and_then(|v| v.as_bool()) == Some(true) {
                        log::warn!(
                            "Dropping open_mouth on '{}' actor {} stage {} (not stored in SLSB)",
                            scene.name,
                            n + 1,
                            i + 1
                        );
                    }
                    if evt.get("silent").and_then(|v| v.as_bool()) == Some(true) {
                        log::warn!(
                            "Dropping silent on '{}' actor {} stage {} (not stored in SLSB)",
                            scene.name,
                            n + 1,
                            i + 1
                        );
                    }
                    if let Some(sos) = evt.get("sos").and_then(|v| v.as_i64()) {
                        // Persist in project JSON (not .slr); mirrors convert tracking
                        edit_position.schlong = sos.clamp(i8::MIN as i64, i8::MAX as i64) as i8;
                    }
                    // strap_on may be bool or string in the wild
                    if evt.get("strap_on").and_then(|v| v.as_bool()) == Some(true)
                        || evt.get("strap_on").and_then(|v| v.as_str()) == Some("True")
                        || evt.get("strap_on").and_then(|v| v.as_str()) == Some("true")
                    {
                        actor_strap_on = true;
                    }
                    match sex.as_str() {
                        "male" | "type" => {
                            edit_position.sex = Sex {
                                male: true,
                                female: false,
                                futa: false,
                            };
                            edit_position.race = "Human".into();
                        }
                        "female" => {
                            edit_position.sex = Sex {
                                male: false,
                                female: true,
                                futa: false,
                            };
                            edit_position.race = "Human".into();
                        }
                        "creaturemale" => {
                            edit_position.sex = Sex {
                                male: true,
                                female: false,
                                futa: false,
                            };
                            edit_position.race = map_legacy_to_racekey(
                                position["race"].as_str().unwrap_or(crt_race),
                            )?;
                        }
                        "creaturefemale" => {
                            edit_position.sex = Sex {
                                male: false,
                                female: true,
                                futa: false,
                            };
                            edit_position.race = map_legacy_to_racekey(
                                position["race"].as_str().unwrap_or(crt_race),
                            )?;
                        }
                        _ => {
                            return Err(format!("Unrecognized gender: {}", sex));
                        }
                    }
                }
                // slsb-convert.py: strap_on → sex.futa on that human slot
                if actor_strap_on {
                    for stage in &mut scene.stages {
                        if let Some(pos) = stage.positions.get_mut(n) {
                            pos.sex.futa = true;
                        }
                    }
                }
            }
            // finalize stage data, adding climax to last positions
            let tags = animation["tags"]
                .as_str()
                .and_then(|tags| {
                    let list = tags
                        .to_lowercase()
                        .split(',')
                        .map(|str| str.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>();
                    Some(list)
                })
                .unwrap_or_default();
            if animation.get("sound").and_then(|v| v.as_str()).is_some() {
                log::warn!(
                    "Dropping animation-level sound on '{}' (not stored in SLSB)",
                    scene.name
                );
            }
            let stage_extra = animation
                .get("stages")
                .or_else(|| animation.get("stage"))
                .and_then(|v| v.as_array());
            for (i, stage) in scene.stages.iter_mut().enumerate() {
                stage.tags = tags.clone();
                if let Some(extra_vec) = stage_extra {
                    for extra in extra_vec {
                        let n = extra["number"].as_i64().unwrap_or(-1);
                        // SLAL uses 1-based stage numbers; accept 0-based as a fallback
                        if n != (i as i64 + 1) && n != i as i64 {
                            continue;
                        }
                        // SLAL timer is seconds; SLSB / convert store milliseconds
                        let timer_sec = extra["timer"].as_f64().unwrap_or_default();
                        if timer_sec > 0.0 {
                            stage.extra.fixed_len = (timer_sec * 1000.0).round() as f32;
                        }
                        if extra.get("sound").and_then(|v| v.as_str()).is_some() {
                            log::warn!(
                                "Dropping per-stage sound on '{}' stage {} (not stored in SLSB)",
                                scene.name,
                                i + 1
                            );
                        }
                    }
                }
            }
            scene.tags = tags;
            // build graph
            scene.root = scene.stages[0].id.clone();
            let mut prev_id: Option<NanoID> = None;
            for stage in scene.stages.iter_mut().rev() {
                let mut value = Node::default();
                if let Some(id) = prev_id {
                    value.dest = vec![id];
                }
                scene.graph.insert(stage.id.clone(), value);
                prev_id = Some(stage.id.clone());
            }
            // Mark climax after graph build (and keep top-level field for IPC;
            // extra.climax is skip_serializing).
            if let Some(last) = scene.stages.last_mut() {
                for position in &mut last.positions {
                    position.climax = true;
                    position.extra.climax = true;
                }
            }
            // add to prjct
            prjct.scenes.insert(scene.id.clone(), scene);
        }
        println!(
            "Loaded {} Animations from {}",
            prjct.scenes.len(),
            path.to_str().unwrap_or_default()
        );
        prjct.update_to_latest_version()?;
        Ok(prjct)
    }

    pub fn export(&self, app: &tauri::AppHandle) -> Result<(), String> {
        self.export_as(app, ExportKind::Slsb)
    }

    pub fn export_as(&self, app: &tauri::AppHandle, kind: ExportKind) -> Result<(), String> {
        let path = app
            .dialog()
            .file()
            .set_title(match kind {
                ExportKind::Slsb => "Export SLSB",
                ExportKind::Slal => "Export SLAL",
                ExportKind::Both => "Export SLSB + SLAL",
            })
            .set_file_name(&self.pack_name)
            .blocking_pick_folder()
            .ok_or_else(|| "Export cancelled".to_string())?
            .into_path()
            .map_err(|e| e.to_string())?;

        match kind {
            ExportKind::Slsb => self.build(path).map_err(|e| e.to_string()),
            ExportKind::Slal => self.write_slal_pack(&path),
            ExportKind::Both => {
                // SLSB and SLAL FNIS list formats clash in the same tree
                self.build(path.join("SLSB")).map_err(|e| e.to_string())?;
                self.write_slal_pack(&path.join("SLAL"))
            }
        }
    }

    pub fn build(&self, root_dir: PathBuf) -> Result<(), std::io::Error> {
        println!("Compiling project {}", self.pack_name);
        self.write_pack_atomically(&root_dir, |staging| {
            self.write_binary_file(staging)?;
            self.write_fnis_files_slsb(staging)?;
            self.generate_behaviors(staging)?;
            Ok(())
        })?;
        info!(
            "Successfully compiled {}",
            root_dir.to_str().unwrap_or_default()
        );
        Ok(())
    }

    fn generate_behaviors(&self, root_dir: &PathBuf) -> Result<(), std::io::Error> {
        match crate::project::behavior_gen::generate_behaviors_under(root_dir) {
            Ok(paths) => {
                info!("Generated {} behavior file(s)", paths.len());
                Ok(())
            }
            Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())),
        }
    }

    /// Write pack contents into a sibling staging dir, then swap into `root_dir`
    /// so a mid-build failure does not leave a half-written destination.
    fn write_pack_atomically<F>(&self, root_dir: &PathBuf, write: F) -> Result<(), std::io::Error>
    where
        F: FnOnce(&PathBuf) -> Result<(), std::io::Error>,
    {
        let parent = root_dir.parent().unwrap_or_else(|| Path::new("."));
        let name = root_dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("pack");
        let staging = parent.join(format!(
            ".{name}.slsb-staging-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging)?;
        match write(&staging) {
            Ok(()) => {
                if root_dir.exists() {
                    fs::remove_dir_all(root_dir)?;
                }
                fs::rename(&staging, root_dir)?;
                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_dir_all(&staging);
                Err(e)
            }
        }
    }

    pub fn write_slal_pack(&self, root_dir: &PathBuf) -> Result<(), String> {
        self.write_pack_atomically(root_dir, |staging| {
            self.write_slal(staging).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e)
            })?;
            self.write_fnis_files_slal(staging).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e)
            })?;
            self.generate_behaviors(staging)?;
            Ok(())
        })
        .map_err(|e| e.to_string())
    }

    pub fn write_slal(&self, root_dir: &PathBuf) -> Result<(), String> {
        let branching = self.branching_scene_names();
        if !branching.is_empty() {
            return Err(format!(
                "SLAL export requires linear stage chains. Branching scenes: {}",
                branching.join(", ")
            ));
        }

        let animations = self
            .scenes
            .values()
            .filter(|s| !s.has_warnings && !s.stages.is_empty())
            .map(|scene| scene_to_slal_animation(scene))
            .collect::<Result<Vec<_>, _>>()?;

        let pack_name = if self.pack_name.is_empty() {
            self.prefix_hash.0.clone()
        } else {
            self.pack_name.clone()
        };

        let root = serde_json::json!({
            "name": pack_name,
            "animations": animations,
        });

        let target_dir = root_dir.join("SLAnims").join("json");
        fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
        let out_path = target_dir.join(format!("{}.json", pack_name));
        let file = fs::File::create(&out_path).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(BufWriter::new(file), &root).map_err(|e| e.to_string())?;
        info!("Wrote SLAL JSON to {}", out_path.display());
        Ok(())
    }

    pub fn branching_scene_names(&self) -> Vec<String> {
        self.scenes
            .values()
            .filter(|scene| scene.graph.values().any(|node| node.dest.len() > 1))
            .map(|scene| {
                if scene.name.is_empty() {
                    scene.id.0.clone()
                } else {
                    scene.name.clone()
                }
            })
            .collect()
    }

    pub fn import_offset(&mut self, app: &tauri::AppHandle) -> Result<(), String> {
        let path = app
            .dialog()
            .file()
            .set_title("Import Offsets")
            .add_filter("Offset File", &["yaml", "yml"])
            .blocking_pick_file()
            .ok_or("No path to load offsets from".to_string())?
            .into_path()
            .map_err(|e| e.to_string())?;
        let file = fs::File::open(&path).map_err(|e| e.to_string())?;
        let offsetfile: serde_yaml::Mapping =
            serde_yaml::from_reader(BufReader::new(file)).map_err(|e| e.to_string())?;

        for (scene_id_v, stages_v) in offsetfile {
            if !stages_v.is_mapping() {
                continue;
            }
            let scene_id = scene_id_v
                .as_str()
                .ok_or("Not a valid offset file, expected string for scene id".to_string())?
                .to_string();
            if let Some(scene) = self.get_scene_mut(&NanoID(scene_id.clone())) {
                scene.import_offset(
                    stages_v
                        .as_mapping()
                        .ok_or(format!("Expected mapping in scene {}", scene_id))?,
                )?;
            }
        }

        Ok(())
    }

    fn set_project_name_from_path(&mut self, path: &PathBuf) -> () {
        self.pack_name = String::from(
            path.file_name() // ...\\{project.slsb.json}
                .and_then(|name| name.to_str())
                .and_then(|str| {
                    let ret = &str[0..str.find(".slsb.json").unwrap_or(str.len())];
                    Some(ret)
                })
                .unwrap_or_default(),
        );
    }

    fn write_binary_file(&self, root_dir: &PathBuf) -> Result<(), std::io::Error> {
        let target_dir = root_dir.join("SKSE\\SexLab\\Registry\\");
        let project_name = format!(
            "{}.slr",
            if self.pack_name.is_empty() {
                &self.prefix_hash.0
            } else {
                &self.pack_name
            }
        );
        let mut buf: Vec<u8> = Vec::new();
        buf.reserve(self.get_byte_size());
        info!(
            "Writing binary file for project {} with size {} at {}",
            project_name,
            buf.capacity(),
            target_dir.to_str().unwrap_or("Unknown path")
        );
        self.write_byte(&mut buf);
        fs::create_dir_all(&target_dir)?;
        fs::File::create(target_dir.join(project_name))?.write(&buf)?;
        Ok(())
    }

    fn write_fnis_files_slsb(&self, root_dir: &PathBuf) -> Result<(), std::io::Error> {
        let mut events: HashMap<&str, Vec<String>> = HashMap::new(); // map<RaceKey, Lines[]>
        let mut control: HashSet<&str> = HashSet::from(["__BLANK__", "__DEFAULT__"]);
        for (_, scene) in &self.scenes {
            if scene.has_warnings {
                continue;
            }
            assert_eq!(
                scene
                    .stages
                    .first()
                    .expect(&format!("Scene {} has 0 Stages", scene.id.0))
                    .positions
                    .len(),
                scene.positions.len()
            );
            for stage in &scene.stages {
                for i in 0..stage.positions.len() {
                    let stage_position = &stage.positions[i];
                    let scene_position = &scene.positions[i];
                    let event = &stage_position.event[0];
                    if control.contains(event.as_str()) {
                        continue;
                    }
                    control.insert(event);
                    let lines = make_fnis_lines(
                        &stage_position.event,
                        &self.prefix_hash.0,
                        stage.extra.fixed_len > 0.0,
                        &split_anim_objs(&stage_position.anim_obj),
                    );
                    insert_fnis_race_lines(&mut events, scene_position.race.as_str(), lines);
                }
            }
        }
        flush_fnis_lists(root_dir, &self.pack_name, &events, false)
    }

    fn write_fnis_files_slal(&self, root_dir: &PathBuf) -> Result<(), String> {
        let branching = self.branching_scene_names();
        if !branching.is_empty() {
            return Err(format!(
                "SLAL FNIS export requires linear stage chains. Branching scenes: {}",
                branching.join(", ")
            ));
        }

        let mut events: HashMap<String, Vec<String>> = HashMap::new();
        for scene in self
            .scenes
            .values()
            .filter(|s| !s.has_warnings && !s.stages.is_empty())
        {
            let stages = linear_stage_order(scene)?;
            let anim_id = sanitize_anim_id(&scene.name, &format!("Scene_{}", scene.id.0));

            for actor_idx in 0..scene.positions.len() {
                let race = scene.positions[actor_idx].race.clone();
                let mut stage_rows: Vec<(String, Vec<String>, bool)> = Vec::new();
                for stage in &stages {
                    let pos = stage.positions.get(actor_idx).ok_or_else(|| {
                        format!(
                            "Scene '{}' missing position {} on a stage",
                            anim_id,
                            actor_idx + 1
                        )
                    })?;
                    let event = pos
                        .event
                        .first()
                        .cloned()
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            format!(
                                "Scene '{}' actor {} has an empty anim event",
                                anim_id,
                                actor_idx + 1
                            )
                        })?;
                    if event == "__BLANK__" || event == "__DEFAULT__" {
                        stage_rows.clear();
                        break;
                    }
                    stage_rows.push((
                        event,
                        split_anim_objs(&pos.anim_obj),
                        stage.extra.fixed_len > 0.0,
                    ));
                }
                if stage_rows.is_empty() {
                    continue;
                }
                let lines =
                    make_fnis_lines_slal_sequence(&anim_id, &stage_rows);
                insert_fnis_race_lines_owned(&mut events, race.as_str(), lines);
            }
        }

        let borrowed: HashMap<&str, Vec<String>> = events
            .iter()
            .map(|(k, v)| (k.as_str(), v.clone()))
            .collect();
        flush_fnis_lists(root_dir, &self.pack_name, &borrowed, true).map_err(|e| e.to_string())
    }
}

fn split_anim_objs(anim_obj: &str) -> Vec<String> {
    // Accept comma-separated (SLSB JSON) and space-separated (FNIS / paste) forms.
    anim_obj
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

fn fnis_race_aliases(race: &str) -> Vec<&str> {
    match race {
        "Canine" => vec!["Canine", "Dog", "Wolf"],
        "Dog" | "Wolf" => vec![race, "Canine"],
        "Chaurus" | "Chaurus Reaper" => vec!["Chaurus"],
        "Spider" | "Large Spider" | "Giant Spider" => vec!["Spider"],
        "Boar" | "Boar (Mounted)" | "Boar (Any)" => vec!["Boar (Any)"],
        _ => vec![race],
    }
}

fn insert_fnis_race_lines<'a>(
    events: &mut HashMap<&'a str, Vec<String>>,
    race: &'a str,
    lines: Vec<String>,
) {
    for key in fnis_race_aliases(race) {
        events
            .entry(key)
            .and_modify(|list| list.extend(lines.iter().cloned()))
            .or_insert_with(|| lines.clone());
    }
}

fn insert_fnis_race_lines_owned(
    events: &mut HashMap<String, Vec<String>>,
    race: &str,
    lines: Vec<String>,
) {
    for key in fnis_race_aliases(race) {
        events
            .entry(key.to_string())
            .and_modify(|list| list.extend(lines.iter().cloned()))
            .or_insert_with(|| lines.clone());
    }
}

fn flush_fnis_lists(
    root_dir: &PathBuf,
    pack_name: &str,
    events: &HashMap<&str, Vec<String>>,
    slal_header: bool,
) -> Result<(), std::io::Error> {
    info!("---------------------------------------------------------");
    for (racekey, anim_events) in events {
        let target_folder = map_race_to_folder(racekey)
            .expect(format!("Cannot find folder for RaceKey {}", racekey).as_str());
        let path = root_dir.join(format!(
            "meshes\\actors\\{}\\animations\\{}",
            target_folder, pack_name
        ));
        let crt = &target_folder[target_folder
            .find('\\')
            .and_then(|w| Some(w + 1))
            .unwrap_or(0)..];
        fs::create_dir_all(&path)?;

        let create = |file_path: PathBuf| -> Result<(), std::io::Error> {
            let name = file_path.to_str().unwrap_or("NONE").to_string();
            let file = fs::File::create(file_path)?;
            let mut file = BufWriter::new(file);
            if slal_header {
                writeln!(file, "Version V1.0")?;
                writeln!(file)?;
            }
            info!(
                "Adding {} lines to race {} |||||| file: {}",
                anim_events.len(),
                racekey,
                name
            );
            for anim_event in anim_events {
                writeln!(file, "{}", anim_event)?;
            }
            Ok(())
        };
        match crt {
            "character" => create(path.join(format!("FNIS_{}_List.txt", pack_name))),
            "canine" => match *racekey {
                "Canine" => create(path.join(format!("FNIS_{}_canine_List.txt", pack_name))),
                "Dog" => create(path.join(format!("FNIS_{}_dog_List.txt", pack_name))),
                _ => create(path.join(format!("FNIS_{}_wolf_List.txt", pack_name))),
            },
            _ => create(path.join(format!("FNIS_{}_{}_List.txt", pack_name, crt))),
        }?;
    }
    info!("---------------------------------------------------------");
    Ok(())
}

fn sanitize_anim_id(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn scene_matches_source(scene: &Scene, anim: &SourceAnim) -> bool {
    if !anim.name.is_empty() {
        if scene.name.eq_ignore_ascii_case(&anim.name) {
            return true;
        }
        // SLAnim name prefixes (e.g. "Billyy ") are applied in JSON display names
        if scene
            .name
            .to_ascii_lowercase()
            .ends_with(&anim.name.to_ascii_lowercase())
        {
            return true;
        }
    }
    let sanitized = sanitize_anim_id(&scene.name, "");
    if !sanitized.is_empty() && sanitized == anim.id {
        return true;
    }
    let prefix = format!("{}_A", anim.id);
    for stage in &scene.stages {
        for pos in &stage.positions {
            if pos
                .event
                .iter()
                .any(|e| e == &anim.id || e.starts_with(&prefix))
            {
                return true;
            }
        }
    }
    false
}

fn apply_source_objects(scene: &mut Scene, anim: &SourceAnim) -> usize {
    let mut updated = 0;
    let actor_count = scene
        .stages
        .first()
        .map(|s| s.positions.len())
        .unwrap_or(0);
    for actor_idx in 0..actor_count {
        let src_actor = actor_idx + 1;
        let default_obj = anim.actor_objects.get(&src_actor);
        let stage_map = anim.stage_objects.get(&src_actor);
        for (si, stage) in scene.stages.iter_mut().enumerate() {
            let stage_num = si + 1;
            let obj = stage_map
                .and_then(|m| m.get(&stage_num))
                .or(default_obj);
            let Some(obj) = obj else {
                continue;
            };
            if obj.is_empty() {
                continue;
            }
            let Some(pos) = stage.positions.get_mut(actor_idx) else {
                continue;
            };
            let converted = objects_to_anim_obj(obj);
            if pos.anim_obj != converted {
                pos.anim_obj = converted;
                updated += 1;
            }
        }
    }
    updated
}

fn linear_stage_order(scene: &Scene) -> Result<Vec<&Stage>, String> {
    let label = if scene.name.is_empty() {
        scene.id.0.as_str()
    } else {
        scene.name.as_str()
    };

    for node in scene.graph.values() {
        if node.dest.len() > 1 {
            return Err(format!(
                "Scene '{}' has branching stages (SLAL export requires a linear chain)",
                label
            ));
        }
    }

    if scene.graph.is_empty() {
        return Ok(scene.stages.iter().collect());
    }

    let mut ordered = Vec::new();
    let mut visited: HashSet<String> = HashSet::new();
    let mut current = Some(scene.root.clone());
    while let Some(id) = current {
        if !visited.insert(id.0.clone()) {
            return Err(format!("Scene '{}' has a cycle in its stage graph", label));
        }
        let stage = scene.get_stage(&id).ok_or_else(|| {
            format!(
                "Scene '{}' references missing stage '{}'",
                label, id.0
            )
        })?;
        ordered.push(stage);
        current = scene
            .graph
            .get(&id)
            .and_then(|n| n.dest.first().cloned());
    }

    if ordered.len() != scene.stages.len() {
        return Err(format!(
            "Scene '{}' is not a single linear chain (unreachable or disconnected stages)",
            label
        ));
    }
    Ok(ordered)
}

fn slal_actor_type(
    info: &super::position_info::PositionInfo,
) -> Result<(String, Option<String>, bool), String> {
    let is_human = info.race == "Human" || info.race.is_empty();
    let male = info.sex.male;
    let female = info.sex.female;
    let futa = info.sex.futa;

    // slsb-convert.py maps SLAL Male+strap_on onto male+futa (and often adds flexible
    // futa on humans). Do not treat futa as Female for the SLAL type string.
    if is_human {
        if male && !female {
            // Male-only, or Male+futa (convert strap_on / flexible-futa pattern)
            Ok(("Male".into(), None, futa))
        } else if female && !male {
            Ok(("Female".into(), None, false))
        } else if male && female {
            // Dual-allowed slot: legacy SLAL uses Male + strap_on
            Ok(("Male".into(), None, true))
        } else if futa {
            // Futa-only: SLAL has no futa type; Male+strap_on is the usual stand-in
            Ok(("Male".into(), None, true))
        } else {
            Err("Position has no sex assigned".into())
        }
    } else {
        let legacy = map_racekey_to_legacy(&info.race)?;
        if female && !male {
            Ok(("CreatureFemale".into(), Some(legacy), false))
        } else {
            Ok(("CreatureMale".into(), Some(legacy), false))
        }
    }
}

/// Prefer the shared `{id}_A#_S#` event prefix over a sanitized scene name.
fn derive_slal_anim_id(scene: &Scene, stages: &[&Stage]) -> String {
    let fallback = sanitize_anim_id(&scene.name, &format!("Scene_{}", scene.id.0));
    let mut derived: Option<String> = None;
    for stage in stages {
        for pos in &stage.positions {
            let Some(event) = pos.event.first().filter(|e| !e.is_empty()) else {
                continue;
            };
            let Some(id) = strip_actor_stage_suffix(event) else {
                return fallback;
            };
            match &derived {
                None => derived = Some(id),
                Some(existing) if existing == &id => {}
                Some(_) => return fallback,
            }
        }
    }
    derived.filter(|id| !id.is_empty()).unwrap_or(fallback)
}

fn strip_actor_stage_suffix(event: &str) -> Option<String> {
    // Match ..._A{n}_S{m} at end of event id
    let bytes = event.as_bytes();
    let mut i = bytes.len();
    // _S digits
    let mut saw_s = false;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
        saw_s = true;
    }
    if !saw_s || i < 2 || bytes[i - 1] != b'S' || bytes[i - 2] != b'_' {
        return None;
    }
    i -= 2;
    // _A digits
    let mut saw_a = false;
    while i > 0 && bytes[i - 1].is_ascii_digit() {
        i -= 1;
        saw_a = true;
    }
    if !saw_a || i < 2 || bytes[i - 1] != b'A' || bytes[i - 2] != b'_' {
        return None;
    }
    i -= 2;
    if i == 0 {
        return None;
    }
    Some(event[..i].to_string())
}

/// Best-effort add_cum bitfield from scene tags when not stored on the position.
fn infer_add_cum(tags: &[String], actor_type: &str) -> Option<i64> {
    if actor_type != "Female" && actor_type != "CreatureFemale" {
        return None;
    }
    let lower: Vec<String> = tags.iter().map(|t| t.to_ascii_lowercase()).collect();
    let has = |keys: &[&str]| lower.iter().any(|t| keys.iter().any(|k| t == k));
    let mut bits: i64 = 0;
    if has(&["vaginal", "creampie"]) {
        bits |= 1; // Vaginal
    }
    if has(&["oral", "blowjob", "facial"]) {
        bits |= 2; // Oral
    }
    if has(&["anal"]) {
        bits |= 4; // Anal
    }
    if bits == 0 {
        None
    } else {
        Some(bits)
    }
}

fn slal_timer_seconds(fixed_len_ms: f32) -> f32 {
    // SLSB UI and slsb-convert.py store milliseconds; SLAL uses seconds
    if fixed_len_ms <= 0.0 {
        0.0
    } else {
        fixed_len_ms / 1000.0
    }
}

fn scene_to_slal_animation(scene: &Scene) -> Result<serde_json::Value, String> {
    let stages = linear_stage_order(scene)?;
    let anim_id = derive_slal_anim_id(scene, &stages);
    let anim_name = if scene.name.is_empty() {
        anim_id.clone()
    } else {
        scene.name.clone()
    };

    let mut tag_set: Vec<String> = if !scene.tags.is_empty() {
        scene.tags.clone()
    } else {
        stages
            .first()
            .map(|s| s.tags.clone())
            .unwrap_or_default()
    };
    tag_set.retain(|t| !t.is_empty());
    let tags = tag_set.join(",");

    let mut actors = Vec::new();
    let mut creature_race: Option<String> = None;
    for (actor_idx, info) in scene.positions.iter().enumerate() {
        let (actor_type, race_legacy, strap_on) = slal_actor_type(info)?;
        if let Some(ref r) = race_legacy {
            if creature_race.is_none() {
                creature_race = Some(r.clone());
            }
        }

        let mut actor_stages = Vec::new();
        for stage in &stages {
            let pos = stage.positions.get(actor_idx).ok_or_else(|| {
                format!(
                    "Scene '{}' stage '{}' missing position {}",
                    anim_name, stage.id.0, actor_idx + 1
                )
            })?;
            let event_id = pos
                .event
                .first()
                .cloned()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    format!(
                        "{}_A{}_S{}",
                        anim_id,
                        actor_idx + 1,
                        actor_stages.len() + 1
                    )
                });

            let mut stage_obj = serde_json::json!({ "id": event_id });
            let off = &pos.offset;
            if off.x != 0.0 || off.y != 0.0 || off.z != 0.0 || off.r != 0.0 {
                // Match Skyrim Location: X=side, Y=forward, Z=up
                stage_obj["side"] = serde_json::json!(off.x);
                stage_obj["forward"] = serde_json::json!(off.y);
                stage_obj["up"] = serde_json::json!(off.z);
                stage_obj["rotate"] = serde_json::json!(off.r);
            }
            if strap_on {
                stage_obj["strap_on"] = serde_json::json!(true);
            }
            if pos.schlong != 0 {
                stage_obj["sos"] = serde_json::json!(pos.schlong as i64);
            }
            if pos.event.len() > 1 {
                log::warn!(
                    "SLAL export uses only the first anim event for {} position {}",
                    anim_name,
                    actor_idx + 1
                );
            }
            actor_stages.push(stage_obj);
        }

        let mut actor = serde_json::json!({
            "type": actor_type,
            "stages": actor_stages,
        });
        if let Some(race) = race_legacy {
            actor["race"] = serde_json::json!(race);
        }
        let add_cum = if info.add_cum != 0 {
            Some(info.add_cum as i64)
        } else {
            infer_add_cum(&tag_set, &actor_type)
        };
        if let Some(cum) = add_cum {
            actor["add_cum"] = serde_json::json!(cum);
        }
        actors.push(actor);
    }

    let mut stage_params = Vec::new();
    for (i, stage) in stages.iter().enumerate() {
        if stage.extra.fixed_len > 0.0 {
            stage_params.push(serde_json::json!({
                "number": i + 1,
                "timer": slal_timer_seconds(stage.extra.fixed_len),
            }));
        }
        if !stage.extra.nav_text.is_empty() {
            log::warn!(
                "Dropping nav_text on scene '{}' stage {} (not supported in SLAL)",
                anim_name,
                i + 1
            );
        }
    }

    let mut anim = serde_json::json!({
        "id": anim_id,
        "name": anim_name,
        "actors": actors,
    });
    if !tags.is_empty() {
        anim["tags"] = serde_json::json!(tags);
    }
    if let Some(race) = creature_race {
        anim["creature_race"] = serde_json::json!(race);
    }
    if !stage_params.is_empty() {
        anim["stages"] = serde_json::json!(stage_params);
    }
    if scene.private {
        log::warn!(
            "Scene '{}' is private; flag is not represented in SLAL JSON",
            anim_name
        );
    }
    Ok(anim)
}

impl EncodeBinary for Package {
    fn get_byte_size(&self) -> usize {
        self.version.get_byte_size()
            + self.pack_name.get_byte_size()
            + self.pack_author.get_byte_size()
            + self.prefix_hash.get_byte_size()
            + self
                .scenes
                .iter()
                .filter(|(_, scene)| !scene.has_warnings && !scene.stages.is_empty())
                .fold(size_of::<u64>(), |acc, (_, scene)| {
                    acc + scene.get_byte_size()
                })
    }

    fn write_byte(&self, buf: &mut Vec<u8>) -> () {
        self.version.write_byte(buf);
        self.pack_name.write_byte(buf);
        self.pack_author.write_byte(buf);
        self.prefix_hash.write_byte(buf);
        buf.extend_from_slice(&(self.scenes.len() as u64).to_be_bytes());
        self.scenes
            .iter()
            .filter(|(_, scene)| !scene.has_warnings && !scene.stages.is_empty())
            .for_each(|(_, scene)| scene.write_byte(buf));
    }
}

#[cfg(test)]
mod tests {
    use super::{infer_add_cum, strip_actor_stage_suffix};
    use crate::project::define::Sex;
    use crate::project::position_info::PositionInfo;

    #[test]
    fn strips_actor_stage_event_suffix() {
        assert_eq!(
            strip_actor_stage_suffix("B_B_CGGrind_A1_S1").as_deref(),
            Some("B_B_CGGrind")
        );
        assert_eq!(
            strip_actor_stage_suffix("B_B_CGGrind_A2_S5").as_deref(),
            Some("B_B_CGGrind")
        );
        assert_eq!(strip_actor_stage_suffix("NoSuffix"), None);
    }

    #[test]
    fn infers_vaginal_add_cum_for_female() {
        let tags = vec!["Creampie".into(), "Vaginal".into()];
        assert_eq!(infer_add_cum(&tags, "Female"), Some(1));
        assert_eq!(infer_add_cum(&tags, "Male"), None);
    }

    #[test]
    fn male_with_futa_exports_as_male_strap_on() {
        let info = PositionInfo {
            sex: Sex {
                male: true,
                female: false,
                futa: true,
            },
            ..Default::default()
        };
        let (ty, _, strap) = super::slal_actor_type(&info).unwrap();
        assert_eq!(ty, "Male");
        assert!(strap);
    }

    #[test]
    fn female_with_futa_exports_as_female() {
        let info = PositionInfo {
            sex: Sex {
                male: false,
                female: true,
                futa: true,
            },
            ..Default::default()
        };
        let (ty, _, strap) = super::slal_actor_type(&info).unwrap();
        assert_eq!(ty, "Female");
        assert!(!strap);
    }

    #[test]
    fn slal_timer_converts_ms_to_seconds() {
        assert!((super::slal_timer_seconds(5000.0) - 5.0).abs() < f32::EPSILON);
        assert_eq!(super::slal_timer_seconds(0.0), 0.0);
    }
}
