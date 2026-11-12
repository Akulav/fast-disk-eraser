//! The window: state, title bar, sidebar, running jobs, dialogs and toasts. The pages themselves
//! live in [`super::pages`].

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Frame, Id, Layout, Margin, Rect, RichText, Sense, Stroke, StrokeKind};
use egui_phosphor::regular as ph;

use super::theme::*;
use super::widgets::*;
use crate::app::install::{self, Offer, StartupInfo};
use crate::app::paths::Paths;
use crate::app::settings::{self, HistoryEntry, JobKind, Settings};
use crate::app::update::{self, UpdateInfo};
use crate::core::drives::{self, Drive};
use crate::core::erase::{self, EraseOptions, Failure};
use crate::core::freespace::{self, FreeSpaceOptions};
use crate::core::method::Method;
use crate::core::progress::Progress;
use crate::core::{format_bytes, format_duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Erase,
    FreeSpace,
    History,
    Settings,
}

pub struct QueueItem {
    pub path: PathBuf,
    pub is_dir: bool,
    /// (bytes, files) once measured.
    pub size: Option<(u64, u64)>,
}

pub enum UpdateState {
    None,
    Checking,
    UpToDate,
    Available(UpdateInfo),
    Downloading { done: u64, total: u64 },
    Ready { path: PathBuf, version: String },
    Failed(String),
}

enum Msg {
    Measured(PathBuf, u64, u64),
    UpdateChecked { result: Result<Option<UpdateInfo>, String>, manual: bool },
    UpdateProgress(u64, u64),
    UpdateDownloaded(Result<PathBuf, String>, String),
}

enum JobDone {
    Files(erase::Outcome),
    Free(Result<freespace::FreeOutcome, String>),
}

pub struct Job {
    pub kind: JobKind,
    pub method: Method,
    pub progress: Arc<Progress>,
    pub started: Instant,
    pub drive: Option<String>,
    rx: Receiver<JobDone>,
    sample: (Instant, u64),
    pub speed: f64,
}

/// What the result screen shows after a job.
pub struct JobResult {
    pub kind: JobKind,
    pub ok: bool,
    pub cancelled: bool,
    pub title: String,
    pub subtitle: String,
    pub stats: Vec<(String, String)>,
    pub failures: Vec<Failure>,
}

pub enum Confirm {
    Erase,
    FreeSpace(Drive),
    ClearHistory,
    Uninstall,
}

pub struct Toast {
    pub text: String,
    pub color: Color32,
    pub icon: &'static str,
    pub born: Instant,
}

pub struct EraserApp {
    pub paths: Paths,
    pub settings: Settings,
    pub history: Vec<HistoryEntry>,
    pub page: Page,
    page_since: Instant,
    app_icon: Option<egui::TextureHandle>,

    pub queue: Vec<QueueItem>,
    pub manual_path: String,
    pub drives: Vec<Drive>,
    drives_at: Instant,
    pub selected_drive: Option<PathBuf>,

    pub job: Option<Job>,
    pub result: Option<JobResult>,
    pub show_failures: bool,
    pub confirm: Option<Confirm>,
    pub hold: Option<f64>,

    pub update: UpdateState,
    pub show_update_dialog: bool,
    last_update_check: Option<Instant>,

    install_offer: Offer,
    install_dir_edit: String,
    install_error: Option<String>,

    toasts: Vec<Toast>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    quit_after_job: bool,
    settings_dirty: Option<Instant>,
    exiting: bool,
}

impl EraserApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: StartupInfo) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_style(&cc.egui_ctx);
        let paths = Paths::detect().unwrap_or_else(|_| Paths { root: std::env::current_dir().unwrap_or_default() });
        let settings = Settings::load(&paths);
        let history = settings::load_history(&paths);
        let app_icon = image::load_from_memory(include_bytes!("../../assets/icon.png")).ok().map(|img| {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            cc.egui_ctx.load_texture("app-icon", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
        });
        let (tx, rx) = unbounded();
        let install_dir_edit = match &startup.offer {
            Offer::Install { suggested } => suggested.display().to_string(),
            Offer::Update { root, .. } => root.display().to_string(),
            Offer::None => String::new(),
        };
        let mut app = Self {
            paths,
            settings,
            history,
            page: Page::Erase,
            page_since: Instant::now(),
            app_icon,
            queue: Vec::new(),
            manual_path: String::new(),
            drives: drives::list(),
            drives_at: Instant::now(),
            selected_drive: None,
            job: None,
            result: None,
            show_failures: false,
            confirm: None,
            hold: None,
            update: UpdateState::None,
            show_update_dialog: false,
            last_update_check: None,
            install_offer: startup.offer.clone(),
            install_dir_edit,
            install_error: None,
            toasts: Vec::new(),
            tx,
            rx,
            quit_after_job: false,
            settings_dirty: None,
            exiting: false,
        };
        app.selected_drive = app.drives.first().map(|d| d.mount.clone());
        if let Some(from) = &startup.updated_from {
            app.toast(ph::CHECK_CIRCLE, GREEN, format!("Updated from {from} to {}", update::CURRENT_VERSION));
        } else if startup.installed {
            app.toast(ph::CHECK_CIRCLE, GREEN, "Fast Disk Eraser is installed and ready");
        }
        // Remove the update that was just installed.
        let _ = std::fs::remove_dir_all(app.paths.root.join("cache").join("update"));
        // Fill folders left behind by a wipe that was interrupted by a crash or power loss.
        for d in &app.drives {
            for loc in drives::fill_locations(d) {
                let n = freespace::remove_leftovers(&loc);
                if n > 0 {
                    app.toasts.push(Toast { text: format!("Removed {n} leftover wipe folder(s) on {}", d.label), color: BLUE, icon: ph::BROOM, born: Instant::now() });
                }
            }
        }
        if app.settings.check_updates && install::running_installed() {
            app.check_updates(false);
        }
        app
    }

    // -----------------------------------------------------------------------------------------
    // Helpers used by the pages
    // -----------------------------------------------------------------------------------------

    pub fn toast(&mut self, icon: &'static str, color: Color32, text: impl Into<String>) {
        self.toasts.push(Toast { text: text.into(), color, icon, born: Instant::now() });
    }

    pub fn settings_changed(&mut self) {
        self.settings_dirty = Some(Instant::now());
    }

    fn save_settings_now(&mut self) {
        if let Err(e) = self.settings.save(&self.paths) {
            self.toast(ph::WARNING, DANGER, format!("Could not save settings: {e:#}"));
        }
        self.settings_dirty = None;
    }

    pub fn app_icon(&self) -> Option<&egui::TextureHandle> {
        self.app_icon.as_ref()
    }

    pub fn go(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.page_since = Instant::now();
        }
    }

    /// Folders that must never be erased (the app's own).
    pub fn protected(&self) -> Vec<PathBuf> {
        let mut v = vec![self.paths.root.clone()];
        v.extend(install::program_root());
        v
    }

    pub fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let protected = self.protected();
        let mut added = 0;
        for p in paths {
            let Ok(meta) = std::fs::symlink_metadata(&p) else {
                self.toast(ph::WARNING, DANGER, format!("Not found: {}", p.display()));
                continue;
            };
            if let Some(why) = erase::protected_reason(&p, &protected) {
                self.toast(ph::SHIELD_WARNING, GOLD, why);
                continue;
            }
            // Already covered by a queued folder (or the same path)?
            if self.queue.iter().any(|q| p.starts_with(&q.path)) {
                continue;
            }
            // A queued item inside this new folder becomes redundant.
            self.queue.retain(|q| !q.path.starts_with(&p));
            let is_dir = meta.is_dir() && !erase::is_link(&meta);
            let size = if is_dir {
                let tx = self.tx.clone();
                let path = p.clone();
                std::thread::spawn(move || {
                    let (b, f) = erase::measure(&path);
                    let _ = tx.send(Msg::Measured(path, b, f));
                });
                None
            } else {
                Some((meta.len(), 1))
            };
            self.queue.push(QueueItem { path: p, is_dir, size });
            added += 1;
        }
        if added > 0 {
            self.result = None;
        }
    }

    pub fn refresh_drives(&mut self) {
        self.drives = drives::list();
        self.drives_at = Instant::now();
        if self.selected_drive.as_ref().is_none_or(|m| !self.drives.iter().any(|d| &d.mount == m)) {
            self.selected_drive = self.drives.first().map(|d| d.mount.clone());
        }
    }

    pub fn selected_drive(&self) -> Option<&Drive> {
        self.selected_drive.as_ref().and_then(|m| self.drives.iter().find(|d| &d.mount == m))
    }

    // -----------------------------------------------------------------------------------------
    // Jobs
    // -----------------------------------------------------------------------------------------

    pub fn start_erase(&mut self) {
        let paths: Vec<PathBuf> = self.queue.iter().map(|q| q.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        let opts = EraseOptions { method: self.settings.method, verify: self.settings.verify, rename: self.settings.rename, threads: self.settings.threads };
        let progress = Progress::new();
        progress.set_phase("Looking at the files");
        let (tx, rx) = unbounded();
        let p = progress.clone();
        std::thread::spawn(move || {
            let scan = erase::scan(&paths);
            let _ = tx.send(JobDone::Files(erase::erase(scan, &opts, &p)));
        });
        self.job = Some(Job { kind: JobKind::Files, method: self.settings.method, progress, started: Instant::now(), drive: None, rx, sample: (Instant::now(), 0), speed: 0.0 });
        self.result = None;
        self.show_failures = false;
    }

    pub fn start_free_space(&mut self, drive: Drive) {
        let opts = FreeSpaceOptions { method: self.settings.free_method, clean_file_table: self.settings.clean_file_table };
        let progress = Progress::new();
        progress.set_phase("Preparing");
        let (tx, rx) = unbounded();
        let p = progress.clone();
        let locations = drives::fill_locations(&drive);
        let label = drive.mount.display().to_string();
        std::thread::spawn(move || {
            let free = drives::available_for(&drive.mount).unwrap_or(drive.available);
            let r = freespace::wipe(&locations, free, &opts, &p).map_err(|e| format!("{e:#}"));
            let _ = tx.send(JobDone::Free(r));
        });
        self.job = Some(Job {
            kind: JobKind::FreeSpace,
            method: self.settings.free_method,
            progress,
            started: Instant::now(),
            drive: Some(label),
            rx,
            sample: (Instant::now(), 0),
            speed: 0.0,
        });
        self.result = None;
        self.show_failures = false;
    }

    fn poll_job(&mut self) {
        let Some(job) = &mut self.job else { return };
        let now = Instant::now();
        let dt = now.duration_since(job.sample.0).as_secs_f64();
        if dt >= 0.5 {
            let done = job.progress.bytes_done.load(Relaxed);
            let inst = done.saturating_sub(job.sample.1) as f64 / dt;
            job.speed = if job.speed == 0.0 { inst } else { job.speed * 0.7 + inst * 0.3 };
            job.sample = (now, done);
        }
        let Ok(done) = job.rx.try_recv() else { return };
        let job = self.job.take().unwrap();
        let elapsed = job.started.elapsed();
        let (entry, result) = match done {
            JobDone::Files(out) => {
                if !out.cancelled {
                    self.queue.retain(|q| std::fs::symlink_metadata(&q.path).is_ok());
                }
                let ok = out.failures.is_empty() && !out.cancelled;
                let title = if out.cancelled {
                    "Erase cancelled".to_string()
                } else if ok {
                    format!("{} erased", plural(out.files, "file"))
                } else {
                    format!("{} erased, {} failed", plural(out.files, "file"), out.failures.len())
                };
                let subtitle = if out.cancelled {
                    "Files that were already erased are gone. The file being erased when you cancelled was partly overwritten and is still there.".into()
                } else if ok {
                    "Overwritten, renamed and deleted. There's nothing left to recover.".into()
                } else {
                    "Some items could not be erased. They are probably open in another program or you don't have permission.".into()
                };
                let stats = vec![
                    ("Data destroyed".into(), format_bytes(out.bytes)),
                    ("Files".into(), out.files.to_string()),
                    ("Folders".into(), out.folders.to_string()),
                    ("Method".into(), format!("{} · {}×", job.method.name(), job.method.pass_count())),
                    ("Time".into(), format_duration(elapsed)),
                ];
                let entry = HistoryEntry {
                    when: chrono::Local::now(),
                    kind: JobKind::Files,
                    method: job.method,
                    items: out.files,
                    bytes: out.bytes,
                    seconds: elapsed.as_secs_f64(),
                    failures: out.failures.len() as u64,
                    cancelled: out.cancelled,
                    drive: None,
                };
                (entry, JobResult { kind: JobKind::Files, ok, cancelled: out.cancelled, title, subtitle, stats, failures: out.failures })
            }
            JobDone::Free(r) => {
                let drive = job.drive.clone().unwrap_or_default();
                match r {
                    Ok(out) => {
                        let title = if out.cancelled { "Free-space wipe cancelled".to_string() } else { format!("Free space wiped on {drive}") };
                        let subtitle = if out.cancelled {
                            "The temporary fill files were removed; your free space is back.".into()
                        } else {
                            "Every unused block was overwritten, so files deleted earlier can't be brought back.".into()
                        };
                        let mut stats = vec![
                            ("Overwritten".into(), format_bytes(out.bytes)),
                            ("Method".into(), format!("{} · {}×", job.method.name(), job.method.pass_count())),
                            ("Time".into(), format_duration(elapsed)),
                        ];
                        if out.tiny_files > 0 {
                            stats.push(("File-table records".into(), out.tiny_files.to_string()));
                        }
                        let entry = HistoryEntry {
                            when: chrono::Local::now(),
                            kind: JobKind::FreeSpace,
                            method: job.method,
                            items: 0,
                            bytes: out.bytes,
                            seconds: elapsed.as_secs_f64(),
                            failures: 0,
                            cancelled: out.cancelled,
                            drive: Some(drive),
                        };
                        (entry, JobResult { kind: JobKind::FreeSpace, ok: !out.cancelled, cancelled: out.cancelled, title, subtitle, stats, failures: vec![] })
                    }
                    Err(e) => {
                        let entry = HistoryEntry {
                            when: chrono::Local::now(),
                            kind: JobKind::FreeSpace,
                            method: job.method,
                            items: 0,
                            bytes: job.progress.bytes_done.load(Relaxed),
                            seconds: elapsed.as_secs_f64(),
                            failures: 1,
                            cancelled: false,
                            drive: Some(drive.clone()),
                        };
                        let result = JobResult {
                            kind: JobKind::FreeSpace,
                            ok: false,
                            cancelled: false,
                            title: format!("Could not wipe {drive}"),
                            subtitle: e,
                            stats: vec![],
                            failures: vec![],
                        };
                        (entry, result)
                    }
                }
            }
        };
        if self.settings.keep_history {
            self.history.push(entry);
            if let Err(e) = settings::save_history(&self.paths, &self.history) {
                self.toast(ph::WARNING, DANGER, format!("Could not save history: {e:#}"));
            }
        }
        self.result = Some(result);
        self.refresh_drives();
        if self.quit_after_job {
            self.exiting = true;
        }
    }

    // -----------------------------------------------------------------------------------------
    // Updates
    // -----------------------------------------------------------------------------------------

    pub fn check_updates(&mut self, manual: bool) {
        if matches!(self.update, UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Ready { .. }) {
            return;
        }
        self.update = UpdateState::Checking;
        self.last_update_check = Some(Instant::now());
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = update::check().map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::UpdateChecked { result, manual });
        });
    }

    pub fn download_update(&mut self) {
        let UpdateState::Available(info) = &self.update else { return };
        let info = info.clone();
        self.update = UpdateState::Downloading { done: 0, total: 0 };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let t2 = tx.clone();
            let r = install::download_update(&info, &move |d, t| {
                let _ = t2.send(Msg::UpdateProgress(d, t));
            }, &|| false)
            .map_err(|e| format!("{e:#}"));
            let _ = tx.send(Msg::UpdateDownloaded(r, info.version.clone()));
        });
    }

    pub fn apply_update(&mut self, ctx: &egui::Context) {
        let UpdateState::Ready { path, .. } = &self.update else { return };
        match install::apply_update(path) {
            Ok(()) => {
                self.exiting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.update = UpdateState::Failed(format!("{e:#}")),
        }
    }

    fn handle_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Measured(path, b, f) => {
                    if let Some(q) = self.queue.iter_mut().find(|q| q.path == path) {
                        q.size = Some((b, f));
                    }
                }
                Msg::UpdateChecked { result, manual } => match result {
                    Ok(Some(info)) => {
                        self.toast(ph::ARROW_CIRCLE_UP, GOLD, format!("Version {} is available", info.version));
                        self.update = UpdateState::Available(info);
                    }
                    Ok(None) => {
                        self.update = UpdateState::UpToDate;
                        if manual {
                            self.toast(ph::CHECK_CIRCLE, GREEN, "You have the latest version");
                        }
                    }
                    Err(e) => {
                        self.update = if manual { UpdateState::Failed(e) } else { UpdateState::None };
                    }
                },
                Msg::UpdateProgress(d, t) => {
                    if let UpdateState::Downloading { done, total, .. } = &mut self.update {
                        *done = d;
                        *total = t;
                    }
                }
                Msg::UpdateDownloaded(r, version) => {
                    self.update = match r {
                        Ok(path) => UpdateState::Ready { path, version },
                        Err(e) => UpdateState::Failed(e),
                    };
                }
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Chrome
    // -----------------------------------------------------------------------------------------

    fn title_bar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let drag = ui.interact(rect, Id::new("title_drag"), Sense::click_and_drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        ui.horizontal_centered(|ui| {
            ui.add_space(14.0);
            if let Some(icon) = &self.app_icon {
                ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(20.0, 20.0)));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("FAST DISK").font(black(13.5)).color(TEXT).extra_letter_spacing(1.5));
            ui.label(RichText::new("ERASER").font(semibold(13.0)).color(ACCENT_HI).extra_letter_spacing(1.5));

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if window_button(ui, ph::X, true).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
                if window_button(ui, if max { ph::CORNERS_IN } else { ph::CORNERS_OUT }, false).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
                }
                if window_button(ui, ph::MINUS, false).clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                ui.add_space(10.0);
                let label = match &self.update {
                    UpdateState::Available(i) => Some((format!("Update {} available", i.version), GOLD)),
                    UpdateState::Downloading { done, total, .. } => Some((
                        if *total > 0 { format!("Downloading update {:.0}%", *done as f32 * 100.0 / *total as f32) } else { "Downloading update…".into() },
                        BLUE,
                    )),
                    UpdateState::Ready { .. } => Some(("Restart to update".into(), GREEN)),
                    _ => None,
                };
                if let Some((text, color)) = label {
                    let r = chip(ui, ph::ARROW_CIRCLE_UP, &text, color).interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        self.show_update_dialog = true;
                    }
                }
            });
        });
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0_f32, BORDER));
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        let busy = self.job.is_some();
        let items = [
            (Page::Erase, ph::FILE_X, "Erase files"),
            (Page::FreeSpace, ph::HARD_DRIVES, "Wipe free space"),
            (Page::History, ph::CLOCK_COUNTER_CLOCKWISE, "History"),
            (Page::Settings, ph::GEAR_SIX, "Settings"),
        ];
        for (page, icon, label) in items {
            let badge = match page {
                Page::Erase if !self.queue.is_empty() => Some(self.queue.len().min(99).to_string()),
                Page::Settings if matches!(self.update, UpdateState::Available(_) | UpdateState::Ready { .. }) => Some("1".into()),
                _ => None,
            };
            let running_here = busy && self.job.as_ref().is_some_and(|j| matches!((j.kind, page), (JobKind::Files, Page::Erase) | (JobKind::FreeSpace, Page::FreeSpace)));
            if nav_item(ui, icon, label, self.page == page, badge.as_deref(), running_here).clicked() {
                self.go(page);
            }
            ui.add_space(4.0);
        }

        // Brand card pinned to the bottom.
        let r = ui.max_rect();
        let card = Rect::from_min_max(pos2(r.left() + 4.0, r.bottom() - 84.0), pos2(r.right() - 4.0, r.bottom() - 16.0));
        let p = ui.painter();
        p.rect_filled(card, CornerRadius::same(R_MD), SURFACE_2);
        p.rect_stroke(card, CornerRadius::same(R_MD), Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        if let Some(icon) = &self.app_icon {
            egui::Image::new(icon).paint_at(ui, Rect::from_min_size(card.min + vec2(12.0, 14.0), vec2(40.0, 40.0)));
        }
        p.text(card.min + vec2(62.0, 22.0), Align2::LEFT_CENTER, "Fast Disk Eraser", semibold(13.0), TEXT);
        p.text(card.min + vec2(62.0, 40.0), Align2::LEFT_CENTER, format!("v{}", update::CURRENT_VERSION), regular(12.0), MUTED);
        p.text(card.min + vec2(62.0, 55.0), Align2::LEFT_CENTER, "made by Akulav", semibold(11.0), ACCENT_HI);
    }

    fn resize_handles(&self, ctx: &egui::Context) {
        use egui::viewport::ResizeDirection as D;
        if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let r = ctx.screen_rect();
        let t = 5.0;
        let zones = [
            (Rect::from_min_max(r.left_top(), pos2(r.right(), r.top() + t)), D::North, egui::CursorIcon::ResizeVertical),
            (Rect::from_min_max(pos2(r.left(), r.bottom() - t), r.right_bottom()), D::South, egui::CursorIcon::ResizeVertical),
            (Rect::from_min_max(r.left_top(), pos2(r.left() + t, r.bottom())), D::West, egui::CursorIcon::ResizeHorizontal),
            (Rect::from_min_max(pos2(r.right() - t, r.top()), r.right_bottom()), D::East, egui::CursorIcon::ResizeHorizontal),
            (Rect::from_min_size(r.left_top(), vec2(t * 2.0, t * 2.0)), D::NorthWest, egui::CursorIcon::ResizeNwSe),
            (Rect::from_min_size(pos2(r.right() - t * 2.0, r.top()), vec2(t * 2.0, t * 2.0)), D::NorthEast, egui::CursorIcon::ResizeNeSw),
            (Rect::from_min_size(pos2(r.left(), r.bottom() - t * 2.0), vec2(t * 2.0, t * 2.0)), D::SouthWest, egui::CursorIcon::ResizeNeSw),
            (Rect::from_min_size(r.right_bottom() - vec2(t * 3.0, t * 3.0), vec2(t * 3.0, t * 3.0)), D::SouthEast, egui::CursorIcon::ResizeNwSe),
        ];
        egui::Area::new(Id::new("resize_handles")).order(egui::Order::Debug).fixed_pos(r.min).interactable(true).show(ctx, |ui| {
            for (i, (rect, dir, cursor)) in zones.into_iter().enumerate().rev() {
                let resp = ui.interact(rect, Id::new(("resize", i)), Sense::drag());
                if resp.hovered() || resp.dragged() {
                    ctx.set_cursor_icon(cursor);
                }
                if resp.drag_started() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                }
            }
        });
    }

    fn toasts_ui(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.born.elapsed() < Duration::from_secs(5));
        if self.toasts.is_empty() {
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
        let screen = ctx.screen_rect();
        egui::Area::new(Id::new("toasts")).order(egui::Order::Tooltip).anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -18.0)).interactable(false).show(ctx, |ui| {
            ui.set_max_width(380.0_f32.min(screen.width() - 40.0));
            for t in self.toasts.iter().rev().take(4) {
                let age = t.born.elapsed().as_secs_f32();
                let alpha = (age / 0.15).min(1.0) * ((5.0 - age) / 0.4).clamp(0.0, 1.0);
                ui.set_opacity(alpha);
                Frame::new()
                    .fill(SURFACE_2)
                    .stroke(Stroke::new(1.0_f32, with_alpha(t.color, 90)))
                    .corner_radius(CornerRadius::same(R_MD))
                    .inner_margin(Margin::symmetric(14, 10))
                    .shadow(egui::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(120) })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(t.icon).font(regular(17.0)).color(t.color));
                            ui.add(egui::Label::new(RichText::new(&t.text).font(regular(13.0)).color(TEXT)).wrap());
                        });
                    });
                ui.add_space(6.0);
            }
        });
    }

    // -----------------------------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------------------------

    fn confirm_ui(&mut self, ctx: &egui::Context) {
        let Some(kind) = &self.confirm else { return };
        let mut close = false;
        let mut go = false;
        let (title, body, action, icon): (String, String, String, &str) = match kind {
            Confirm::Erase => {
                let bytes: u64 = self.queue.iter().filter_map(|q| q.size.map(|s| s.0)).sum();
                let files: u64 = self.queue.iter().filter_map(|q| q.size.map(|s| s.1)).sum();
                (
                    format!("Erase {}?", plural(self.queue.len() as u64, "item")),
                    format!(
                        "{} in {} will be overwritten with {} ({}), renamed and deleted.\n\nThis can't be undone. Nothing, including Fast Disk Eraser, can bring them back.",
                        format_bytes(bytes),
                        plural(files, "file"),
                        self.settings.method.name(),
                        plural(self.settings.method.pass_count() as u64, "pass"),
                    ),
                    "Erase permanently".into(),
                    ph::FILE_X,
                )
            }
            Confirm::FreeSpace(d) => (
                format!("Wipe free space on {}?", d.label),
                format!(
                    "About {} of free space will be overwritten with {} ({}). Your files stay untouched.\n\nThe drive is completely full while this runs, so other programs may complain about low disk space. Close programs that are writing to it.",
                    format_bytes(d.available),
                    self.settings.free_method.name(),
                    plural(self.settings.free_method.pass_count() as u64, "pass"),
                ),
                "Wipe free space".into(),
                ph::HARD_DRIVES,
            ),
            Confirm::ClearHistory => ("Clear history?".into(), "All entries on the History page are deleted.".into(), "Clear history".into(), ph::TRASH),
            Confirm::Uninstall => (
                "Uninstall Fast Disk Eraser?".into(),
                if install::UNINSTALL_ASKS_ABOUT_DATA {
                    "The uninstaller opens and asks whether to keep your settings and history.".into()
                } else {
                    "The app, its menu entries and its settings and history are removed from the install folder.".into()
                },
                "Uninstall".into(),
                ph::SIGN_OUT,
            ),
        };
        let hold = self.settings.hold_to_confirm && matches!(kind, Confirm::Erase | Confirm::FreeSpace(_));
        let modal = egui::Modal::new(Id::new("confirm")).frame(
            Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::same(24)),
        );
        let resp = modal.show(ctx, |ui| {
            ui.set_width(440.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).font(regular(26.0)).color(DANGER));
                ui.label(RichText::new(&title).font(bold(19.0)).color(TEXT));
            });
            ui.add_space(8.0);
            ui.add(egui::Label::new(RichText::new(&body).font(regular(13.5)).color(MUTED)).wrap());
            ui.add_space(18.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if hold {
                    if hold_button(ui, &format!("Hold to {}", action.to_lowercase()), vec2(230.0, 40.0), 1.0, &mut self.hold) {
                        go = true;
                    }
                } else if primary_button(ui, &action, vec2(200.0, 40.0), true).clicked() {
                    go = true;
                }
                if ghost_button(ui, "", "Cancel").clicked() {
                    close = true;
                }
            });
        });
        if resp.should_close() {
            close = true;
        }
        if go {
            match self.confirm.take() {
                Some(Confirm::Erase) => self.start_erase(),
                Some(Confirm::FreeSpace(d)) => self.start_free_space(d),
                Some(Confirm::ClearHistory) => {
                    self.history.clear();
                    settings::clear_history(&self.paths);
                    self.toast(ph::CHECK_CIRCLE, GREEN, "History cleared");
                }
                Some(Confirm::Uninstall) => match install::uninstall(false) {
                    Ok(()) => {
                        self.exiting = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.toast(ph::WARNING, DANGER, format!("{e:#}")),
                },
                None => {}
            }
            self.hold = None;
        } else if close {
            self.confirm = None;
            self.hold = None;
        }
    }

    fn update_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_update_dialog {
            return;
        }
        let mut close = false;
        let mut download = false;
        let mut apply = false;
        let modal = egui::Modal::new(Id::new("update")).frame(
            Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::same(24)),
        );
        let resp = modal.show(ctx, |ui| {
            ui.set_width(420.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(ph::ARROW_CIRCLE_UP).font(regular(26.0)).color(GOLD));
                ui.label(RichText::new("Update Fast Disk Eraser").font(bold(19.0)));
            });
            ui.add_space(8.0);
            match &self.update {
                UpdateState::Available(info) => {
                    ui.label(RichText::new(format!("Version {} is available (you have {}).", info.version, update::CURRENT_VERSION)).color(MUTED));
                    if !info.notes_url.is_empty() {
                        ui.hyperlink_to(format!("{}  What's new", ph::ARROW_SQUARE_OUT), &info.notes_url);
                    }
                }
                UpdateState::Downloading { done, total, .. } => {
                    ui.label(RichText::new(format!("Downloading… {} of {}", format_bytes(*done), format_bytes(*total))).color(MUTED));
                    ui.add_space(6.0);
                    progress_bar(ui, if *total > 0 { *done as f32 / *total as f32 } else { 0.0 }, 6.0, BLUE);
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                UpdateState::Ready { version, .. } => {
                    ui.label(RichText::new(format!("Version {version} is downloaded and verified (SHA-256). Restart to install it into the same folder.")).color(MUTED));
                }
                UpdateState::Failed(e) => {
                    ui.label(RichText::new(format!("The update failed: {e}")).color(DANGER));
                }
                _ => {
                    ui.label(RichText::new("No update is available right now.").color(MUTED));
                }
            }
            ui.add_space(18.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                match &self.update {
                    UpdateState::Available(_) => {
                        if primary_button(ui, &format!("{}  Download", ph::DOWNLOAD_SIMPLE), vec2(150.0, 38.0), true).clicked() {
                            download = true;
                        }
                    }
                    UpdateState::Ready { .. } => {
                        let busy = self.job.is_some();
                        if primary_button(ui, &format!("{}  Restart & update", ph::ARROW_CLOCKWISE), vec2(200.0, 38.0), !busy).on_disabled_hover_text("Wait for the running erase to finish").clicked() {
                            apply = true;
                        }
                    }
                    _ => {}
                }
                if ghost_button(ui, "", "Later").clicked() {
                    close = true;
                }
            });
        });
        if resp.should_close() || close {
            self.show_update_dialog = false;
        }
        if download {
            self.download_update();
        }
        if apply {
            self.apply_update(ctx);
        }
    }

    // -----------------------------------------------------------------------------------------
    // Install screen (Linux / macOS, when started from a download)
    // -----------------------------------------------------------------------------------------

    fn install_screen(&mut self, ui: &mut egui::Ui) {
        let offer = self.install_offer.clone();
        let updating = matches!(offer, Offer::Update { .. });
        let (mut go, mut skip) = (false, false);
        ui.add_space((ui.available_height() * 0.5 - 200.0).max(24.0));
        ui.vertical_centered(|ui| {
            Frame::new()
                .fill(SURFACE)
                .stroke(Stroke::new(1.0_f32, BORDER))
                .corner_radius(CornerRadius::same(R_LG))
                .inner_margin(Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(520.0_f32.min(ui.available_width()));
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.horizontal(|ui| {
                            if let Some(icon) = &self.app_icon {
                                ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(46.0, 46.0)));
                            }
                            ui.vertical(|ui| {
                                kicker(ui, &format!("Version {}  ·  made by {}", update::CURRENT_VERSION, update::AUTHOR));
                                ui.label(RichText::new(if updating { "Update Fast Disk Eraser" } else { "Install Fast Disk Eraser" }).font(bold(20.0)));
                            });
                        });
                        ui.add_space(10.0);
                        match &offer {
                            Offer::Install { .. } => {
                                ui.label(RichText::new("Choose a folder. The app and everything it saves (settings, history, updates) go there, and updates keep using it.").color(MUTED));
                                ui.add_space(14.0);
                                ui.label(RichText::new("Install folder").font(semibold(13.0)));
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    let edit = egui::TextEdit::singleline(&mut self.install_dir_edit).desired_width(ui.available_width() - 110.0).font(regular(13.5)).margin(Margin::symmetric(10, 8));
                                    if ui.add(edit).changed() {
                                        self.install_error = None;
                                    }
                                    if ghost_button(ui, ph::FOLDER_OPEN, "Browse").clicked() {
                                        if let Some(dir) = rfd::FileDialog::new().set_title("Install Fast Disk Eraser in…").pick_folder() {
                                            self.install_dir_edit = dir.display().to_string();
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                                let extras = if cfg!(target_os = "macos") {
                                    "Creates Fast Disk Eraser.app in this folder."
                                } else {
                                    "Also adds an app-menu entry, a Desktop shortcut and the fast-disk-eraser command."
                                };
                                ui.label(RichText::new(extras).font(regular(12.0)).color(FAINT));
                            }
                            Offer::Update { root, installed } => {
                                let from = installed.as_deref().map(|v| format!("version {v}")).unwrap_or_else(|| "an older version".into());
                                ui.label(
                                    RichText::new(format!(
                                        "Fast Disk Eraser {from} is installed in {}. Update it to {}? Your settings and history stay where they are.",
                                        root.display(),
                                        update::CURRENT_VERSION
                                    ))
                                    .color(MUTED),
                                );
                            }
                            Offer::None => {}
                        }
                        if let Some(err) = &self.install_error {
                            ui.add_space(8.0);
                            ui.label(RichText::new(err).font(regular(12.5)).color(DANGER));
                        }
                        ui.add_space(20.0);
                        ui.horizontal(|ui| {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let label = if updating { format!("{}  Update", ph::ARROW_CIRCLE_UP) } else { format!("{}  Install", ph::DOWNLOAD_SIMPLE) };
                                if primary_button(ui, &label, vec2(150.0, 38.0), true).clicked() {
                                    go = true;
                                }
                                if ghost_button(ui, "", "Just run it").on_hover_text("Run this copy without installing it").clicked() {
                                    skip = true;
                                }
                            });
                        });
                    });
                });
        });
        if skip {
            self.install_offer = Offer::None;
        }
        if go {
            let result = match &offer {
                Offer::Install { .. } => install::parse_install_dir(&self.install_dir_edit).and_then(|root| install::install_and_launch(&root, &["--installed"]).map_err(|e| format!("{e:#}"))),
                Offer::Update { root, installed } => {
                    let from = installed.clone().unwrap_or_default();
                    install::install_and_launch(root, &["--updated-from", &from]).map_err(|e| format!("{e:#}"))
                }
                Offer::None => Ok(()),
            };
            match result {
                Ok(()) => {
                    self.exiting = true;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Err(e) => self.install_error = Some(e),
            }
        }
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        if !hovering {
            return;
        }
        let screen = ctx.screen_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("drop")));
        p.rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(190));
        let r = screen.shrink(40.0);
        dashed_rect(&p, r, ACCENT);
        p.text(r.center() - vec2(0.0, 28.0), Align2::CENTER_CENTER, ph::FILE_ARROW_DOWN, regular(54.0), ACCENT_HI);
        p.text(r.center() + vec2(0.0, 26.0), Align2::CENTER_CENTER, "Drop to add to the erase list", bold(20.0), TEXT);
        p.text(r.center() + vec2(0.0, 54.0), Align2::CENTER_CENTER, "Nothing is erased until you confirm", regular(13.0), MUTED);
    }
}

pub fn plural(n: u64, word: &str) -> String {
    let w = if n == 1 {
        word.to_string()
    } else if word.ends_with('s') {
        format!("{word}es")
    } else {
        format!("{word}s")
    };
    format!("{n} {w}")
}

/// Shorten a path for display: "…/parent/name".
pub fn short_path(p: &Path) -> String {
    let s = p.display().to_string();
    if s.chars().count() <= 64 {
        return s;
    }
    let comps: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    let tail = comps.iter().rev().take(2).rev().cloned().collect::<Vec<_>>().join(std::path::MAIN_SEPARATOR_STR);
    format!("…{}{tail}", std::path::MAIN_SEPARATOR)
}

fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, active: bool, badge: Option<&str>, running: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let act = ui.ctx().animate_bool_with_time(resp.id.with("a"), active, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(R_MD), lerp_color(with_alpha(Color32::WHITE, (hover * 8.0) as u8), with_alpha(ACCENT, 30), act));
    if act > 0.0 {
        let bar = Rect::from_min_size(pos2(rect.left(), rect.center().y - 9.0 * act), vec2(3.0, 18.0 * act));
        p.rect_filled(bar, CornerRadius::same(2), ACCENT);
    }
    let col = lerp_color(MUTED, TEXT, act.max(hover));
    p.text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, icon, regular(18.0), lerp_color(col, ACCENT_HI, act));
    p.text(pos2(rect.left() + 44.0, rect.center().y), Align2::LEFT_CENTER, label, semibold(13.5), col);
    if running {
        let t = ui.input(|i| i.time) as f32;
        let a = (0.5 + 0.5 * (t * 4.0).sin()) * 255.0;
        p.circle_filled(pos2(rect.right() - 18.0, rect.center().y), 4.0, with_alpha(ACCENT, a as u8));
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    } else if let Some(b) = badge {
        let g = p.layout_no_wrap(b.to_string(), bold(11.0), Color32::WHITE);
        let w = (g.size().x + 12.0).max(20.0);
        let r = Rect::from_center_size(pos2(rect.right() - 12.0 - w / 2.0, rect.center().y), vec2(w, 20.0));
        p.rect_filled(r, CornerRadius::same(255), ACCENT_LO);
        p.galley(r.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl eframe::App for EraserApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [BG.r() as f32 / 255.0, BG.g() as f32 / 255.0, BG.b() as f32 / 255.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_messages();
        self.poll_job();

        // Closing while a job runs: cancel it first (a free-space wipe must clean up its files).
        if ctx.input(|i| i.viewport().close_requested()) && !self.exiting {
            if let Some(job) = &self.job {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                job.progress.cancel();
                self.quit_after_job = true;
            }
        }
        if self.exiting && self.job.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if self.install_offer != Offer::None {
            egui::TopBottomPanel::top("title_bar").exact_height(42.0).frame(Frame::new().fill(SURFACE)).show(ctx, |ui| self.title_bar(ui));
            egui::CentralPanel::default().frame(Frame::new().fill(BG)).show(ctx, |ui| self.install_screen(ui));
            self.toasts_ui(ctx);
            self.resize_handles(ctx);
            return;
        }

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() && self.job.is_none() {
            self.add_paths(dropped);
            self.go(Page::Erase);
        }
        if self.settings.check_updates && install::running_installed() && self.last_update_check.is_some_and(|t| t.elapsed() > Duration::from_secs(6 * 3600)) {
            self.check_updates(false);
        }
        if self.page == Page::FreeSpace && self.job.is_none() && self.drives_at.elapsed() > Duration::from_secs(5) {
            self.refresh_drives();
        }

        egui::TopBottomPanel::top("title_bar").exact_height(42.0).frame(Frame::new().fill(SURFACE)).show(ctx, |ui| self.title_bar(ui));
        egui::SidePanel::left("sidebar")
            .exact_width(220.0)
            .resizable(false)
            .frame(Frame::new().fill(SURFACE).inner_margin(Margin { left: 12, right: 12, top: 0, bottom: 0 }))
            .show(ctx, |ui| {
                let r = ui.max_rect();
                ui.painter().vline(r.right() + 11.5, r.y_range(), Stroke::new(1.0_f32, BORDER));
                self.sidebar(ui)
            });

        let fade = (self.page_since.elapsed().as_secs_f32() / 0.2).min(1.0);
        let fade = 1.0 - (1.0 - fade).powi(3);
        if fade < 1.0 {
            ctx.request_repaint();
        }
        egui::CentralPanel::default().frame(Frame::new().fill(BG).inner_margin(Margin::same(28))).show(ctx, |ui| {
            ui.set_opacity(fade);
            let job_here = self.job.as_ref().is_some_and(|j| matches!((j.kind, self.page), (JobKind::Files, Page::Erase) | (JobKind::FreeSpace, Page::FreeSpace)));
            let result_here = self.result.as_ref().is_some_and(|r| matches!((r.kind, self.page), (JobKind::Files, Page::Erase) | (JobKind::FreeSpace, Page::FreeSpace)));
            if job_here {
                super::pages::job_view(self, ui);
            } else if result_here {
                super::pages::result_view(self, ui);
            } else {
                match self.page {
                    Page::Erase => super::pages::erase_page(self, ui),
                    Page::FreeSpace => super::pages::free_space_page(self, ui),
                    Page::History => super::pages::history_page(self, ui),
                    Page::Settings => super::pages::settings_page(self, ui),
                }
            }
        });

        self.confirm_ui(ctx);
        self.update_dialog(ctx);
        self.drop_overlay(ctx);
        self.toasts_ui(ctx);
        self.resize_handles(ctx);

        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if matches!(self.update, UpdateState::Checking) || self.queue.iter().any(|q| q.size.is_none()) {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(t) = self.settings_dirty {
            if t.elapsed() > Duration::from_millis(500) {
                self.save_settings_now();
            } else {
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.settings_dirty.is_some() {
            let _ = self.settings.save(&self.paths);
        }
    }
}
