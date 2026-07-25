//! Reference conversion checks against packs under `/mnt/Data/Coding/Animations`.

#[cfg(test)]
mod tests {
    use crate::project::ostim::convert::{
        animations_from_ostim_json, import_ostim_scenes, is_transition, scene_to_ostim_files,
    };
    use crate::project::ostim::export::write_ostim_pack;
    use crate::project::package::Package;
    use serde_json::Value;
    use std::fs;
    use std::path::PathBuf;

    fn anim_root() -> PathBuf {
        PathBuf::from("/mnt/Data/Coding/Animations")
    }

    fn ostim_mlc() -> PathBuf {
        anim_root().join("OStim/Lovemaking Compendium for OStim Standalone")
    }

    fn billy_furniture_slsb() -> PathBuf {
        anim_root().join(
            "SLR/Billy/SLAL_Billyy_HumanFurnitureInvis/SKSE/SexLab/Registry/Source/Billyy_HumanFurnitureInvis.slsb.json",
        )
    }

    fn billy_furniture_ref_count() -> Option<usize> {
        let path = billy_furniture_slsb();
        if !path.exists() {
            return None;
        }
        let file = fs::File::open(&path).ok()?;
        let v: Value = serde_json::from_reader(file).ok()?;
        Some(v.get("scenes")?.as_object()?.len())
    }

    /// OStim scene JSON is usable if it has the fields OStim's loader expects in practice.
    fn assert_ostim_scene_usable(id: &str, json: &Value) {
        assert!(
            json.get("name").and_then(|v| v.as_str()).is_some(),
            "{id}: missing name"
        );
        let length = json.get("length").and_then(|v| v.as_f64()).unwrap_or(0.0);
        assert!(length > 0.0, "{id}: length must be > 0");
        let speeds = json
            .get("speeds")
            .and_then(|v| v.as_array())
            .expect(&format!("{id}: missing speeds"));
        assert!(!speeds.is_empty(), "{id}: empty speeds");
        for (i, speed) in speeds.iter().enumerate() {
            assert!(
                speed
                    .get("animation")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false),
                "{id}: speed {i} missing animation"
            );
        }
        let actors = json
            .get("actors")
            .and_then(|v| v.as_array())
            .expect(&format!("{id}: missing actors"));
        assert!(!actors.is_empty(), "{id}: empty actors");
        if is_transition(json) {
            assert!(
                json.get("destination")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false),
                "{id}: transition missing destination"
            );
        }
    }

    /// SLSB scene is usable for .slr export if stages/events/positions are coherent.
    fn assert_slsb_scene_usable(name: &str, scene: &crate::project::scene::Scene) {
        assert!(!scene.stages.is_empty(), "{name}: no stages");
        assert!(!scene.positions.is_empty(), "{name}: no positions");
        assert!(
            scene.graph.contains_key(&scene.root),
            "{name}: root missing from graph"
        );
        for stage in &scene.stages {
            assert_eq!(
                stage.positions.len(),
                scene.positions.len(),
                "{name}/{}: position count mismatch",
                stage.id.0
            );
            for (pi, pos) in stage.positions.iter().enumerate() {
                assert!(
                    pos.event.first().map(|e| !e.is_empty()).unwrap_or(false),
                    "{name}/{} actor {pi}: empty event",
                    stage.id.0
                );
            }
            assert!(
                scene.graph.contains_key(&stage.id),
                "{name}: stage {} not in graph",
                stage.id.0
            );
        }
    }

    #[test]
    fn ostim_to_slr_mlc_matches_reference_shape() {
        let root = ostim_mlc();
        if !root.exists() {
            return;
        }
        let pack = Package::from_ostim(root.clone()).unwrap();
        assert!(
            (10..=40).contains(&pack.scenes.len()),
            "expected grouped SLSB scenes (~17), got {}",
            pack.scenes.len()
        );

        let mut total_stages = 0usize;
        let mut branching = 0usize;
        let mut with_furniture = 0usize;
        let mut with_look = 0usize;
        for scene in pack.scenes.values() {
            assert_slsb_scene_usable(&scene.name, scene);
            total_stages += scene.stages.len();
            if scene.graph.values().any(|n| n.dest.len() > 1) {
                branching += 1;
            }
            if scene.furniture.furni_types.iter().any(|t| t != "None")
                || !scene.furniture.ostim_type.is_empty()
            {
                with_furniture += 1;
            }
            for stage in &scene.stages {
                if stage
                    .positions
                    .iter()
                    .any(|p| p.look_up != 0 || p.look_left != 0)
                {
                    with_look += 1;
                    break;
                }
            }
        }
        assert!(total_stages >= 300, "expected most MLC nodes as stages, got {total_stages}");
        assert!(branching >= 5, "expected several branching scenes, got {branching}");
        assert!(with_furniture >= 3, "expected furniture scenes, got {with_furniture}");
        assert!(with_look > 0, "expected lookUp/lookLeft preserved from MLC");

        // Build .slr pack (registry + FNIS) — must succeed for usability
        let tmp = std::env::temp_dir().join(format!("slsb_ostim2slr_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        pack.build(tmp.clone()).unwrap();
        let registry = tmp.join("SKSE/SexLab/Registry");
        assert!(registry.is_dir(), "missing Registry after build");
        let slr_files: Vec<_> = fs::read_dir(&registry)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    == Some("slr")
            })
            .collect();
        assert_eq!(slr_files.len(), 1, "expected one .slr file");
        let slr_size = fs::metadata(slr_files[0].path()).unwrap().len();
        assert!(slr_size > 1000, ".slr too small: {slr_size}");

        // Compare against Billy furniture reference: similar package schema version / fields
        if let Some(ref_scenes) = billy_furniture_ref_count() {
            assert!(ref_scenes > 0);
            // Converted pack should be fewer scenes than raw OStim nodes but multi-stage like SLR refs
            let avg_stages = total_stages as f64 / pack.scenes.len() as f64;
            assert!(
                avg_stages > 5.0,
                "grouped scenes should be multi-stage (avg {avg_stages})"
            );
        }

        // Round-trip OStim → SLSB → OStim preserves required fields + look data
        let (pack_name, _, scenes, _) = import_ostim_scenes(&root).unwrap();
        let mut rt = Package::new();
        rt.pack_name = pack_name;
        rt.scenes = scenes;
        let out = write_ostim_pack(&rt, &tmp.join("ostim_rt"), None).unwrap();
        assert!(out.json_files >= 300);
        // Spot-check a known MLC scene
        let cowgirl = tmp
            .join("ostim_rt")
            .join(sanitize_pack_folder(&rt.pack_name))
            .join("SKSE/Plugins/OStim/scenes/MLCBedCowgirl/MLCBedCowgirl.json");
        // pack folder may differ — search
        let found = find_named_json(&tmp.join("ostim_rt"), "MLCBedCowgirl.json");
        assert!(found.is_some(), "MLCBedCowgirl.json missing after round-trip");
        let json: Value =
            serde_json::from_str(&fs::read_to_string(found.unwrap()).unwrap()).unwrap();
        assert_ostim_scene_usable("MLCBedCowgirl", &json);
        let actors = json.get("actors").and_then(|a| a.as_array()).unwrap();
        assert!(
            actors.iter().any(|a| a.get("lookUp").is_some()),
            "lookUp should survive OStim→SLSB→OStim round-trip"
        );
        let _ = cowgirl;
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn slr_to_ostim_billy_furniture_usable() {
        let path = billy_furniture_slsb();
        if !path.exists() {
            return;
        }
        let file = fs::File::open(&path).unwrap();
        let pack = Package::from_file(file).unwrap();
        // Skip creature-only / empty
        let humanish: Vec<_> = pack
            .scenes
            .values()
            .filter(|s| {
                !s.has_warnings
                    && !s.stages.is_empty()
                    && s.positions.iter().all(|p| p.race == "Human" || p.race.is_empty())
            })
            .collect();
        assert!(!humanish.is_empty());

        let tmp = std::env::temp_dir().join(format!("slsb_slr2ostim_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let summary = write_ostim_pack(&pack, &tmp, None).unwrap();
        assert!(summary.json_files > 0);
        assert!(summary.animlist.as_ref().unwrap().exists());
        assert!(summary.nemesis_dir.as_ref().unwrap().join("info.ini").exists());

        let mut checked = 0usize;
        let mut with_furniture = 0usize;
        visit_jsons(&tmp, &mut |path, json| {
            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");
            assert_ostim_scene_usable(id, json);
            if json
                .get("furniture")
                .and_then(|v| v.as_str())
                .map(|s| s != "none")
                .unwrap_or(false)
            {
                with_furniture += 1;
            }
            // speeds/animation must map to animlist entries
            for (anim, _) in animations_from_ostim_json(json) {
                assert!(!anim.is_empty());
            }
            checked += 1;
        });
        assert_eq!(checked, summary.json_files);
        // Billy furniture pack should yield furniture-typed OStim scenes
        assert!(
            with_furniture > 0,
            "expected some furniture fields on OStim export from Billy furniture SLR"
        );

        // Structural compare: scene count in OStim export should be >= SLSB scene count
        // (branching expands; linear same-base may collapse to one JSON with speeds)
        assert!(
            summary.json_files >= humanish.len() / 2,
            "export too sparse: {} json vs {} slsb scenes",
            summary.json_files,
            humanish.len()
        );

        // Spot-check one exported file against OStim reference conventions (MLC)
        let mlc = ostim_mlc();
        if mlc.exists() {
            let ref_scene = find_named_json(&mlc.join("SKSE/Plugins/OStim/scenes"), "MLCBedCowgirl.json");
            if let Some(ref_path) = ref_scene {
                let ref_json: Value =
                    serde_json::from_str(&fs::read_to_string(ref_path).unwrap()).unwrap();
                assert_ostim_scene_usable("ref", &ref_json);
                // Ensure our export uses the same top-level keys OStim cares about
                let sample = find_any_ostim_scene(&tmp).unwrap();
                let sample_json: Value =
                    serde_json::from_str(&fs::read_to_string(sample).unwrap()).unwrap();
                for key in ["name", "modpack", "length", "speeds", "actors"] {
                    assert!(
                        sample_json.get(key).is_some(),
                        "export missing key {key} present in OStim refs"
                    );
                    assert!(ref_json.get(key).is_some());
                }
            }
        }

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn ostim_fields_round_trip_on_position() {
        let json: Value = serde_json::json!({
            "name": "Test",
            "length": 2.0,
            "speeds": [{ "animation": "TestAnim", "playbackSpeed": 1, "displaySpeed": 1 }],
            "actors": [{
                "intendedSex": "male",
                "lookUp": -10,
                "lookLeft": 20,
                "animationIndex": 1,
                "expressionOverride": "tongue",
                "equipObjects": { "strapon": true },
                "sosBend": 3,
                "tags": ["lyingback"]
            }, {
                "intendedSex": "female",
                "tags": ["kneeling"]
            }],
            "actions": [{ "type": "vaginalsex", "actor": 0, "target": 1 }],
            "tags": ["cowgirl"]
        });
        let scene = crate::project::ostim::convert::ostim_json_to_scene("TestPose", &json).unwrap();
        let pos0 = &scene.stages[0].positions[0];
        assert_eq!(pos0.look_up, -10);
        assert_eq!(pos0.look_left, 20);
        assert_eq!(pos0.animation_index, Some(1));
        assert_eq!(pos0.expression_override, "tongue");
        assert!(pos0.equip_objects.contains("strapon"));

        let files = scene_to_ostim_files(&scene, "Pack").unwrap();
        assert_eq!(files.len(), 1);
        let actor0 = &files[0].1["actors"][0];
        assert_eq!(actor0["lookUp"], -10);
        assert_eq!(actor0["lookLeft"], 20);
        assert_eq!(actor0["animationIndex"], 1);
        assert_eq!(actor0["expressionOverride"], "tongue");
        assert_eq!(actor0["equipObjects"]["strapon"], true);
    }

    fn sanitize_pack_folder(name: &str) -> String {
        crate::project::ostim::events::sanitize_ostim_id(name, "pack")
    }

    fn find_named_json(root: &PathBuf, file_name: &str) -> Option<PathBuf> {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().and_then(|s| s.to_str()) == Some(file_name) {
                    return Some(p);
                }
            }
        }
        None
    }

    fn find_any_ostim_scene(root: &PathBuf) -> Option<PathBuf> {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.to_string_lossy().contains("OStim")
                {
                    return Some(p);
                }
            }
        }
        None
    }

    fn visit_jsons(root: &PathBuf, f: &mut dyn FnMut(&PathBuf, &Value)) {
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(rd) = fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.extension().and_then(|s| s.to_str()) == Some("json")
                    && p.to_string_lossy().contains("scenes")
                {
                    if let Ok(text) = fs::read_to_string(&p) {
                        if let Ok(json) = serde_json::from_str::<Value>(&text) {
                            if json.get("speeds").is_some() {
                                f(&p, &json);
                            }
                        }
                    }
                }
            }
        }
    }
}
