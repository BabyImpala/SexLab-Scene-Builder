//! Write a complete OStim pack tree from an SLSB `Package`.

use std::collections::HashSet;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use log::info;

use crate::project::ostim::convert::{
    animations_from_ostim_json, scene_to_ostim_files, stage_ostim_folder,
};
use crate::project::ostim::events::{
    copy_slsb_hkx_to_ostim, sanitize_ostim_id, strip_actor_stage_suffix, strip_ostim_actor_suffix,
};
use crate::project::ostim::nemesis::{write_ostim_animlist, OstimAnimEntry};
use crate::project::ostim::nemesis_gen::write_nemesis_patches;
use crate::project::package::Package;
use crate::project::progress::JobProgress;
use crate::project::scene::Scene;

#[derive(Debug, Default)]
pub struct OstimExportSummary {
    pub scenes_written: usize,
    pub json_files: usize,
    pub hkx_copied: usize,
    pub animlist: Option<PathBuf>,
    pub nemesis_dir: Option<PathBuf>,
    pub facial_copied: bool,
    pub nemesis_from_source: bool,
    pub nemesis_synthesized: bool,
    pub assets_written: usize,
    pub sound_copied: bool,
}

pub fn pack_folder_name(pack: &Package) -> String {
    sanitize_ostim_id(&pack.fnis_mod_name(), &pack.prefix_hash.0)
}

fn scene_export_folder(scene: &Scene, fallback_id: &str) -> String {
    scene
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("ostim_folder:"))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_id.to_string())
}

fn stage_export_folder(scene: &Scene, scene_id: &str, group_fallback: &str) -> String {
    if let Some(stage) = scene.stages.iter().find(|s| {
        s.tags
            .iter()
            .any(|t| t == &format!("ostim_id:{scene_id}"))
    }) {
        if let Some(folder) = stage_ostim_folder(stage) {
            return folder;
        }
    }
    scene_export_folder(scene, group_fallback)
}

/// Export OStim scenes + animlist + Nemesis under `root_dir/{Pack}/` or `root_dir`.
pub fn write_ostim_pack(
    pack: &Package,
    root_dir: &Path,
    hkx_source: Option<&Path>,
    progress: Option<&JobProgress<'_>>,
) -> Result<OstimExportSummary, String> {
    let pack_folder = pack_folder_name(pack);
    let pack_root = if root_dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s == pack_folder)
        .unwrap_or(false)
    {
        root_dir.to_path_buf()
    } else {
        root_dir.join(&pack_folder)
    };

    let scenes_root = pack_root
        .join("SKSE")
        .join("Plugins")
        .join("OStim")
        .join("scenes");
    fs::create_dir_all(&scenes_root).map_err(|e| e.to_string())?;

    let modpack = if pack.pack_name.trim().is_empty() {
        pack_folder.clone()
    } else {
        pack.pack_name.clone()
    };

    let mut summary = OstimExportSummary::default();
    let mut anim_entries: Vec<OstimAnimEntry> = Vec::new();
    let mut seen_anim = HashSet::new();

    let scene_list: Vec<_> = pack
        .scenes
        .values()
        .filter(|s| !s.has_warnings && !s.stages.is_empty())
        .collect();
    let total_scenes = scene_list.len() as u64;
    if let Some(p) = progress {
        p.update("Writing OStim scene JSON…", Some(0), Some(total_scenes.max(1)));
    }

    for (si, scene) in scene_list.into_iter().enumerate() {
        if let Some(p) = progress {
            p.update(
                &format!(
                    "Writing OStim scenes… ({}/{})",
                    si + 1,
                    total_scenes.max(1)
                ),
                Some((si + 1) as u64),
                Some(total_scenes.max(1)),
            );
        }
        let files = scene_to_ostim_files(scene, &modpack)?;
        if files.is_empty() {
            continue;
        }
        summary.scenes_written += 1;

        let group_fallback = files
            .first()
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| "Scene".into());

        for (scene_id, json) in files {
            let folder_name = stage_export_folder(scene, &scene_id, &group_fallback);
            let folder = scenes_root.join(&folder_name);
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let out_path = folder.join(format!("{scene_id}.json"));
            let file = fs::File::create(&out_path).map_err(|e| e.to_string())?;
            serde_json::to_writer_pretty(BufWriter::new(file), &json)
                .map_err(|e| e.to_string())?;
            summary.json_files += 1;

            let actor_count = scene.positions.len().max(1);
            for (animation, oneshot) in animations_from_ostim_json(&json) {
                if seen_anim.insert(animation.clone()) {
                    anim_entries.push(OstimAnimEntry {
                        folder: animation.clone(),
                        animation: animation.clone(),
                        actor_count,
                        oneshot,
                    });
                }
            }

            if let Some(src) = hkx_source {
                summary.hkx_copied += copy_scene_hkx(
                    src,
                    &pack_root,
                    &pack_folder,
                    scene,
                    &json,
                )?;
            }
        }
    }

    if summary.json_files == 0 {
        return Err("No scenes to export to OStim".into());
    }

    if let Some(src) = hkx_source {
        if let Some(p) = progress {
            p.phase("Copying HKX and pack extras…");
        }
        let (extra_hkx, extra_anims) =
            copy_remaining_source_hkx(src, &pack_root, &pack_folder, &seen_anim)?;
        summary.hkx_copied += extra_hkx;
        for (animation, oneshot, actor_count) in extra_anims {
            if seen_anim.insert(animation.clone()) {
                anim_entries.push(OstimAnimEntry {
                    folder: animation.clone(),
                    animation,
                    actor_count,
                    oneshot,
                });
            }
        }

        if copy_facial_expressions(src, &pack_root)? {
            summary.facial_copied = true;
        }

        if copy_sound_from_source(src, &pack_root)? {
            summary.sound_copied = true;
        }

        if let Some(nem) = copy_nemesis_from_source(src, &pack_root)? {
            summary.nemesis_dir = Some(nem);
            summary.nemesis_from_source = true;
        }
    }

    // Prefer embedded text assets (survive .slsb.json without the original folder).
    if let Some(p) = progress {
        p.phase("Writing animlist and assets…");
    }
    let n_assets = write_embedded_ostim_assets(pack, &pack_root)?;
    summary.assets_written = n_assets;
    if n_assets > 0 {
        summary.facial_copied = summary.facial_copied
            || pack
                .ostim_assets
                .keys()
                .any(|k| k.starts_with("facial expressions/"));
    }

    let animlist = write_ostim_animlist(&pack_root, &pack_folder, &anim_entries)?;
    summary.animlist = Some(animlist);

    if summary.nemesis_dir.is_none() {
        let author = if pack.pack_author.trim().is_empty() {
            "Unknown"
        } else {
            pack.pack_author.as_str()
        };
        let gen = write_nemesis_patches(
            &pack_root,
            &modpack,
            &pack_folder,
            author,
            "",
            &anim_entries,
        )?;
        summary.nemesis_dir = Some(gen.mod_dir);
        summary.nemesis_synthesized = true;
        info!(
            "Synthesized Nemesis patches for {} clip event(s) ({} files)",
            gen.clips, gen.files_written
        );
    }

    info!(
        "Wrote {} OStim scene JSON(s) under {}",
        summary.json_files,
        pack_root.display()
    );
    Ok(summary)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<usize, String> {
    if !src.is_dir() {
        return Ok(0);
    }
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let mut n = 0;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            n += copy_dir_recursive(&from, &to)?;
        } else {
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&from, &to).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

fn copy_facial_expressions(src_root: &Path, pack_root: &Path) -> Result<bool, String> {
    let src = src_root
        .join("SKSE")
        .join("Plugins")
        .join("OStim")
        .join("facial expressions");
    if !src.is_dir() {
        return Ok(false);
    }
    let dst = pack_root
        .join("SKSE")
        .join("Plugins")
        .join("OStim")
        .join("facial expressions");
    let n = copy_dir_recursive(&src, &dst)?;
    Ok(n > 0)
}

fn copy_sound_from_source(src_root: &Path, pack_root: &Path) -> Result<bool, String> {
    let src = src_root.join("Sound");
    if !src.is_dir() {
        return Ok(false);
    }
    let n = copy_dir_recursive(&src, &pack_root.join("Sound"))?;
    Ok(n > 0)
}

/// Collect small UTF-8 OStim assets for embedding in `.slsb.json`.
///
/// Covers facial expressions and custom action JSON under `SKSE/Plugins/OStim/`.
/// Binary assets (HKX, WAV, Nemesis trees) stay on disk via `ostim_source`.
pub fn collect_ostim_text_assets(
    src_root: &Path,
) -> Result<indexmap::IndexMap<String, String>, String> {
    use indexmap::IndexMap;
    let mut out = IndexMap::new();
    let ostim = src_root.join("SKSE").join("Plugins").join("OStim");
    if !ostim.is_dir() {
        return Ok(out);
    }
    for sub in ["facial expressions", "actions", "equip objects", "voice sets"] {
        let dir = ostim.join(sub);
        if !dir.is_dir() {
            continue;
        }
        collect_text_files_under(&dir, sub, &mut out)?;
    }
    Ok(out)
}

fn collect_text_files_under(
    dir: &Path,
    rel_prefix: &str,
    out: &mut indexmap::IndexMap<String, String>,
) -> Result<(), String> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(rd) = fs::read_dir(&cur) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let Some(ext) = p.extension().and_then(|x| x.to_str()) else {
                continue;
            };
            if !ext.eq_ignore_ascii_case("json")
                && !ext.eq_ignore_ascii_case("txt")
                && !ext.eq_ignore_ascii_case("ini")
            {
                continue;
            }
            let rel = p
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let key = if rel.is_empty() {
                rel_prefix.to_string()
            } else {
                format!("{rel_prefix}/{rel}")
            };
            match fs::read_to_string(&p) {
                Ok(body) => {
                    out.insert(key, body);
                }
                Err(_) => {
                    // Skip non-UTF8 binaries under these folders.
                }
            }
        }
    }
    Ok(())
}

fn write_embedded_ostim_assets(pack: &Package, pack_root: &Path) -> Result<usize, String> {
    if pack.ostim_assets.is_empty() {
        return Ok(0);
    }
    let base = pack_root.join("SKSE").join("Plugins").join("OStim");
    let mut n = 0usize;
    for (rel, body) in &pack.ostim_assets {
        let rel = rel.replace('\\', "/");
        let dest = base.join(Path::new(&rel));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Don't overwrite newer disk copies with identical content unnecessarily,
        // but always ensure the file exists for portable exports.
        fs::write(&dest, body).map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// Prefer the source pack's real Nemesis patches over the SLSB stub.
fn copy_nemesis_from_source(src_root: &Path, pack_root: &Path) -> Result<Option<PathBuf>, String> {
    let src_mod = src_root.join("Nemesis_Engine").join("mod");
    if !src_mod.is_dir() {
        return Ok(None);
    }
    let mut has_patch = false;
    for entry in fs::read_dir(&src_mod).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        // Real packs have behavior patch trees (0_master / defaultmale / …), not just info.ini.
        if p.join("0_master").is_dir()
            || p.join("defaultmale").is_dir()
            || p.join("defaultfemale").is_dir()
        {
            has_patch = true;
            break;
        }
        // Or any .txt patch fragments beyond README
        let mut txts = 0;
        if let Ok(rd) = fs::read_dir(&p) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".txt") && !name.contains("README") {
                    txts += 1;
                }
            }
        }
        if txts > 2 {
            has_patch = true;
            break;
        }
    }
    if !has_patch {
        return Ok(None);
    }
    let dst = pack_root.join("Nemesis_Engine");
    copy_dir_recursive(&src_root.join("Nemesis_Engine"), &dst)?;
    Ok(Some(dst.join("mod")))
}

/// Copy HKX clips referenced by the source animlist / present under source meshes
/// that scene JSON alone would miss (behavior-graph idle clips, etc.).
fn copy_remaining_source_hkx(
    src_root: &Path,
    pack_root: &Path,
    pack_folder: &str,
    seen_anim: &HashSet<String>,
) -> Result<(usize, Vec<(String, bool, usize)>), String> {
    let src_anim_root = src_root
        .join("meshes")
        .join("actors")
        .join("character")
        .join("animations");
    if !src_anim_root.is_dir() {
        return Ok((0, Vec::new()));
    }

    let dest_anim_root = pack_root
        .join("meshes")
        .join("actors")
        .join("character")
        .join("animations")
        .join(pack_folder);
    fs::create_dir_all(&dest_anim_root).map_err(|e| e.to_string())?;

    let mut copied = 0usize;
    let mut extras: Vec<(String, bool, usize)> = Vec::new();
    let mut extras_seen = HashSet::new();

    let walker = walkdir_hkx(&src_anim_root);
    for src_hkx in walker {
        let Some(stem) = src_hkx.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let (base, _actor) = match strip_ostim_actor_suffix(stem) {
            Some(v) => v,
            None => (stem.to_string(), 0),
        };
        // Skip if already covered by scene-driven copy for this animation name
        // (still copy if the specific file is missing from dest).
        let dest_dir = dest_anim_root.join(&base);
        let dest_file = dest_dir.join(format!("{stem}.hkx"));
        if dest_file.exists() {
            continue;
        }
        fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        fs::copy(&src_hkx, &dest_file).map_err(|e| e.to_string())?;
        copied += 1;

        if !seen_anim.contains(&base) && extras_seen.insert(base.clone()) {
            // Infer actor count from sibling clips in source folder.
            let actor_count = count_actor_clips(src_hkx.parent().unwrap_or(&src_anim_root), &base)
                .max(1);
            let oneshot = base.to_ascii_lowercase().contains("goto")
                || base.to_ascii_lowercase().contains("transition");
            extras.push((base, oneshot, actor_count));
        }
    }

    if copied > 0 {
        info!("Copied {copied} additional OStim HKX clip(s) from source pack");
    }
    Ok((copied, extras))
}

fn count_actor_clips(dir: &Path, base: &str) -> usize {
    let mut max_actor = 0usize;
    let Ok(rd) = fs::read_dir(dir) else {
        return 1;
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !name.ends_with(".hkx") {
            continue;
        }
        let stem = name.trim_end_matches(".hkx");
        if let Some((b, actor)) = strip_ostim_actor_suffix(stem) {
            if b.eq_ignore_ascii_case(base) {
                max_actor = max_actor.max(actor + 1);
            }
        }
    }
    max_actor.max(1)
}

fn walkdir_hkx(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("hkx"))
                .unwrap_or(false)
            {
                out.push(p);
            }
        }
    }
    out
}

/// Copy SLSB `_A#_S#` clips into OStim `{anim}_{actor}.hkx` layout.
/// When all speeds share one animation name, only stage 1's clips are copied.
fn copy_scene_hkx(
    source_root: &Path,
    pack_root: &Path,
    pack_folder: &str,
    scene: &Scene,
    json: &serde_json::Value,
) -> Result<usize, String> {
    let actor_count = scene.positions.len().max(1);
    let speeds = json
        .get("speeds")
        .and_then(|s| s.as_array())
        .cloned()
        .unwrap_or_default();
    if speeds.is_empty() {
        return Ok(0);
    }

    let shared_anim = speeds
        .iter()
        .filter_map(|s| s.get("animation").and_then(|a| a.as_str()))
        .collect::<Vec<_>>();
    let one_clip_set = shared_anim.len() > 1 && shared_anim.windows(2).all(|w| w[0] == w[1]);

    let mut copied = 0;
    if one_clip_set {
        let animation = shared_anim[0];
        let base = scene
            .stages
            .first()
            .and_then(|st| st.positions.first())
            .and_then(|p| p.event.first())
            .map(|e| strip_actor_stage_suffix(e).unwrap_or_else(|| e.clone()))
            .unwrap_or_else(|| animation.to_string());
        let dest = pack_root
            .join("meshes")
            .join("actors")
            .join("character")
            .join("animations")
            .join(pack_folder)
            .join(animation);
        copied += copy_slsb_hkx_to_ostim(source_root, &dest, &base, animation, 1, actor_count)?;
    } else {
        for (si, speed) in speeds.iter().enumerate() {
            let Some(animation) = speed.get("animation").and_then(|a| a.as_str()) else {
                continue;
            };
            let stage = scene.stages.get(si).or_else(|| scene.stages.first());
            let base = stage
                .and_then(|st| st.positions.first())
                .and_then(|p| p.event.first())
                .map(|e| strip_actor_stage_suffix(e).unwrap_or_else(|| e.clone()))
                .unwrap_or_else(|| animation.to_string());
            let dest = pack_root
                .join("meshes")
                .join("actors")
                .join("character")
                .join("animations")
                .join(pack_folder)
                .join(animation);
            copied += copy_slsb_hkx_to_ostim(
                source_root,
                &dest,
                &base,
                animation,
                si + 1,
                actor_count,
            )?;
        }
    }
    // Also try copying native OStim-named clips from the source pack for this animation.
    if let Some(anim) = shared_anim.first().copied() {
        copied += copy_native_ostim_hkx(source_root, pack_root, pack_folder, anim, actor_count)?;
    }
    Ok(copied)
}

fn copy_native_ostim_hkx(
    source_root: &Path,
    pack_root: &Path,
    pack_folder: &str,
    animation: &str,
    actor_count: usize,
) -> Result<usize, String> {
    let src_anim_root = source_root
        .join("meshes")
        .join("actors")
        .join("character")
        .join("animations");
    if !src_anim_root.is_dir() {
        return Ok(0);
    }
    let dest_dir = pack_root
        .join("meshes")
        .join("actors")
        .join("character")
        .join("animations")
        .join(pack_folder)
        .join(animation);
    let mut copied = 0;
    for actor in 0..actor_count {
        let name = format!("{animation}_{actor}.hkx");
        let dest = dest_dir.join(&name);
        if dest.exists() {
            continue;
        }
        // Search source tree for this filename.
        let mut found = None;
        for p in walkdir_hkx(&src_anim_root) {
            if p.file_name().and_then(|s| s.to_str()) == Some(name.as_str()) {
                found = Some(p);
                break;
            }
        }
        let Some(src) = found else {
            continue;
        };
        fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        fs::copy(&src, &dest).map_err(|e| e.to_string())?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::package::Package;
    use std::fs;

    #[test]
    fn round_trip_mlc_subset() {
        let root = PathBuf::from(
            "/mnt/Data/Coding/Animations/OStim/Lovemaking Compendium for OStim Standalone",
        );
        if !root.exists() {
            return;
        }
        let pack = Package::from_ostim(root.clone(), None).unwrap();
        assert!(
            pack.scenes.len() < 50 && pack.scenes.len() > 5,
            "expected grouped components, got {}",
            pack.scenes.len()
        );

        let tmp = std::env::temp_dir().join(format!("slsb_ostim_rt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let summary = write_ostim_pack(&pack, &tmp, Some(&root), None).unwrap();
        assert!(
            summary.json_files > 300,
            "expected one JSON per OStim node, got {}",
            summary.json_files
        );
        assert!(summary.animlist.as_ref().unwrap().exists());
        assert!(summary.facial_copied, "expected facial expressions copy");
        assert!(
            summary.nemesis_from_source,
            "expected Nemesis patches copied from source"
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}
