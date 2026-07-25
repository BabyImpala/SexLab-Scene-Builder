//! OStim animlist + Nemesis stub generation for playable pack scaffolding.
//!
//! Full Nemesis behavior graphs (clip generators, state machines, `$variableID[OStimSpeed]$`
//! wiring across `#mod$N.txt` fragments) are pack-specific and not synthesized here.
//! Instead we emit:
//! - `ATT_*_animlist.txt` (FNIS/Nemesis registration lines)
//! - `Nemesis_Engine/mod/<id>/info.ini`
//! - `Nemesis_Engine/mod/<id>/README_SLSB.txt` instructing the user to run Nemesis

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::project::ostim::events::{ostim_actor_event, ostim_hkx_rel_path};

#[derive(Debug, Clone)]
pub struct OstimAnimEntry {
    pub animation: String,
    /// Folder under `meshes/actors/character/animations/<pack>/`
    pub folder: String,
    pub actor_count: usize,
    /// Transition / one-shot clips use `-a,Tn`
    pub oneshot: bool,
}

pub fn sanitize_nemesis_mod_id(pack_name: &str) -> String {
    let cleaned: String = pack_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if cleaned.is_empty() {
        "slsbostim".into()
    } else if cleaned.len() > 8 {
        cleaned[..8].to_string()
    } else {
        cleaned
    }
}

pub fn write_ostim_animlist(
    pack_root: &Path,
    pack_folder: &str,
    entries: &[OstimAnimEntry],
) -> Result<std::path::PathBuf, String> {
    let anim_root = pack_root
        .join("meshes")
        .join("actors")
        .join("character")
        .join("animations")
        .join(pack_folder);
    fs::create_dir_all(&anim_root).map_err(|e| e.to_string())?;

    let list_name = format!("ATT_{pack_folder}_animlist.txt");
    let list_path = anim_root.join(&list_name);
    let mut file = fs::File::create(&list_path).map_err(|e| e.to_string())?;

    writeln!(
        file,
        "' SLSB-generated OStim animlist for {pack_folder}"
    )
    .map_err(|e| e.to_string())?;
    writeln!(file, "' Run Nemesis (or compatible) to register these events.").map_err(|e| e.to_string())?;
    writeln!(file).map_err(|e| e.to_string())?;

    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        for actor in 0..entry.actor_count {
            let event = ostim_actor_event(&entry.animation, actor);
            if !seen.insert(event.clone()) {
                continue;
            }
            let rel = ostim_hkx_rel_path(pack_folder, &entry.folder, &entry.animation, actor);
            let flags = if entry.oneshot { "-a,Tn" } else { "-Tn" };
            writeln!(file, "b {flags} {event} {rel}").map_err(|e| e.to_string())?;
        }
    }
    Ok(list_path)
}

pub fn write_nemesis_stub(
    pack_root: &Path,
    pack_name: &str,
    author: &str,
    entries: &[OstimAnimEntry],
) -> Result<std::path::PathBuf, String> {
    let mod_id = sanitize_nemesis_mod_id(pack_name);
    let mod_dir = pack_root
        .join("Nemesis_Engine")
        .join("mod")
        .join(&mod_id);
    fs::create_dir_all(&mod_dir).map_err(|e| e.to_string())?;

    let info = format!(
        "name={pack_name}\n\
         author={author}\n\
         site=\n\
         auto=null\n\
         hidden=true\n"
    );
    fs::write(mod_dir.join("info.ini"), info).map_err(|e| e.to_string())?;

    // Event checklist for authors / future patch generation
    let mut events = Vec::new();
    for entry in entries {
        for actor in 0..entry.actor_count {
            events.push(ostim_actor_event(&entry.animation, actor));
        }
    }
    events.sort();
    events.dedup();

    let readme = format!(
        "SLSB OStim export — Nemesis stub\n\
         ================================\n\
         \n\
         This folder only contains info.ini and an event checklist.\n\
         OStim playbackSpeed requires Nemesis patches that bind clips to the\n\
         OStimSpeed graph variable. Generate a full behavior patch by running\n\
         Nemesis against the ATT_*_animlist.txt under:\n\
           meshes/actors/character/animations/<pack>/\n\
         \n\
         Registered animation events ({count}):\n\
         {list}\n",
        count = events.len(),
        list = events
            .iter()
            .map(|e| format!("  - {e}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    fs::write(mod_dir.join("README_SLSB.txt"), readme).map_err(|e| e.to_string())?;

    // Lightweight event name fragment Nemesis-style authors sometimes merge manually
    let mut event_xml = String::from(
        "<!-- SLSB-generated event name list (merge into behavior string data as needed) -->\n",
    );
    for e in &events {
        event_xml.push_str(&format!("\t\t\t\t<hkcstring>{e}</hkcstring>\n"));
    }
    fs::write(mod_dir.join("slsb_event_names.xml.fragment"), event_xml)
        .map_err(|e| e.to_string())?;

    Ok(mod_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn writes_animlist_and_stub() {
        let tmp = std::env::temp_dir().join(format!("slsb_ostim_nem_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let entries = vec![OstimAnimEntry {
            animation: "TestAnim".into(),
            folder: "TestAnim".into(),
            actor_count: 2,
            oneshot: false,
        }];
        let list = write_ostim_animlist(&tmp, "TestPack", &entries).unwrap();
        assert!(list.exists());
        let body = fs::read_to_string(&list).unwrap();
        assert!(body.contains("b -Tn TestAnim_0"));
        assert!(body.contains("TestAnim_1"));
        let nem = write_nemesis_stub(&tmp, "Test Pack", "Author", &entries).unwrap();
        assert!(nem.join("info.ini").exists());
        let _ = fs::remove_dir_all(&tmp);
    }
}
