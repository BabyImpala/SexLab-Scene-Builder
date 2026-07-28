use crate::furniture::{furniture_label, FURNITURE_GROUPS};
use crate::graph::{self, GraphAction, GraphView};
use crate::graph_layout::{arrange_scene, graph_coords_all_zeros, graph_coords_stacked};
use crate::jobs::{ChannelProgress, JobEvent, JobUi};
use crate::prefs::{Prefs, ThemePref};
use crate::stage_editor::{show_stage_editor, StageEditorAction, StageEditorState};
use crate::tag_tree::{tag_tree_ui, TagTreeState};
use crate::toasts::{ToastKind, Toasts};
use eframe::App;
use egui::{Context, RichText};
use log::{error, info};
use scene_builder_core::project::define::Node as GraphNode;
use scene_builder_core::project::package::{ExportKind, Package};
use scene_builder_core::project::scene::Scene;
use scene_builder_core::project::stage::Stage;
use scene_builder_core::project::NanoID;
use scene_builder_core::Progress;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

const WIKI_URL: &str = "https://slp-community.github.io/SexLab-Wiki/slsb/creating-packs-using-slsb/";
const DISCORD_URL: &str = "https://discord.gg/JPSHb4ebqj";
const PATREON_URL: &str = "https://www.patreon.com/ScrabJoseline";
const KOFI_URL: &str = "https://ko-fi.com/scrab";
const KOFI_MISS_URL: &str = "https://ko-fi.com/misscorruption";
const REPO_URL: &str = "https://github.com/SLP-Community/SexLab-Scene-Builder";

enum DialogResult {
    Open(PathBuf),
    OpenSlal(PathBuf),
    OpenOffset(PathBuf),
    SaveAs(PathBuf),
    ExportDir { path: PathBuf, kind: ExportKind },
    EnrichSlanim(Vec<PathBuf>),
    EnrichFnis(Vec<PathBuf>),
    Cancelled,
}

enum PendingAction {
    New,
    Open,
    ImportSlal,
    Quit,
}

/// Pre-export confirmations (Pandora clip tip and merge warning).
enum ExportConfirm {
    Tip {
        kind: ExportKind,
        dont_show: bool,
    },
    Merge {
        path: PathBuf,
        kind: ExportKind,
        dont_show: bool,
    },
}

pub struct SceneBuilderApp {
    package: Package,
    dirty: bool,
    selected_scene: Option<NanoID>,
    selected_stage: Option<NanoID>,
    prefs: Prefs,
    graph: GraphView,
    stage_editor: Option<StageEditorState>,
    job: JobUi,
    job_rx: Receiver<JobEvent>,
    job_tx: Sender<JobEvent>,
    dialog_rx: Receiver<DialogResult>,
    dialog_tx: Sender<DialogResult>,
    show_close_confirm: bool,
    show_about: bool,
    pending_after_confirm: Option<PendingAction>,
    status: String,
    /// Stage awaiting a target scene in the "Clone to…" modal.
    clone_to: Option<NanoID>,
    clone_to_search: String,
    confirm_clear_canvas: bool,
    confirm_delete_scene: Option<NanoID>,
    export_confirm: Option<ExportConfirm>,
    tag_tree_state: TagTreeState,
    race_keys: Vec<String>,
    toasts: Toasts,
}

impl SceneBuilderApp {
    pub const APP_TITLE: &'static str = "SexLab Scene Builder";

    pub fn new(prefs: Prefs) -> Self {
        let (job_tx, job_rx) = mpsc::channel();
        let (dialog_tx, dialog_rx) = mpsc::channel();
        Self {
            package: Package::new(),
            dirty: false,
            selected_scene: None,
            selected_stage: None,
            prefs,
            graph: GraphView::default(),
            stage_editor: None,
            job: JobUi::default(),
            job_rx,
            job_tx,
            dialog_rx,
            dialog_tx,
            show_close_confirm: false,
            show_about: false,
            pending_after_confirm: None,
            status: String::new(),
            clone_to: None,
            clone_to_search: String::new(),
            confirm_clear_canvas: false,
            confirm_delete_scene: None,
            export_confirm: None,
            tag_tree_state: TagTreeState::default(),
            race_keys: scene_builder_core::racekeys::get_race_keys_string(),
            toasts: Toasts::default(),
        }
    }

    fn window_title(&self) -> String {
        let name = if self.package.pack_name.is_empty() {
            "Untitled"
        } else {
            self.package.pack_name.as_str()
        };
        if self.dirty {
            format!("* {} - {}", name, Self::APP_TITLE)
        } else if self.package.pack_name.is_empty() && self.package.pack_path.as_os_str().is_empty()
        {
            Self::APP_TITLE.to_string()
        } else {
            format!("{} - {}", name, Self::APP_TITLE)
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn request_if_clean(&mut self, action: PendingAction) {
        if self.dirty {
            self.pending_after_confirm = Some(action);
            self.show_close_confirm = true;
        } else {
            self.run_pending(action);
        }
    }

    fn run_pending(&mut self, action: PendingAction) {
        match action {
            PendingAction::New => {
                self.package = Package::new();
                self.dirty = false;
                self.selected_scene = None;
                self.selected_stage = None;
                self.stage_editor = None;
                self.status = "New project".into();
            }
            PendingAction::Open => self.spawn_open_dialog(),
            PendingAction::ImportSlal => self.spawn_slal_dialog(),
            PendingAction::Quit => {
            }
        }
    }

    fn spawn_open_dialog(&self) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter("SLSB Project", &["json"])
                .pick_file();
            let _ = match path {
                Some(p) => tx.send(DialogResult::Open(p)),
                None => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_slal_dialog(&self) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .set_title("Import SLAL pack (folder)")
                .pick_folder();
            let _ = match path {
                Some(p) => tx.send(DialogResult::OpenSlal(p)),
                None => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_offset_dialog(&self) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter("Offset YAML", &["yaml", "yml"])
                .pick_file();
            let _ = match path {
                Some(p) => tx.send(DialogResult::OpenOffset(p)),
                None => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_save_as_dialog(&self) {
        let tx = self.dialog_tx.clone();
        let suggested = if self.package.pack_name.is_empty() {
            "project.slsb.json".into()
        } else {
            format!("{}.slsb.json", self.package.pack_name)
        };
        thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter("SLSB Project", &["json"])
                .set_file_name(&suggested)
                .save_file();
            let _ = match path {
                Some(p) => tx.send(DialogResult::SaveAs(p)),
                None => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_export_dialog(&self, kind: ExportKind) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let path = rfd::FileDialog::new().pick_folder();
            let _ = match path {
                Some(p) => tx.send(DialogResult::ExportDir { path: p, kind }),
                None => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_enrich_slanim(&self) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let paths = rfd::FileDialog::new()
                .add_filter("Source / text", &["txt", "json", "xml"])
                .pick_files();
            let _ = match paths {
                Some(p) if !p.is_empty() => tx.send(DialogResult::EnrichSlanim(p)),
                _ => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn spawn_enrich_fnis(&self) {
        let tx = self.dialog_tx.clone();
        thread::spawn(move || {
            let paths = rfd::FileDialog::new()
                .add_filter("FNIS AnimList", &["txt"])
                .pick_files();
            let _ = match paths {
                Some(p) if !p.is_empty() => tx.send(DialogResult::EnrichFnis(p)),
                _ => tx.send(DialogResult::Cancelled),
            };
        });
    }

    fn save_project(&mut self, save_as: bool) {
        if !save_as && !self.package.pack_path.as_os_str().is_empty() {
            let path = self.package.pack_path.clone();
            match self.package.write(path) {
                Ok(()) => {
                    self.dirty = false;
                    self.status = "Saved".into();
                }
                Err(e) => {
                    self.status = format!("Save failed: {e}");
                    error!("{e}");
                }
            }
        } else {
            self.spawn_save_as_dialog();
        }
    }

    /// Show the Pandora clip tip before export unless the user dismissed it.
    fn request_export(&mut self, kind: ExportKind) {
        if self.prefs.hide_export_clip_tip {
            self.spawn_export_dialog(kind);
        } else {
            self.export_confirm = Some(ExportConfirm::Tip {
                kind,
                dont_show: false,
            });
        }
    }

    /// Warn when soft-merging into a non-empty export folder unless dismissed.
    fn export_dir_chosen(&mut self, path: PathBuf, kind: ExportKind) {
        let (_, write_roots) = self.package.resolve_export_paths(&path, kind);
        let would_merge = write_roots
            .iter()
            .any(|p| scene_builder_core::project::package::dir_nonempty(p));
        if would_merge && !self.prefs.hide_export_merge_warn {
            self.export_confirm = Some(ExportConfirm::Merge {
                path,
                kind,
                dont_show: false,
            });
        } else {
            self.start_export(path, kind);
        }
    }

    fn start_export(&mut self, parent: PathBuf, kind: ExportKind) {
        let pack = self.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Export".into(),
            message: "Starting…".into(),
            fraction: 0.0,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Export");
            progress.set_message("Resolving paths…");
            progress.set_fraction(0.1);
            let (pack_root, _) = pack.resolve_export_paths(&parent, kind);
            progress.set_message(format!("Writing to {}…", pack_root.display()).as_str());
            progress.set_fraction(0.3);
            let result = pack.export_into(&pack_root, kind);
            match result {
                Ok(()) => {
                    progress.set_fraction(1.0);
                    progress.set_message("Done");
                    let _ = tx.send(JobEvent::Finished {
                        ok: true,
                        message: format!("Exported to {}", pack_root.display()),
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_slal_pack_import(&mut self, dir: PathBuf) {
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Import SLAL pack".into(),
            message: "Scanning folder…".into(),
            fraction: 0.1,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Import SLAL pack");
            progress.set_message("Reading pack…");
            match Package::from_slal_pack(dir, Some(&progress))
            {
                Ok(pack) => {
                    let n = pack.scenes.len();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: format!("Imported {n} scene(s) from SLAL pack"),
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_enrich_slanim(&mut self, paths: Vec<PathBuf>) {
        let mut pack = self.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Enrich SLAnim".into(),
            message: "Reading sources…".into(),
            fraction: 0.2,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Enrich SLAnim");
            progress.set_message("Applying…");
            match pack.enrich_from_slanim_paths(&paths) {
                Ok(summary) => {
                    let msg = summary.message();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: msg,
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn start_enrich_fnis(&mut self, paths: Vec<PathBuf>) {
        let mut pack = self.package.clone();
        let tx = self.job_tx.clone();
        self.job = JobUi {
            active: true,
            title: "Enrich FNIS".into(),
            message: "Reading AnimLists…".into(),
            fraction: 0.2,
        };
        thread::spawn(move || {
            let progress = ChannelProgress::new(tx.clone());
            progress.set_title("Enrich FNIS");
            progress.set_message("Applying…");
            match pack.enrich_from_fnis_paths(&paths) {
                Ok(summary) => {
                    let msg = summary.message_fnis();
                    let _ = tx.send(JobEvent::PackageUpdated {
                        package: pack,
                        message: msg,
                    });
                }
                Err(e) => {
                    let _ = tx.send(JobEvent::Finished {
                        ok: false,
                        message: e,
                    });
                }
            }
        });
    }

    fn poll_channels(&mut self, ctx: &Context) {
        while let Ok(ev) = self.job_rx.try_recv() {
            match ev {
                JobEvent::Progress {
                    title,
                    message,
                    fraction,
                } => {
                    self.job.active = true;
                    self.job.title = title;
                    self.job.message = message;
                    self.job.fraction = fraction;
                }
                JobEvent::Finished { ok, message } => {
                    self.job.active = false;
                    self.status = message.clone();
                    if !ok {
                        error!("{message}");
                    } else {
                        info!("{message}");
                    }
                }
                JobEvent::PackageUpdated { package, message } => {
                    self.package = package;
                    self.dirty = true;
                    self.job.active = false;
                    self.stage_editor = None;
                    self.status = message;
                    if let Some(id) = self.package.scenes.keys().next().cloned() {
                        self.select_scene(id);
                    } else {
                        self.selected_scene = None;
                        self.selected_stage = None;
                    }
                }
            }
            ctx.request_repaint();
        }

        while let Ok(ev) = self.dialog_rx.try_recv() {
            match ev {
                DialogResult::Open(path) => match Package::load_from_path(path) {
                    Ok(pack) => {
                        self.package = pack;
                        self.dirty = false;
                        self.stage_editor = None;
                        self.status = format!("Opened {}", self.package.pack_path.display());
                        if let Some(id) = self.package.scenes.keys().next().cloned() {
                            self.select_scene(id);
                        } else {
                            self.selected_scene = None;
                            self.selected_stage = None;
                        }
                    }
                    Err(e) => {
                        self.status = format!("Open failed: {e}");
                        error!("{e}");
                    }
                },
                DialogResult::OpenSlal(path) => self.start_slal_pack_import(path),
                DialogResult::OpenOffset(path) => {
                    match self.package.import_offset_from_path(path) {
                        Ok(()) => {
                            self.dirty = true;
                            self.status = "Imported offsets".into();
                        }
                        Err(e) => {
                            self.status = format!("Offset import failed: {e}");
                            error!("{e}");
                        }
                    }
                }
                DialogResult::SaveAs(path) => match self.package.write(path) {
                    Ok(()) => {
                        self.dirty = false;
                        self.status = "Saved".into();
                    }
                    Err(e) => {
                        self.status = format!("Save failed: {e}");
                        error!("{e}");
                    }
                },
                DialogResult::ExportDir { path, kind } => self.export_dir_chosen(path, kind),
                DialogResult::EnrichSlanim(paths) => self.start_enrich_slanim(paths),
                DialogResult::EnrichFnis(paths) => self.start_enrich_fnis(paths),
                DialogResult::Cancelled => {}
            }
            ctx.request_repaint();
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("New").shortcut_text("Ctrl+N"))
                    .clicked()
                {
                    self.request_if_clean(PendingAction::New);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Open…").shortcut_text("Ctrl+O"))
                    .clicked()
                {
                    self.request_if_clean(PendingAction::Open);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                    .clicked()
                {
                    self.save_project(false);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Save As…").shortcut_text("Ctrl+Shift+S"))
                    .clicked()
                {
                    self.save_project(true);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Import SLAL pack…").clicked() {
                    self.request_if_clean(PendingAction::ImportSlal);
                    ui.close_menu();
                }
                if ui.button("Import Offset…").clicked() {
                    self.spawn_offset_dialog();
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Export SLSB…").clicked() {
                    self.request_export(ExportKind::Slsb);
                    ui.close_menu();
                }
                if ui.button("Export SLAL…").clicked() {
                    self.request_export(ExportKind::Slal);
                    ui.close_menu();
                }
                if ui
                    .add(egui::Button::new("Export Both…").shortcut_text("Ctrl+B"))
                    .clicked()
                {
                    self.request_export(ExportKind::Both);
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    if self.dirty {
                        self.pending_after_confirm = Some(PendingAction::Quit);
                        self.show_close_confirm = true;
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.close_menu();
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui.button("Enrich SLAnim…").clicked() {
                    self.spawn_enrich_slanim();
                    ui.close_menu();
                }
                if ui.button("Enrich FNIS…").clicked() {
                    self.spawn_enrich_fnis();
                    ui.close_menu();
                }
            });
            ui.menu_button("View", |ui| {
                ui.menu_button("Theme", |ui| {
                    for (label, pref) in [
                        ("System", ThemePref::System),
                        ("Light", ThemePref::Light),
                        ("Dark", ThemePref::Dark),
                    ] {
                        if ui
                            .selectable_label(self.prefs.theme == pref, label)
                            .clicked()
                        {
                            self.prefs.theme = pref;
                            pref.apply(ctx);
                            self.prefs.save();
                            ui.close_menu();
                        }
                    }
                });
                #[cfg(windows)]
                {
                    let mut show = self.prefs.show_console;
                    if ui
                        .checkbox(&mut show, "Show console")
                        .on_hover_text("Attach a console window for log output (also: --console)")
                        .changed()
                    {
                        self.prefs.show_console = show;
                        self.prefs.save();
                        if show {
                            let _ = crate::console_win::show();
                            info!("Console enabled");
                        } else {
                            crate::console_win::hide();
                        }
                    }
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Wiki").clicked() {
                    let _ = open::that(WIKI_URL);
                    ui.close_menu();
                }
                if ui.button("About").clicked() {
                    self.show_about = true;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Discord").clicked() {
                    let _ = open::that(DISCORD_URL);
                    ui.close_menu();
                }
                if ui.button("Patreon").clicked() {
                    let _ = open::that(PATREON_URL);
                    ui.close_menu();
                }
                if ui.button("Ko-Fi (Scrab)").clicked() {
                    let _ = open::that(KOFI_URL);
                    ui.close_menu();
                }
                if ui.button("Ko-Fi (Miss Corruption)").clicked() {
                    let _ = open::that(KOFI_MISS_URL);
                    ui.close_menu();
                }
            });
        });
    }

    fn left_panel(&mut self, ui: &mut egui::Ui) {
        crate::theme::fill_width(ui);
        let full = ui.available_width();
        let muted = crate::theme::text_muted(ui.visuals().dark_mode);
        for (value, hint) in [
            (&mut self.package.pack_name, "Package Name"),
            (&mut self.package.pack_author, "Author Name"),
            (&mut self.package.pack_version, "Pack Version"),
        ] {
            if ui
                .add(
                    egui::TextEdit::singleline(value)
                        .hint_text(RichText::new(hint).color(muted).italics())
                        .desired_width(full),
                )
                .changed()
            {
                self.dirty = true;
            }
        }

        ui.separator();

        if ui
            .add(egui::Button::new("＋  New Scene").frame(false))
            .clicked()
        {
            self.add_blank_scene();
        }

        let mut to_delete: Option<NanoID> = None;
        let mut to_select: Option<NanoID> = None;
        let count = self.package.scenes.len();
        let header = if count > 0 {
            format!("Scenes ({count})")
        } else {
            "Scenes".to_string()
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::CollapsingHeader::new(header)
                .default_open(true)
                .show(ui, |ui| {
                    for (id, scene) in &self.package.scenes {
                        let selected = self.selected_scene.as_ref() == Some(id);
                        let label = if scene.name.is_empty() {
                            id.0.clone()
                        } else {
                            scene.name.clone()
                        };
                        // Font-safe glyphs (⚗ is missing from egui's default fonts).
                        let icon = if scene.has_warnings {
                            RichText::new("⚠").color(egui::Color32::RED)
                        } else {
                            RichText::new("◆").color(egui::Color32::from_rgb(17, 175, 17))
                        };
                        ui.horizontal(|ui| {
                            ui.label(icon);
                            let resp = ui
                                .selectable_label(selected, &label)
                                .on_hover_text(&label);
                            if resp.clicked() {
                                to_select = Some(id.clone());
                            }
                            resp.context_menu(|ui| {
                                if ui.button("Edit").clicked() {
                                    to_select = Some(id.clone());
                                    ui.close_menu();
                                }
                                if ui
                                    .button(RichText::new("Delete").color(egui::Color32::RED))
                                    .clicked()
                                {
                                    to_delete = Some(id.clone());
                                    ui.close_menu();
                                }
                            });
                        });
                    }
                });
        });

        if let Some(id) = to_select {
            self.select_scene(id);
        }
        if let Some(id) = to_delete {
            self.confirm_delete_scene = Some(id);
        }
    }

    fn add_blank_scene(&mut self) {
        let mut scene = Scene::default();
        scene.name = format!("Scene {}", self.package.scenes.len() + 1);
        let stage = Stage::new(&scene);
        scene.root = stage.id.clone();
        graph::ensure_graph_node(&mut scene, &stage.id, 0);
        scene.stages.push(stage);
        scene.positions = scene
            .stages
            .first()
            .map(|s| {
                s.positions
                    .iter()
                    .map(|p| p.extract_position_info())
                    .collect()
            })
            .unwrap_or_default();
        let id = scene.id.clone();
        self.package.save_scene(scene);
        self.select_scene(id);
        self.mark_dirty();
    }

    fn select_scene(&mut self, id: NanoID) {
        self.selected_scene = Some(id.clone());
        self.selected_stage = None;
        self.graph.selected = None;
        self.graph.request_fit();
        if let Some(scene) = self.package.get_scene_mut(&id) {
            if graph_coords_stacked(scene) || graph_coords_all_zeros(scene) {
                arrange_scene(scene);
            }
        }
    }

    fn delete_stage_from_scene(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.package.get_scene_mut(scene_id) else {
            return;
        };
        self.graph.push_undo(scene);
        scene.stages.retain(|s| &s.id != stage_id);
        scene.graph.remove(stage_id);
        for node in scene.graph.values_mut() {
            node.dest.retain(|d| d != stage_id);
        }
        if scene.root == *stage_id {
            scene.root = scene
                .stages
                .first()
                .map(|s| s.id.clone())
                .unwrap_or_else(NanoID::new_nanoid);
        }
        if self.selected_stage.as_ref() == Some(stage_id) {
            self.selected_stage = None;
        }
        if self.graph.selected.as_ref() == Some(stage_id) {
            self.graph.selected = None;
        }
        Stage::renumber_auto_names(scene);
        self.mark_dirty();
    }

    fn set_scene_root(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.package.get_scene_mut(scene_id) else {
            return;
        };
        if scene.stages.iter().any(|s| &s.id == stage_id) {
            scene.root = stage_id.clone();
            self.mark_dirty();
        }
    }

    fn center_panel(&mut self, ui: &mut egui::Ui) {
        let Some(scene_id) = self.selected_scene.clone() else {
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.4);
                    ui.label(RichText::new("No scene loaded :(").weak());
                    ui.add_space(8.0);
                    if ui.button("New Scene").clicked() {
                        self.add_blank_scene();
                    }
                });
            });
            return;
        };

        let mut open_editor: Option<NanoID> = None;
        let mut add_stage = false;
        let mut store = false;
        let mut rename: Option<String> = None;
        let mut toolbar_action = crate::graph::GraphAction::None;

        {
            let Some(scene) = self.package.get_scene_mut(&scene_id) else {
                ui.label("Scene missing");
                return;
            };

            // Three fixed strips: name | graph controls | scene actions.
            // Nesting right_to_left + left_to_right still allowed the toolbar
            // to grow under Add Stage (add_overlaps_toolbar stayed true).
            let full = ui.available_rect_before_wrap();
            let row_h = ui.spacing().interact_size.y.max(28.0);
            let right_w = 176.0;
            // Toolbar needs ~340px; 300 let Clear (✕) sit under Add Stage.
            let mid_w = 340.0;
            let left_w = (full.width() - right_w - mid_w).max(100.0);

            let left_rect = egui::Rect::from_min_size(full.min, egui::vec2(left_w, row_h));
            let mid_rect = egui::Rect::from_min_size(
                egui::pos2(left_rect.max.x, full.min.y),
                egui::vec2(mid_w, row_h),
            );
            let right_rect = egui::Rect::from_min_size(
                egui::pos2(mid_rect.max.x, full.min.y),
                egui::vec2((full.width() - left_w - mid_w).max(right_w), row_h),
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(left_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(left_rect);
                    if self.dirty {
                        ui.label(
                            RichText::new("≠")
                                .color(egui::Color32::RED)
                                .size(22.0),
                        )
                        .on_hover_text("Unsaved changes");
                    }
                    let mut name = scene.name.clone();
                    let name_edit = egui::TextEdit::singleline(&mut name)
                        .frame(false)
                        .char_limit(30)
                        .hint_text("Scene Name")
                        .font(egui::TextStyle::Heading)
                        .desired_width((ui.available_width() - 8.0).max(60.0));
                    let output = name_edit.show(ui);
                    if output.response.changed() {
                        rename = Some(name.clone());
                    }
                    if output.response.gained_focus() {
                        if let Some(mut state) =
                            egui::TextEdit::load_state(ui.ctx(), output.response.id)
                        {
                            let range = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(name.chars().count()),
                            );
                            state.cursor.set_char_range(Some(range));
                            state.store(ui.ctx(), output.response.id);
                        }
                    }
                },
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(mid_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(mid_rect);
                    ui.separator();
                    toolbar_action = self.graph.toolbar_ui(ui, scene);
                },
            );

            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(right_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |ui| {
                    ui.set_clip_rect(right_rect);
                    let accent = crate::theme::accent(ui.visuals().dark_mode);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Store").color(egui::Color32::WHITE))
                                .fill(accent),
                        )
                        .clicked()
                    {
                        store = true;
                    }
                    if ui.button("Add Stage").clicked() {
                        add_stage = true;
                    }
                    ui.separator();
                },
            );

            // Reserve the row; strip UIs are clipped so they must not expand width.
            let _ = ui.allocate_rect(
                egui::Rect::from_min_size(full.min, egui::vec2(full.width(), row_h)),
                egui::Sense::hover(),
            );

            ui.separator();
        }

        if let Some(name) = rename {
            if let Some(scene) = self.package.get_scene_mut(&scene_id) {
                scene.name = name;
                self.mark_dirty();
            }
        }
        if store {
            self.store_scene(ui.ctx(), &scene_id);
        }

        let action = {
            let Some(scene) = self.package.get_scene_mut(&scene_id) else {
                return;
            };
            egui::Frame::canvas(ui.style())
                .show(ui, |ui| self.graph.ui(ui, scene))
                .inner
        };

        let action = if !matches!(action, crate::graph::GraphAction::None) {
            action
        } else {
            toolbar_action
        };

        match action {
            GraphAction::None => {}
            GraphAction::Select(id) => {
                self.selected_stage = Some(id.clone());
                self.graph.selected = Some(id);
            }
            GraphAction::OpenEditor(id) => {
                open_editor = Some(id);
            }
            GraphAction::CloneStage(id) => {
                self.clone_stage_in_scene(&scene_id, &id);
            }
            GraphAction::CloneStageTo(id) => {
                self.clone_to = Some(id);
                self.clone_to_search.clear();
            }
            GraphAction::ClearCanvas => {
                self.confirm_clear_canvas = true;
            }
            GraphAction::SetRoot(id) => {
                self.set_scene_root(&scene_id, &id);
            }
            GraphAction::DeleteStage(id) => {
                self.delete_stage_from_scene(&scene_id, &id);
            }
            GraphAction::Arrange => {
                if let Some(scene) = self.package.get_scene_mut(&scene_id) {
                    arrange_scene(scene);
                    self.mark_dirty();
                }
            }
            GraphAction::Dirty => {
                if let Some(id) = self.graph.selected.clone() {
                    self.selected_stage = Some(id);
                }
                self.mark_dirty();
            }
        }

        if add_stage {
            if let Some(id) = self.add_stage_to_scene(&scene_id) {
                open_editor = Some(id);
            }
        }
        if let Some(stage_id) = open_editor {
            self.open_stage_editor(&scene_id, &stage_id);
        }
    }

    /// Adds a stage (optionally linked from the previous last stage). Returns the new id.
    fn add_stage_to_scene(&mut self, scene_id: &NanoID) -> Option<NanoID> {
        let scene = self.package.get_scene_mut(scene_id)?;
        self.graph.push_undo(scene);
        let stage = Stage::new(scene);
        let id = stage.id.clone();
        let idx = scene.stages.len();
        if scene.stages.is_empty() {
            scene.root = id.clone();
        } else if let Some(prev) = scene.stages.last() {
            let prev_id = prev.id.clone();
            graph::ensure_graph_node(scene, &prev_id, idx.saturating_sub(1));
            if let Some(node) = scene.graph.get_mut(&prev_id) {
                if node.dest.is_empty() {
                    node.dest.push(id.clone());
                }
            }
        }
        graph::ensure_graph_node(scene, &id, idx);
        scene.stages.push(stage);
        Stage::renumber_auto_names(scene);
        self.selected_stage = Some(id.clone());
        self.graph.selected = Some(id.clone());
        self.mark_dirty();
        Some(id)
    }

    /// Validate name/root/reachability, toast problems, then persist has_warnings.
    fn store_scene(&mut self, ctx: &Context, scene_id: &NanoID) {
        let Some(scene) = self.package.get_scene(scene_id) else {
            return;
        };
        let mut has_warnings = false;
        let mut do_save = true;

        if scene.name.trim().is_empty() {
            self.toasts.push(
                ctx,
                ToastKind::Error,
                "Missing Name",
                "Add a short, descriptive name to your scene.",
            );
            do_save = false;
        }

        let root_exists = scene.stages.iter().any(|s| s.id == scene.root);
        if !root_exists {
            self.toasts.push(
                ctx,
                ToastKind::Warning,
                "Missing Start Animation",
                "Choose the stage which the scene is supposed to start at.",
            );
            has_warnings = true;
        } else {
            // BFS from root across graph destinations.
            let mut visited = std::collections::HashSet::new();
            let mut queue = vec![scene.root.clone()];
            visited.insert(scene.root.clone());
            while let Some(id) = queue.pop() {
                if let Some(node) = scene.graph.get(&id) {
                    for dest in &node.dest {
                        if scene.stages.iter().any(|s| &s.id == dest)
                            && visited.insert(dest.clone())
                        {
                            queue.push(dest.clone());
                        }
                    }
                }
            }
            if visited.len() < scene.stages.len() {
                self.toasts.push(
                    ctx,
                    ToastKind::Warning,
                    "Unreachable Stages",
                    "Scene contains stages which cannot be reached from the start animation.",
                );
                has_warnings = true;
            }
        }

        if !do_save {
            return;
        }
        if let Some(scene) = self.package.get_scene_mut(scene_id) {
            scene.has_warnings = has_warnings;
        }
        self.status = "Scene stored".into();
    }

    /// Duplicate a stage inside its own scene, offset from the original.
    fn clone_stage_in_scene(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.package.get_scene_mut(scene_id) else {
            return;
        };
        let Some(orig) = scene.get_stage(stage_id) else {
            return;
        };
        let mut copy = orig.clone();
        copy.id = NanoID::new_nanoid();
        if Stage::is_auto_name(&copy.name) {
            copy.name = "Stage 0/0".into();
        }
        let new_id = copy.id.clone();
        let (x, y) = scene
            .graph
            .get(stage_id)
            .map(|n| (n.x + 40.0, n.y + 40.0))
            .unwrap_or((40.0, 40.0));
        self.graph.push_undo(scene);
        scene.graph.insert(
            new_id.clone(),
            GraphNode {
                dest: Vec::new(),
                x,
                y,
            },
        );
        scene.stages.push(copy);
        Stage::renumber_auto_names(scene);
        self.selected_stage = Some(new_id.clone());
        self.graph.selected = Some(new_id);
        self.mark_dirty();
    }

    /// Copy a stage into another scene (Clone to… modal target).
    fn clone_stage_to_scene(
        &mut self,
        ctx: &Context,
        stage_id: &NanoID,
        from_scene: &NanoID,
        to_scene: &NanoID,
    ) {
        let Some(stage) = self
            .package
            .get_scene(from_scene)
            .and_then(|s| s.get_stage(stage_id))
            .cloned()
        else {
            self.toasts.push(
                ctx,
                ToastKind::Error,
                "Clone failed",
                "The source stage no longer exists.",
            );
            return;
        };
        let target_name;
        {
            let Some(target) = self.package.get_scene_mut(to_scene) else {
                return;
            };
            if !target.stages.is_empty() && target.positions.len() != stage.positions.len() {
                let msg = format!(
                    "\"{}\" expects {} positions, the stage has {}.",
                    target.name,
                    target.positions.len(),
                    stage.positions.len()
                );
                self.toasts
                    .push(ctx, ToastKind::Error, "Clone failed", &msg);
                return;
            }
            let mut copy = stage;
            copy.id = NanoID::new_nanoid();
            let idx = target.stages.len();
            graph::ensure_graph_node(target, &copy.id, idx);
            if target.stages.is_empty() {
                target.root = copy.id.clone();
                target.positions = copy
                    .positions
                    .iter()
                    .map(|p| p.extract_position_info())
                    .collect();
            }
            target_name = target.name.clone();
            if Stage::is_auto_name(&copy.name) {
                copy.name = "Stage 0/0".into();
            }
            target.stages.push(copy);
            Stage::renumber_auto_names(target);
        }
        self.mark_dirty();
        self.toasts.push(
            ctx,
            ToastKind::Success,
            "Stage cloned",
            &format!("Added to \"{target_name}\"."),
        );
    }

    fn open_stage_editor(&mut self, scene_id: &NanoID, stage_id: &NanoID) {
        let Some(scene) = self.package.get_scene(scene_id) else {
            return;
        };
        let Some(stage) = scene.get_stage(stage_id) else {
            return;
        };
        self.stage_editor = Some(StageEditorState::new(
            scene_id.clone(),
            stage.clone(),
            scene.positions.clone(),
        ));
    }

    /// Right column: Scene Tags + Furniture.
    fn tags_furniture_panel(&mut self, ui: &mut egui::Ui) {
        let Some(scene_id) = self.selected_scene.clone() else {
            return;
        };
        let panel_w = ui.available_width();
        ui.set_max_width(panel_w);
        ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));

        let furniture_panel = egui::TopBottomPanel::bottom("furniture_section")
            .resizable(true)
            .default_height(self.prefs.furniture_panel_height)
            .height_range(120.0..=420.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 4)))
            .show_inside(ui, |ui| {
                ui.set_max_width(panel_w);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Furniture").strong());
                    crate::theme::info_tip(ui, "Furniture settings for the scene.");
                });
                egui::ScrollArea::vertical()
                    .id_salt("furniture_scroll")
                    .auto_shrink([false, false])
                    .hscroll(false)
                    .show(ui, |ui| {
                        ui.set_max_width(panel_w);
                        self.furniture_section(ui, &scene_id);
                    });
            });
        let furni_h = furniture_panel.response.rect.height();
        if furni_h >= 120.0 && (furni_h - self.prefs.furniture_panel_height).abs() > 1.0 {
            self.prefs.furniture_panel_height = furni_h;
            self.prefs.save();
        }

        let mut copy_to_stages = false;
        ui.label(RichText::new("Scene Tags").strong());
        ui.horizontal_wrapped(|ui| {
            ui.set_max_width(panel_w);
            let has_stages = self
                .package
                .get_scene(&scene_id)
                .map(|s| !s.stages.is_empty())
                .unwrap_or(false);
            if ui
                .add_enabled(has_stages, egui::Button::new("Copy").small())
                .on_hover_text(
                    "Copy scene tags onto every stage (replaces each stage's tags).",
                )
                .clicked()
            {
                copy_to_stages = true;
            }
            crate::theme::info_tip(
                ui,
                "Tags which are shared between all stages in the scene.",
            );
        });

        egui::ScrollArea::vertical()
            .id_salt("scene_tags_scroll")
            .auto_shrink([false, false])
            .hscroll(false)
            .max_width(panel_w)
            .show(ui, |ui| {
                ui.set_max_width(panel_w);
                ui.set_min_width(0.0);

                let mut tags_changed = false;
                let mut custom_changed = false;
                if let Some(scene) = self.package.get_scene_mut(&scene_id) {
                    let result = tag_tree_ui(
                        ui,
                        "scene_tags",
                        &mut self.tag_tree_state,
                        &mut scene.tags,
                        &mut self.prefs.custom_tags,
                    );
                    tags_changed = result.tags_changed;
                    custom_changed = result.custom_changed;
                }
                if copy_to_stages {
                    if let Some(scene) = self.package.get_scene_mut(&scene_id) {
                        let copied = scene.tags.clone();
                        for stage in &mut scene.stages {
                            stage.tags = copied.clone();
                        }
                    }
                    self.mark_dirty();
                }
                if tags_changed {
                    self.mark_dirty();
                }
                if custom_changed {
                    self.prefs.save();
                }
            });
    }

    fn furniture_section(&mut self, ui: &mut egui::Ui, scene_id: &NanoID) {
        let mut furni_changed = false;
        if let Some(scene) = self.package.get_scene_mut(scene_id) {
            let furniture = &mut scene.furniture;
            let selected_label = {
                let names: Vec<&str> = furniture
                    .furni_types
                    .iter()
                    .map(|t| furniture_label(t))
                    .collect();
                if names.is_empty() {
                    "None".to_string()
                } else {
                    names.join(", ")
                }
            };
            egui::ComboBox::from_id_salt("furniture_select")
                .width(ui.available_width())
                .selected_text(selected_label)
                .show_ui(ui, |ui| {
                    let mut none_on = furniture.furni_types.iter().any(|t| t == "None");
                    if ui.checkbox(&mut none_on, "None").changed() {
                        furniture.furni_types = vec!["None".into()];
                        furni_changed = true;
                    }
                    for group in FURNITURE_GROUPS {
                        ui.label(RichText::new(group.label).small());
                        for (label, value) in group.options {
                            let mut on = furniture.furni_types.iter().any(|t| t == value);
                            if ui.checkbox(&mut on, *label).changed() {
                                if on {
                                    furniture.furni_types.retain(|t| t != "None");
                                    furniture.furni_types.push((*value).to_string());
                                    furniture.allow_bed = false;
                                } else {
                                    furniture.furni_types.retain(|t| t != value);
                                    if furniture.furni_types.is_empty() {
                                        furniture.furni_types = vec!["None".into()];
                                    }
                                }
                                furni_changed = true;
                            }
                        }
                    }
                });

            let none_selected = furniture.furni_types.iter().any(|t| t == "None");
            let mut allow_bed = furniture.allow_bed;
            if ui
                .add_enabled(none_selected, egui::Checkbox::new(&mut allow_bed, "Allow Bed"))
                .changed()
            {
                furniture.allow_bed = allow_bed;
                furni_changed = true;
            }
            let mut private = scene.private;
            if ui.checkbox(&mut private, "Private").changed() {
                scene.private = private;
                furni_changed = true;
            }

            ui.add_space(4.0);
            // Avoid ui.columns — it expands the parent when column content
            // exceeds the soft max (was blowing the right panel to ~2k px).
            egui::Grid::new("furniture_offset_grid")
                .num_columns(2)
                .spacing([8.0, 4.0])
                .min_col_width(((ui.available_width() - 8.0) / 2.0).max(40.0))
                .show(ui, |ui| {
                    let offset = &mut scene.furniture.offset;
                    let fields: [(&str, &mut f32, Option<std::ops::RangeInclusive<f32>>); 4] = [
                        ("X", &mut offset.x, None),
                        ("Y", &mut offset.y, None),
                        ("Z", &mut offset.z, None),
                        ("°", &mut offset.r, Some(0.0..=359.9_f32)),
                    ];
                    for (i, (label, value, clamp)) in fields.into_iter().enumerate() {
                        let mut drag = egui::DragValue::new(value)
                            .speed(0.1)
                            .fixed_decimals(1);
                        if let Some(range) = clamp {
                            drag = drag.range(range);
                        }
                        if crate::theme::labeled_drag(ui, label, drag).changed() {
                            furni_changed = true;
                        }
                        if i % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        }
        if furni_changed {
            self.mark_dirty();
        }
    }

    /// Bottom panel: actor slots shared by every stage in the scene.
    fn positions_panel(&mut self, ui: &mut egui::Ui) {
        let Some(scene_id) = self.selected_scene.clone() else {
            return;
        };
        crate::theme::fill_width(ui);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scene Positions").strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::theme::info_tip(
                    ui,
                    "Position data shared between all stages in the scene.",
                );
            });
        });

        let mut changed = false;
        let panel_w = ui.available_width().max(0.0);
        let panel_h = ui.available_height().max(0.0);
        ui.set_max_width(panel_w);

        let _ = egui::ScrollArea::vertical()
            .id_salt("scene_positions_scroll")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .max_width(panel_w)
            .show(ui, |ui| {
                ui.set_width(panel_w);
                ui.set_max_width(panel_w);
                let Some(scene) = self.package.get_scene_mut(&scene_id) else {
                    return;
                };
                if scene.positions.is_empty() {
                    ui.label(RichText::new(
                        "No positions yet — use \"Add Stage\" or add a position from the stage editor.",
                    ).weak());
                    return;
                }

                let n = scene.positions.len().clamp(1, 5);
                let gap = if n <= 2 {
                    12.0
                } else if n <= 3 {
                    10.0
                } else {
                    8.0
                };
                let budget = (panel_w - 2.0).max(0.0);
                let card_w = ((budget - gap * (n.saturating_sub(1) as f32)) / n as f32)
                    .floor()
                    .max(80.0);
                let card_h = panel_h.max(128.0);

                let roomy = card_w >= 240.0;
                let pad = if card_w >= 280.0 {
                    10.0
                } else if card_w >= 200.0 {
                    8.0
                } else {
                    6.0
                };

                // One allocation for the row — avoids double-counting height from
                // scope_builder max_rects + a second allocate_exact_size.
                ui.allocate_ui_with_layout(
                    egui::vec2(budget, card_h),
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.set_min_height(card_h);
                        ui.set_max_height(card_h);
                        for idx in 0..n {
                            let info = &mut scene.positions[idx];
                            ui.allocate_ui_with_layout(
                                egui::vec2(card_w, card_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_min_size(egui::vec2(card_w, card_h));
                                    ui.set_max_size(egui::vec2(card_w, card_h));
                                    egui::Frame::group(ui.style())
                                        .inner_margin(egui::Margin::same(pad as i8))
                                        .show(ui, |ui| {
                                            let inner_w = (card_w - pad * 2.0).max(1.0);
                                            ui.set_max_width(inner_w);

                                            ui.label(
                                                RichText::new(format!("Position {}", idx + 1))
                                                    .small()
                                                    .weak(),
                                            );

                                            let is_human = info.race == "Human";
                                            let combo_w = ui.available_width().max(64.0);

                                            egui::ComboBox::from_id_salt(("position_race", idx))
                                                .width(combo_w)
                                                .selected_text(info.race.clone())
                                                .show_ui(ui, |ui| {
                                                    for key in &self.race_keys {
                                                        if ui
                                                            .selectable_label(
                                                                &info.race == key,
                                                                key,
                                                            )
                                                            .clicked()
                                                        {
                                                            info.race = key.clone();
                                                            if key != "Human" {
                                                                info.sex.futa = false;
                                                                info.vampire = false;
                                                            }
                                                            changed = true;
                                                        }
                                                    }
                                                });

                                            ui.add_space(4.0);
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing =
                                                    egui::vec2(6.0, 4.0);
                                                changed |= crate::theme::sex_radios(
                                                    ui,
                                                    &mut info.sex,
                                                    is_human,
                                                );
                                            });

                                            ui.add_space(2.0);
                                            ui.separator();
                                            ui.add_space(2.0);

                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing =
                                                    egui::vec2(6.0, 4.0);
                                                changed |= crate::theme::state_flags(
                                                    ui,
                                                    &mut info.submissive,
                                                    &mut info.vampire,
                                                    &mut info.dead,
                                                    is_human,
                                                    roomy,
                                                );
                                            });

                                            ui.add_space(4.0);
                                            ui.horizontal(|ui| {
                                                ui.label("Scale").on_hover_text(
                                                    "Actor scale factor used by SexLab for this position (typically 1.0).",
                                                );
                                                let h = ui.spacing().interact_size.y;
                                                let w = ui.available_width().max(56.0);
                                                changed |= ui
                                                    .add_sized(
                                                        [w, h],
                                                        egui::DragValue::new(&mut info.scale)
                                                            .speed(0.01)
                                                            .range(0.01..=2.0)
                                                            .fixed_decimals(2),
                                                    )
                                                    .changed();
                                            });
                                        });
                                },
                            );
                            if idx + 1 < n {
                                ui.add_space(gap);
                            }
                        }
                    },
                );
            });
        if changed {
            self.mark_dirty();
        }
    }

    fn modals(&mut self, ctx: &Context) {
        if self.show_close_confirm {
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("There are unsaved changes. Continue and discard them?");
                    ui.horizontal(|ui| {
                        if ui.button("Discard").clicked() {
                            self.show_close_confirm = false;
                            self.dirty = false;
                            if let Some(action) = self.pending_after_confirm.take() {
                                match action {
                                    PendingAction::Quit => {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                    other => self.run_pending(other),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_close_confirm = false;
                            self.pending_after_confirm = None;
                        }
                    });
                });
        }

        if let Some(confirm) = self.export_confirm.take() {
            let fnis_mod = self.package.fnis_mod_name();
            let mut keep = Some(confirm);
            match keep.as_mut().unwrap() {
                ExportConfirm::Tip { kind, dont_show } => {
                    let kind = *kind;
                    let mut decided: Option<bool> = None;
                    egui::Window::new("Animation clips for Pandora")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.set_max_width(460.0);
                            ui.label(format!(
                                "Export writes into a subfolder named {fnis_mod} under the folder you pick.\n\n\
                                 It writes AnimLists, Behavior files, and registry data — not your .hkx animation clips.\n\n\
                                 Copy your animation HKX files into:\n\
                                 meshes/actors/<race>/animations/{fnis_mod}/\n\n\
                                 For humans that is usually:\n\
                                 meshes/actors/character/animations/{fnis_mod}/\n\n\
                                 Pandora only plays clips that live in the folder the Behavior references."
                            ));
                            ui.add_space(6.0);
                            ui.checkbox(dont_show, "Don't show this tip again on export");
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Continue").clicked() {
                                    decided = Some(true);
                                }
                                if ui.button("Cancel").clicked() {
                                    decided = Some(false);
                                }
                            });
                        });
                    if let Some(proceed) = decided {
                        let dont_show = matches!(
                            keep.as_ref(),
                            Some(ExportConfirm::Tip { dont_show: true, .. })
                        );
                        if dont_show {
                            self.prefs.hide_export_clip_tip = true;
                            self.prefs.save();
                        }
                        keep = None;
                        if proceed {
                            self.spawn_export_dialog(kind);
                        }
                    }
                }
                ExportConfirm::Merge {
                    path,
                    kind,
                    dont_show,
                } => {
                    let kind = *kind;
                    let path = path.clone();
                    let mut decided: Option<bool> = None;
                    egui::Window::new("Export merge")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.set_max_width(460.0);
                            ui.label(format!(
                                "Export writes into a subfolder named {fnis_mod} and soft-merges with anything already there.\n\n\
                                 Matching files are overwritten. Other files (such as .hkx animation clips) are kept.\n\n\
                                 Continue?"
                            ));
                            ui.add_space(6.0);
                            ui.checkbox(dont_show, "Don't warn about export overwrites again");
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if ui.button("Continue").clicked() {
                                    decided = Some(true);
                                }
                                if ui.button("Cancel").clicked() {
                                    decided = Some(false);
                                }
                            });
                        });
                    if let Some(proceed) = decided {
                        let dont_show = matches!(
                            keep.as_ref(),
                            Some(ExportConfirm::Merge { dont_show: true, .. })
                        );
                        if dont_show {
                            self.prefs.hide_export_merge_warn = true;
                            self.prefs.save();
                        }
                        keep = None;
                        if proceed {
                            self.start_export(path, kind);
                        }
                    }
                }
            }
            self.export_confirm = keep;
        }

        if let Some(scene_id) = self.confirm_delete_scene.clone() {
            let name = self
                .package
                .get_scene(&scene_id)
                .map(|s| {
                    if s.name.is_empty() {
                        s.id.0.clone()
                    } else {
                        s.name.clone()
                    }
                })
                .unwrap_or_default();
            egui::Window::new("Delete scene")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(format!("Delete \"{name}\"? This cannot be undone."));
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Delete").color(egui::Color32::RED))
                            .clicked()
                        {
                            self.package.discard_scene(&scene_id);
                            if self.selected_scene.as_ref() == Some(&scene_id) {
                                self.selected_scene = None;
                                self.selected_stage = None;
                            }
                            self.mark_dirty();
                            self.confirm_delete_scene = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete_scene = None;
                        }
                    });
                });
        }

        if self.confirm_clear_canvas {
            egui::Window::new("Clear canvas")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Remove all stages from this scene? This can be undone.");
                    ui.horizontal(|ui| {
                        if ui.button("Clear").clicked() {
                            self.confirm_clear_canvas = false;
                            if let Some(id) = self.selected_scene.clone() {
                                if let Some(scene) = self.package.get_scene_mut(&id) {
                                    self.graph.push_undo(scene);
                                    scene.stages.clear();
                                    scene.graph.clear();
                                    scene.root = NanoID::new_nanoid();
                                    self.selected_stage = None;
                                    self.graph.selected = None;
                                    self.mark_dirty();
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_clear_canvas = false;
                        }
                    });
                });
        }

        if let Some(stage_id) = self.clone_to.clone() {
            let mut close = false;
            let mut target: Option<NanoID> = None;
            egui::Window::new("Clone stage to…")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.clone_to_search)
                            .hint_text("Search scenes"),
                    );
                    ui.add_space(4.0);
                    let needle = self.clone_to_search.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (id, scene) in &self.package.scenes {
                                if Some(id) == self.selected_scene.as_ref() {
                                    continue;
                                }
                                let name = if scene.name.is_empty() {
                                    id.0.as_str()
                                } else {
                                    scene.name.as_str()
                                };
                                if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
                                    continue;
                                }
                                if ui.selectable_label(false, name).clicked() {
                                    target = Some(id.clone());
                                }
                            }
                        });
                    ui.add_space(4.0);
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            if let Some(to_scene) = target {
                if let Some(from_scene) = self.selected_scene.clone() {
                    self.clone_stage_to_scene(ctx, &stage_id, &from_scene, &to_scene);
                }
                close = true;
            }
            if close {
                self.clone_to = None;
            }
        }

        if self.show_about {
            egui::Window::new("About SexLab Scene Builder")
                .collapsible(false)
                .resizable(false)
                .open(&mut self.show_about)
                .show(ctx, |ui| {
                    ui.label(format!("SexLab Scene Builder {}", env!("CARGO_PKG_VERSION")));
                    ui.label("Apache-2.0 — Scrab and contributors");
                    if ui.link(REPO_URL).clicked() {
                        let _ = open::that(REPO_URL);
                    }
                    ui.separator();
                    ui.label("Third-party: serde-hkx (MIT OR Apache-2.0) for Behavior.hkx packing");
                });
        }

        if self.job.active {
            egui::Window::new(&self.job.title)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(&self.job.message);
                    let progress = egui::ProgressBar::new(self.job.fraction)
                        .show_percentage()
                        .animate(true);
                    ui.add(progress);
                });
        }
    }

    fn handle_stage_editor(&mut self, ctx: &Context) {
        let Some(mut editor) = self.stage_editor.take() else {
            return;
        };
        let action = show_stage_editor(ctx, &mut editor, &mut self.prefs.custom_tags);
        if editor.custom_tags_changed {
            editor.custom_tags_changed = false;
            self.prefs.save();
        }
        match action {
            StageEditorAction::None => {
                if editor.open {
                    self.stage_editor = Some(editor);
                }
            }
            StageEditorAction::Cancel => {
            }
            StageEditorAction::Save => {
                let scene_id = editor.scene_id.clone();
                let stage = editor.draft.clone();
                let infos = editor.positions_info.clone();
                if let Some(scene) = self.package.get_scene_mut(&scene_id) {
                    if let Some(existing) = scene.get_stage_mut(&stage.id) {
                        *existing = stage;
                    } else {
                        let idx = scene.stages.len();
                        graph::ensure_graph_node(scene, &stage.id, idx);
                        scene.stages.push(stage);
                    }
                    scene.positions = infos;
                    self.mark_dirty();
                    self.status = "Stage saved".into();
                }
            }
        }
    }
}

impl App for SceneBuilderApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_channels(ctx);

        if ctx.input(|i| i.viewport().close_requested()) {
            if self.stage_editor.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            } else if self.dirty {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_after_confirm = Some(PendingAction::Quit);
                self.show_close_confirm = true;
            }
        }

        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.window_title()));

        if self.stage_editor.is_none() {
            use egui::{Key, KeyboardShortcut, Modifiers};
            const SAVE_AS: KeyboardShortcut =
                KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
            const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
            const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
            const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
            const EXPORT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
            if ctx.input_mut(|i| i.consume_shortcut(&SAVE_AS)) {
                self.save_project(true);
            } else if ctx.input_mut(|i| i.consume_shortcut(&SAVE)) {
                self.save_project(false);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&NEW)) {
                self.request_if_clean(PendingAction::New);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&OPEN)) {
                self.request_if_clean(PendingAction::Open);
            }
            if ctx.input_mut(|i| i.consume_shortcut(&EXPORT)) {
                self.request_export(ExportKind::Both);
            }
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            self.menu_bar(ui, ctx);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(if self.dirty { "Modified" } else { "Saved" });
                });
            });
        });

        let left_w = self.prefs.left_panel_width;
        let dark = self.prefs.theme.is_dark();
        let panel_stroke = egui::Stroke::new(1.0, crate::theme::border(dark));
        egui::SidePanel::left("left")
            .resizable(true)
            .default_width(left_w)
            .width_range(180.0..=420.0)
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .fill(crate::theme::panel_bg(dark))
                    .stroke(panel_stroke)
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ctx, |ui| {
                self.left_panel(ui);
                let new_w = ui.max_rect().width();
                if (new_w - self.prefs.left_panel_width).abs() > 1.0 {
                    self.prefs.left_panel_width = new_w;
                    self.prefs.save();
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .fill(crate::theme::shell_bg(dark))
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ctx, |ui| {
                // tags/furniture sit right of the graph in the top half.
                if self.selected_scene.is_some() {
                    egui::TopBottomPanel::bottom("scene_positions_panel")
                        .resizable(true)
                        .default_height(self.prefs.bottom_panel_height)
                        .height_range(120.0..=480.0)
                        .frame(
                            egui::Frame::side_top_panel(&ctx.style())
                                .fill(crate::theme::panel_bg(dark))
                                .stroke(panel_stroke)
                                .inner_margin(egui::Margin::same(10)),
                        )
                        .show_inside(ui, |ui| {
                            self.positions_panel(ui);
                            let new_h = ui.max_rect().height();
                            if (new_h - self.prefs.bottom_panel_height).abs() > 1.0 {
                                self.prefs.bottom_panel_height = new_h;
                                self.prefs.save();
                            }
                        });

                    let tags_panel = egui::SidePanel::right("tags_furniture_panel")
                        .resizable(true)
                        .default_width(self.prefs.right_panel_width)
                        .width_range(200.0..=560.0)
                        .frame(
                            egui::Frame::side_top_panel(&ctx.style())
                                .fill(crate::theme::panel_bg(dark))
                                .stroke(panel_stroke)
                                .inner_margin(egui::Margin::same(10)),
                        )
                        .show_inside(ui, |ui| {
                            let w = ui.available_width().max(0.0);
                            let h = ui.available_height().max(0.0);
                            // Pin an exact child region so overflowing content cannot
                            // expand SidePanel's response rect (egui stores that as width).
                            ui.set_min_size(egui::vec2(w, h));
                            ui.set_max_size(egui::vec2(w, h));
                            ui.set_clip_rect(ui.max_rect());
                            ui.allocate_ui_with_layout(
                                egui::vec2(w, h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.set_max_size(egui::vec2(w, h));
                                    ui.set_clip_rect(ui.max_rect());
                                    self.tags_furniture_panel(ui);
                                },
                            );
                        });
                    let new_w = tags_panel.response.rect.width();
                    if new_w >= 200.0
                        && new_w <= 560.0
                        && (new_w - self.prefs.right_panel_width).abs() > 1.0
                    {
                        self.prefs.right_panel_width = new_w;
                        self.prefs.save();
                    }
                }

                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(egui::Margin::same(4)))
                    .show_inside(ui, |ui| {
                        self.center_panel(ui);
                    });
            });

        self.handle_stage_editor(ctx);
        self.modals(ctx);
        self.toasts.ui(ctx);
    }
}
