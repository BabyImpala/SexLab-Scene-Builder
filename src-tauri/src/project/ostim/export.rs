//! Write a complete OStim pack tree from an SLSB `Package`.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use log::info;

use crate::project::ostim::convert::{animations_from_ostim_json, scene_to_ostim_files};
use crate::project::ostim::events::{
    copy_slsb_hkx_to_ostim, sanitize_ostim_id, strip_actor_stage_suffix,
};
use crate::project::scene::Scene;
use crate::project::ostim::nemesis::{
    write_nemesis_stub, write_ostim_animlist, OstimAnimEntry,
};
use crate::project::package::Package;

#[derive(Debug, Default)]
pub struct OstimExportSummary {
    pub scenes_written: usize,
    pub json_files: usize,
    pub hkx_copied: usize,
    pub animlist: Option<PathBuf>,
    pub nemesis_dir: Option<PathBuf>,
}

pub fn pack_folder_name(pack: &Package) -> String {
    sanitize_ostim_id(&pack.fnis_mod_name(), &pack.prefix_hash.0)
}

/// Export OStim scenes + animlist + Nemesis stub under `root_dir/{Pack}/` or `root_dir`.
pub fn write_ostim_pack(
    pack: &Package,
    root_dir: &Path,
    hkx_source: Option<&Path>,
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
    let mut seen_anim = std::collections::HashSet::new();

    for scene in pack.scenes.values().filter(|s| !s.has_warnings && !s.stages.is_empty()) {
        let files = scene_to_ostim_files(scene, &modpack)?;
        if files.is_empty() {
            continue;
        }
        summary.scenes_written += 1;

        for (scene_id, json) in files {
            let folder = scenes_root.join(&scene_id);
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

    let animlist = write_ostim_animlist(&pack_root, &pack_folder, &anim_entries)?;
    summary.animlist = Some(animlist);
    let author = if pack.pack_author.trim().is_empty() {
        "Unknown"
    } else {
        pack.pack_author.as_str()
    };
    let nem = write_nemesis_stub(&pack_root, &modpack, author, &anim_entries)?;
    summary.nemesis_dir = Some(nem);

    info!(
        "Wrote {} OStim scene JSON(s) under {}",
        summary.json_files,
        pack_root.display()
    );
    Ok(summary)
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
        let pack = Package::from_ostim(root).unwrap();
        assert!(
            pack.scenes.len() < 50 && pack.scenes.len() > 5,
            "expected grouped components, got {}",
            pack.scenes.len()
        );

        let tmp = std::env::temp_dir().join(format!("slsb_ostim_rt_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let summary = write_ostim_pack(&pack, &tmp, None).unwrap();
        assert!(
            summary.json_files > 300,
            "expected one JSON per OStim node, got {}",
            summary.json_files
        );
        assert!(summary.animlist.as_ref().unwrap().exists());
        assert!(summary.nemesis_dir.as_ref().unwrap().join("info.ini").exists());
        let _ = fs::remove_dir_all(&tmp);
    }
}
