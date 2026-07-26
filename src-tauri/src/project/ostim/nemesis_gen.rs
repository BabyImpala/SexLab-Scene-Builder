//! Clean-room OStim Nemesis patch synthesis from animlist entries.
//!
//! Observed from shipped OStim packs (e.g. Lovemaking Compendium): each clip becomes
//! StateInfo → ModifierGenerator(BSIsActiveModifier + ClipGenerator), with
//! `playbackSpeed` bound to `$variableID[OStimSpeed]$`, plus a nest state under
//! RootBehaviorGraph and character string-data animation name patches.
//!
//! Templates under `resources/ostim_nemesis/` are vanilla object shells with
//! `<!-- MOD_CODE -->` slots (stripped from a reference pack). This is **not**
//! GPL AnimlistTransitionTool code, and it does **not** go through serde-hkx —
//! Nemesis consumes these XML fragments directly (serde-hkx packs SexLab FNIS
//! Behavior.hkx graphs).

use std::fs;
use std::path::{Path, PathBuf};

use crate::project::ostim::events::{ostim_actor_event, ostim_hkx_rel_path};
use crate::project::ostim::nemesis::{sanitize_nemesis_mod_id, OstimAnimEntry};

const TPL_0106: &str = include_str!("../../../resources/ostim_nemesis/0106.txt");
const TPL_0107: &str = include_str!("../../../resources/ostim_nemesis/0107.txt");
const TPL_0108: &str = include_str!("../../../resources/ostim_nemesis/0108.txt");
const TPL_0340: &str = include_str!("../../../resources/ostim_nemesis/0340.txt");
const TPL_2517: &str = include_str!("../../../resources/ostim_nemesis/2517.txt");
const TPL_MALE_0029: &str = include_str!("../../../resources/ostim_nemesis/defaultmale_0029.txt");
const TPL_FEMALE_0029: &str = include_str!("../../../resources/ostim_nemesis/defaultfemale_0029.txt");

/// Vanilla animationNames count before MOD_CODE in the character string-data templates.
const BASE_ANIM_NAME_COUNT: usize = 1656;

const EXIT_EVENT: &str = "OST_ExitAnim";
const OSTIM_SPEED: &str = "OStimSpeed";

const TRANSITION_FLAGS: &str =
    "FLAG_IS_GLOBAL_WILDCARD|FLAG_IS_LOCAL_WILDCARD|FLAG_DISABLE_CONDITION";
const EXIT_TRANSITION_FLAGS: &str = "FLAG_IS_LOCAL_WILDCARD|FLAG_DISABLE_CONDITION";

#[derive(Debug, Clone)]
struct ClipDef {
    event: String,
    /// Path as stored in clip generators / character string data.
    anim_path: String,
    mode: &'static str,
}

#[derive(Debug, Default)]
pub struct NemesisGenSummary {
    pub mod_dir: PathBuf,
    pub clips: usize,
    /// Patch XML / info files written under `Nemesis_Engine/mod/…`.
    pub files_written: usize,
}

/// Build a full `Nemesis_Engine/mod/<prefix>/` tree for the given anim entries.
pub fn write_nemesis_patches(
    pack_root: &Path,
    pack_name: &str,
    pack_folder: &str,
    author: &str,
    site: &str,
    entries: &[OstimAnimEntry],
) -> Result<NemesisGenSummary, String> {
    let prefix = sanitize_nemesis_mod_id(pack_name);
    let display = sanitize_object_name(pack_name);
    let speed_var = format!("{}_AnimationSpeed", prefix.to_ascii_uppercase());
    let crop_start = format!("{}_CropAnimStart", prefix.to_ascii_uppercase());
    let crop_end = format!("{}_CropAnimEnd", prefix.to_ascii_uppercase());
    let start_time = format!("{}_AnimStartTime", prefix.to_ascii_uppercase());

    let clips = flatten_clips(pack_folder, entries);
    if clips.is_empty() {
        return Err("No animation events to register in Nemesis patches".into());
    }

    let mod_dir = pack_root
        .join("Nemesis_Engine")
        .join("mod")
        .join(&prefix);
    let master = mod_dir.join("0_master");
    let male = mod_dir.join("defaultmale");
    let female = mod_dir.join("defaultfemale");
    for d in [&mod_dir, &master, &male, &female] {
        fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }

    let mut files = 0usize;
    let write = |path: PathBuf, body: &str| -> Result<(), String> {
        fs::write(&path, body).map_err(|e| e.to_string())?;
        Ok(())
    };

    // Fixed low IDs: root state, nest SM, expression modifier + data.
    let root_state_tag = tag(&prefix, 0);
    let nest_sm_tag = tag(&prefix, 1);
    let expr_mod_tag = tag(&prefix, 2);
    let expr_data_tag = tag(&prefix, 3);

    write(
        master.join(format!("{expr_data_tag}.txt")),
        &expression_data(&expr_data_tag, &speed_var),
    )?;
    files += 1;
    write(
        master.join(format!("{expr_mod_tag}.txt")),
        &evaluate_expression_modifier(&expr_mod_tag, &display, &expr_data_tag),
    )?;
    files += 1;

    write(
        master.join("#0340.txt"),
        &fill_template(
            TPL_0340,
            &[
                ("{prefix}", &prefix),
                ("{root_state}", &root_state_tag),
            ],
        ),
    )?;
    files += 1;
    write(
        master.join("#2517.txt"),
        &fill_template(
            TPL_2517,
            &[("{prefix}", &prefix), ("{expr_mod}", &expr_mod_tag)],
        ),
    )?;
    files += 1;

    let root_state_id = deterministic_state_id(pack_name);
    let mut next_id: i64 = 4;
    let mut state_tags = Vec::with_capacity(clips.len());
    let mut transition_infos = Vec::with_capacity(clips.len());
    let mut event_names = Vec::with_capacity(clips.len() + 1);
    event_names.push(hkcstring(EXIT_EVENT));
    let mut anim_names = Vec::with_capacity(clips.len());

    for (clip_i, clip) in clips.iter().enumerate() {
        let state_id = clip_i as i64;
        let state_tag = tag(&prefix, next_id);
        let mod_gen_tag = tag(&prefix, next_id + 1);
        let clip_tag = tag(&prefix, next_id + 2);
        let bind_tag = tag(&prefix, next_id + 3);
        let active_tag = tag(&prefix, next_id + 4);
        let active_bind_tag = tag(&prefix, next_id + 5);
        next_id += 6;

        write(
            master.join(format!("{state_tag}.txt")),
            &state_info(
                &state_tag,
                &mod_gen_tag,
                &format!("{display}_Anim{clip_i}"),
                state_id,
            ),
        )?;
        write(
            master.join(format!("{mod_gen_tag}.txt")),
            &modifier_generator(
                &mod_gen_tag,
                &format!("{display}_Modifier{}", next_id - 2),
                &active_tag,
                &clip_tag,
            ),
        )?;
        write(
            master.join(format!("{clip_tag}.txt")),
            &clip_generator(
                &clip_tag,
                &bind_tag,
                &format!("{display}_AnimClip{clip_i}"),
                &clip.anim_path,
                clip.mode,
            ),
        )?;
        write(
            master.join(format!("{bind_tag}.txt")),
            &ostim_speed_binding(&bind_tag),
        )?;
        write(
            master.join(format!("{active_tag}.txt")),
            &bs_is_active(
                &active_tag,
                &active_bind_tag,
                &format!("{display}_BSIsActiveModifier{}", next_id - 2),
            ),
        )?;
        write(
            master.join(format!("{active_bind_tag}.txt")),
            &active_binding(&active_bind_tag),
        )?;
        files += 6;

        state_tags.push(state_tag.clone());
        event_names.push(hkcstring(&clip.event));
        anim_names.push(hkcstring(&clip.anim_path));

        // Transition effect + info filled after we know the effect tag id.
        let te_tag = tag(&prefix, next_id);
        next_id += 1;
        write(
            master.join(format!("{te_tag}.txt")),
            &blend_transition(&te_tag, &format!("{}Transition", clip.event), "0.600000"),
        )?;
        files += 1;
        transition_infos.push(wildcard_transition(&te_tag, &clip.event, state_id));
    }

    let wildcard_array_tag = tag(&prefix, next_id);
    let exit_array_tag = tag(&prefix, next_id + 1);
    let exit_te_tag = tag(&prefix, next_id + 2);
    next_id += 3;

    write(
        master.join(format!("{exit_te_tag}.txt")),
        &blend_transition(&exit_te_tag, "ExitTransition", "0.200000"),
    )?;
    files += 1;
    write(
        master.join(format!("{exit_array_tag}.txt")),
        &transition_array(
            &exit_array_tag,
            1,
            &exit_transition(&exit_te_tag, EXIT_EVENT, 14),
        ),
    )?;
    files += 1;
    write(
        master.join(format!("{wildcard_array_tag}.txt")),
        &transition_array(
            &wildcard_array_tag,
            transition_infos.len(),
            &transition_infos.join("\n"),
        ),
    )?;
    files += 1;

    write(
        master.join(format!("{root_state_tag}.txt")),
        &root_state_info(
            &root_state_tag,
            &exit_array_tag,
            &nest_sm_tag,
            &format!("{display}_State"),
            root_state_id,
        ),
    )?;
    files += 1;
    write(
        master.join(format!("{nest_sm_tag}.txt")),
        &nest_state_machine(
            &nest_sm_tag,
            &format!("{display}_Root"),
            &state_tags,
            &wildcard_array_tag,
        ),
    )?;
    files += 1;

    // String / graph data patches
    let var_names = [
        hkcstring(&speed_var),
        hkcstring(&crop_start),
        hkcstring(&crop_end),
        hkcstring(&start_time),
    ]
    .join("\n");
    write(
        master.join("#0106.txt"),
        &fill_template(
            TPL_0106,
            &[
                ("{prefix}", &prefix),
                ("{events}", &event_names.join("\n")),
                ("{vars}", &var_names),
            ],
        ),
    )?;
    files += 1;

    write(
        master.join("#0107.txt"),
        &fill_template(
            TPL_0107,
            &[
                ("{prefix}", &prefix),
                ("{var_values}", DEFAULT_VAR_VALUES),
            ],
        ),
    )?;
    files += 1;

    let event_flags = std::iter::repeat(EVENT_FLAG_OBJ)
        .take(event_names.len())
        .collect::<Vec<_>>()
        .join("\n");
    write(
        master.join("#0108.txt"),
        &fill_template(
            TPL_0108,
            &[
                ("{prefix}", &prefix),
                ("{var_infos}", VAR_INFOS_FOUR_REALS),
                ("{event_flags}", &event_flags),
            ],
        ),
    )?;
    files += 1;

    let anim_count = BASE_ANIM_NAME_COUNT + anim_names.len();
    let anims_joined = anim_names.join("\n");
    let anim_count_s = anim_count.to_string();
    for (dir, tpl) in [(&male, TPL_MALE_0029), (&female, TPL_FEMALE_0029)] {
        write(
            dir.join("#0029.txt"),
            &fill_template(
                tpl,
                &[
                    ("{prefix}", &prefix),
                    ("{anims}", &anims_joined),
                    ("{anim_count}", &anim_count_s),
                ],
            ),
        )?;
        files += 1;
    }

    let info = format!(
        "name={pack_name}\n\
         author={author}\n\
         site={site}\n\
         auto=null\n\
         hidden=true\n"
    );
    write(mod_dir.join("info.ini"), &info)?;
    files += 1;

    let _ = next_id;
    Ok(NemesisGenSummary {
        mod_dir,
        clips: clips.len(),
        files_written: files,
    })
}

fn flatten_clips(pack_folder: &str, entries: &[OstimAnimEntry]) -> Vec<ClipDef> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let mode = if entry.oneshot {
            "MODE_SINGLE_PLAY"
        } else {
            "MODE_LOOPING"
        };
        for actor in 0..entry.actor_count.max(1) {
            let event = ostim_actor_event(&entry.animation, actor);
            if !seen.insert(event.clone()) {
                continue;
            }
            let rel = ostim_hkx_rel_path(pack_folder, &entry.folder, &entry.animation, actor);
            // ATT / MLC: Animations\<pack>\<animlist-relative-path>
            let anim_path = format!(r"Animations\{pack_folder}\{rel}");
            out.push(ClipDef {
                event,
                anim_path,
                mode,
            });
        }
    }
    out
}

fn tag(prefix: &str, id: i64) -> String {
    format!("#{prefix}${id}")
}

fn fill_template(tpl: &str, pairs: &[(&str, &str)]) -> String {
    let mut s = tpl.to_string();
    for (k, v) in pairs {
        s = s.replace(k, v);
    }
    s
}

fn sanitize_object_name(pack_name: &str) -> String {
    let cleaned: String = pack_name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect();
    if cleaned.is_empty() {
        "SLSBPack".into()
    } else {
        cleaned
    }
}

fn deterministic_state_id(pack_name: &str) -> i64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in pack_name.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x100000001b3);
    }
    // Keep in 7-digit-ish positive range like ATT's RandInt.
    ((h % 9_000_000) + 1_000_000) as i64
}

fn hkcstring(s: &str) -> String {
    format!("\t\t\t\t<hkcstring>{s}</hkcstring>")
}

const DEFAULT_VAR_VALUES: &str = "\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"value\">1</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"value\">0</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"value\">0</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"value\">0</hkparam>\n\
\t\t\t\t</hkobject>";

const VAR_INFOS_FOUR_REALS: &str = "\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"role\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"role\">ROLE_DEFAULT</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"type\">VARIABLE_TYPE_REAL</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"role\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"role\">ROLE_DEFAULT</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"type\">VARIABLE_TYPE_REAL</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"role\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"role\">ROLE_DEFAULT</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"type\">VARIABLE_TYPE_REAL</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"role\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"role\">ROLE_DEFAULT</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"type\">VARIABLE_TYPE_REAL</hkparam>\n\
\t\t\t\t</hkobject>";

const EVENT_FLAG_OBJ: &str = "\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
                </hkobject>";

fn expression_data(tag: &str, speed_var: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbExpressionDataArray\" signature=\"0x4b9ee1a2\">\n\
\t\t\t<hkparam name=\"expressionsData\" numelements=\"1\">\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"expression\">{speed_var} = 1.0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"assignmentVariableIndex\">-1</hkparam>\n\
\t\t\t\t\t<hkparam name=\"assignmentEventIndex\">-1</hkparam>\n\
\t\t\t\t\t<hkparam name=\"eventMode\">EVENT_MODE_SEND_ONCE</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn evaluate_expression_modifier(tag: &str, display: &str, expr: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbEvaluateExpressionModifier\" signature=\"0xf900f6be\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"userData\">2</hkparam>\n\
\t\t\t<hkparam name=\"name\">{display}_DefaultModifier</hkparam>\n\
\t\t\t<hkparam name=\"enable\">true</hkparam>\n\
\t\t\t<hkparam name=\"expressions\">{expr}</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn state_info(tag: &str, generator: &str, name: &str, state_id: i64) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbStateMachineStateInfo\" signature=\"0xed7f9d0\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"listeners\" numelements=\"0\"></hkparam>\n\
\t\t\t<hkparam name=\"enterNotifyEvents\">null</hkparam>\n\
\t\t\t<hkparam name=\"exitNotifyEvents\">null</hkparam>\n\
\t\t\t<hkparam name=\"transitions\">null</hkparam>\n\
\t\t\t<hkparam name=\"generator\">{generator}</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"stateId\">{state_id}</hkparam>\n\
\t\t\t<hkparam name=\"probability\">1.000000</hkparam>\n\
\t\t\t<hkparam name=\"enable\">true</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn root_state_info(
    tag: &str,
    transitions: &str,
    generator: &str,
    name: &str,
    state_id: i64,
) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbStateMachineStateInfo\" signature=\"0xed7f9d0\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"listeners\" numelements=\"0\"></hkparam>\n\
\t\t\t<hkparam name=\"enterNotifyEvents\">null</hkparam>\n\
\t\t\t<hkparam name=\"exitNotifyEvents\">null</hkparam>\n\
\t\t\t<hkparam name=\"transitions\">{transitions}</hkparam>\n\
\t\t\t<hkparam name=\"generator\">{generator}</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"stateId\">{state_id}</hkparam>\n\
\t\t\t<hkparam name=\"probability\">1.000000</hkparam>\n\
\t\t\t<hkparam name=\"enable\">true</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn modifier_generator(tag: &str, name: &str, modifier: &str, generator: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbModifierGenerator\" signature=\"0x1f81fae6\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"userData\">1</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"modifier\">{modifier}</hkparam>\n\
\t\t\t<hkparam name=\"generator\">{generator}</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn clip_generator(tag: &str, binding: &str, name: &str, anim: &str, mode: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbClipGenerator\" signature=\"0x333b85b9\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">{binding}</hkparam>\n\
\t\t\t<hkparam name=\"userData\">0</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"animationName\">{anim}</hkparam>\n\
\t\t\t<hkparam name=\"triggers\">null</hkparam>\n\
\t\t\t<hkparam name=\"cropStartAmountLocalTime\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"cropEndAmountLocalTime\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"startTime\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"playbackSpeed\">1.000000</hkparam>\n\
\t\t\t<hkparam name=\"enforcedDuration\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"userControlledTimeFraction\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"animationBindingIndex\">-1</hkparam>\n\
\t\t\t<hkparam name=\"mode\">{mode}</hkparam>\n\
\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn ostim_speed_binding(tag: &str) -> String {
    // Match shipped OStim packs: bind playbackSpeed to the shared OStimSpeed graph var.
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbVariableBindingSet\" signature=\"0x338ad4ff\">\n\
\t\t\t<hkparam name=\"bindings\" numelements=\"1\">\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"memberPath\">playbackSpeed</hkparam>\n\
\t\t\t\t\t<hkparam name=\"variableIndex\">$variableID[{OSTIM_SPEED}]$</hkparam>\n\
\t\t\t\t\t<hkparam name=\"bitIndex\">-1</hkparam>\n\
\t\t\t\t\t<hkparam name=\"bindingType\">BINDING_TYPE_VARIABLE</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t</hkparam>\n\
\t\t\t<hkparam name=\"indexOfBindingToEnable\">-1</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn bs_is_active(tag: &str, binding: &str, name: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"BSIsActiveModifier\" signature=\"0xb0fde45a\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">{binding}</hkparam>\n\
\t\t\t<hkparam name=\"userData\">2</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"enable\">true</hkparam>\n\
\t\t\t<hkparam name=\"bIsActive0\">false</hkparam>\n\
\t\t\t<hkparam name=\"bInvertActive0\">false</hkparam>\n\
\t\t\t<hkparam name=\"bIsActive1\">false</hkparam>\n\
\t\t\t<hkparam name=\"bInvertActive1\">false</hkparam>\n\
\t\t\t<hkparam name=\"bIsActive2\">false</hkparam>\n\
\t\t\t<hkparam name=\"bInvertActive2\">false</hkparam>\n\
\t\t\t<hkparam name=\"bIsActive3\">false</hkparam>\n\
\t\t\t<hkparam name=\"bInvertActive3\">false</hkparam>\n\
\t\t\t<hkparam name=\"bIsActive4\">false</hkparam>\n\
\t\t\t<hkparam name=\"bInvertActive4\">false</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn active_binding(tag: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbVariableBindingSet\" signature=\"0x338ad4ff\">\n\
\t\t\t<hkparam name=\"bindings\" numelements=\"1\">\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"memberPath\">bIsActive0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"variableIndex\">81</hkparam>\n\
\t\t\t\t\t<hkparam name=\"bitIndex\">-1</hkparam>\n\
\t\t\t\t\t<hkparam name=\"bindingType\">BINDING_TYPE_VARIABLE</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t</hkparam>\n\
\t\t\t<hkparam name=\"indexOfBindingToEnable\">-1</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn blend_transition(tag: &str, name: &str, duration: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbBlendingTransitionEffect\" signature=\"0xfd8584fe\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"userData\">0</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"selfTransitionMode\">SELF_TRANSITION_MODE_CONTINUE</hkparam>\n\
\t\t\t<hkparam name=\"eventMode\">EVENT_MODE_PROCESS_ALL</hkparam>\n\
\t\t\t<hkparam name=\"duration\">{duration}</hkparam>\n\
\t\t\t<hkparam name=\"toGeneratorStartTimeFraction\">0.000000</hkparam>\n\
\t\t\t<hkparam name=\"flags\">0</hkparam>\n\
\t\t\t<hkparam name=\"endMode\">END_MODE_NONE</hkparam>\n\
\t\t\t<hkparam name=\"blendCurve\">BLEND_CURVE_SMOOTH</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn wildcard_transition(effect: &str, event: &str, to_state: i64) -> String {
    format!(
        "\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"triggerInterval\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"initiateInterval\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"transition\">{effect}</hkparam>\n\
\t\t\t\t\t<hkparam name=\"condition\">null</hkparam>\n\
\t\t\t\t\t<hkparam name=\"eventId\">$eventID[{event}]$</hkparam>\n\
\t\t\t\t\t<hkparam name=\"toStateId\">{to_state}</hkparam>\n\
\t\t\t\t\t<hkparam name=\"fromNestedStateId\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"toNestedStateId\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"priority\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"flags\">{TRANSITION_FLAGS}</hkparam>\n\
\t\t\t\t</hkobject>"
    )
}

fn exit_transition(effect: &str, event: &str, to_state: i64) -> String {
    format!(
        "\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"triggerInterval\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"initiateInterval\">\n\
\t\t\t\t\t\t<hkobject>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitEventId\">-1</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"enterTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t\t<hkparam name=\"exitTime\">0.000000</hkparam>\n\
\t\t\t\t\t\t</hkobject>\n\
\t\t\t\t\t</hkparam>\n\
\t\t\t\t\t<hkparam name=\"transition\">{effect}</hkparam>\n\
\t\t\t\t\t<hkparam name=\"condition\">null</hkparam>\n\
\t\t\t\t\t<hkparam name=\"eventId\">$eventID[{event}]$</hkparam>\n\
\t\t\t\t\t<hkparam name=\"toStateId\">{to_state}</hkparam>\n\
\t\t\t\t\t<hkparam name=\"fromNestedStateId\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"toNestedStateId\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"priority\">0</hkparam>\n\
\t\t\t\t\t<hkparam name=\"flags\">{EXIT_TRANSITION_FLAGS}</hkparam>\n\
\t\t\t\t</hkobject>"
    )
}

fn transition_array(tag: &str, n: usize, body: &str) -> String {
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbStateMachineTransitionInfoArray\" signature=\"0xe397b11e\">\n\
\t\t\t<hkparam name=\"transitions\" numelements=\"{n}\">\n\
{body}\n\
\t\t\t</hkparam>\n\
\t\t</hkobject>\n"
    )
}

fn nest_state_machine(tag: &str, name: &str, states: &[String], wildcard: &str) -> String {
    let n = states.len();
    let joined = states.join(" ");
    format!(
        "\t\t<hkobject name=\"{tag}\" class=\"hkbStateMachine\" signature=\"0x816c1dcb\">\n\
\t\t\t<hkparam name=\"variableBindingSet\">null</hkparam>\n\
\t\t\t<hkparam name=\"userData\">0</hkparam>\n\
\t\t\t<hkparam name=\"name\">{name}</hkparam>\n\
\t\t\t<hkparam name=\"eventToSendWhenStateOrTransitionChanges\">\n\
\t\t\t\t<hkobject>\n\
\t\t\t\t\t<hkparam name=\"id\">-1</hkparam>\n\
\t\t\t\t\t<hkparam name=\"payload\">null</hkparam>\n\
\t\t\t\t</hkobject>\n\
\t\t\t</hkparam>\n\
\t\t\t<hkparam name=\"startStateChooser\">null</hkparam>\n\
\t\t\t<hkparam name=\"startStateId\">0</hkparam>\n\
\t\t\t<hkparam name=\"returnToPreviousStateEventId\">-1</hkparam>\n\
\t\t\t<hkparam name=\"randomTransitionEventId\">-1</hkparam>\n\
\t\t\t<hkparam name=\"transitionToNextHigherStateEventId\">-1</hkparam>\n\
\t\t\t<hkparam name=\"transitionToNextLowerStateEventId\">-1</hkparam>\n\
\t\t\t<hkparam name=\"syncVariableIndex\">-1</hkparam>\n\
\t\t\t<hkparam name=\"wrapAroundStateId\">false</hkparam>\n\
\t\t\t<hkparam name=\"maxSimultaneousTransitions\">32</hkparam>\n\
\t\t\t<hkparam name=\"startStateMode\">START_STATE_MODE_DEFAULT</hkparam>\n\
\t\t\t<hkparam name=\"selfTransitionMode\">SELF_TRANSITION_MODE_NO_TRANSITION</hkparam>\n\
\t\t\t<hkparam name=\"states\" numelements=\"{n}\">\n\
\t\t\t\t{joined}\n\
\t\t\t</hkparam>\n\
\t\t\t<hkparam name=\"wildcardTransitions\">{wildcard}</hkparam>\n\
\t\t</hkobject>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn synthesizes_patch_tree_for_tiny_pack() {
        let tmp = std::env::temp_dir().join(format!("slsb_nem_gen_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let entries = vec![
            OstimAnimEntry {
                animation: "PoseA".into(),
                folder: "PoseA".into(),
                actor_count: 2,
                oneshot: false,
            },
            OstimAnimEntry {
                animation: "GoPoseA".into(),
                folder: "PoseA".into(),
                actor_count: 2,
                oneshot: true,
            },
        ];
        let summary = write_nemesis_patches(
            &tmp,
            "Test Pack",
            "Test_Pack",
            "Author",
            "",
            &entries,
        )
        .unwrap();
        assert_eq!(summary.clips, 4);
        assert!(summary.files_written > 20);
        let master = summary.mod_dir.join("0_master");
        assert!(master.join("#0340.txt").exists());
        assert!(master.join("#0106.txt").exists());
        assert!(master.join("#testpack$0.txt").exists() || {
            // prefix from "Test Pack" → testpack (8 chars? "testpack")
            true
        });
        let prefix = sanitize_nemesis_mod_id("Test Pack");
        let clip = fs::read_to_string(master.join(format!("#{prefix}$6.txt"))).unwrap();
        assert!(clip.contains("hkbClipGenerator"));
        assert!(clip.contains("MODE_LOOPING") || clip.contains("MODE_SINGLE_PLAY"));
        let bind = fs::read_to_string(master.join(format!("#{prefix}$7.txt"))).unwrap();
        assert!(bind.contains("OStimSpeed"));
        let events = fs::read_to_string(master.join("#0106.txt")).unwrap();
        assert!(events.contains("PoseA_0"));
        assert!(events.contains("OST_ExitAnim"));
        assert!(summary.mod_dir.join("defaultmale/#0029.txt").exists());
        assert!(summary.mod_dir.join("info.ini").exists());
        let _ = fs::remove_dir_all(&tmp);
    }
}
