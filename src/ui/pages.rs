//! The four pages plus the running-job and result screens.

use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;

use egui::{pos2, vec2, Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, Ui};
use egui_phosphor::regular as ph;

use super::app::{plural, short_path, Confirm, EraserApp, UpdateState};
use super::theme::*;
use super::widgets::*;
use crate::app::install;
use crate::app::settings::JobKind;
use crate::app::update;
use crate::core::drives::DriveKind;
use crate::core::method::Method;
use crate::core::{format_bytes, format_duration};

const SIDE_W: f32 = 420.0;

fn header(ui: &mut Ui, kick: &str, title: &str, subtitle: &str) {
    kicker(ui, kick);
    ui.label(RichText::new(title).font(black(26.0)).color(TEXT));
    ui.label(RichText::new(subtitle).font(regular(14.0)).color(MUTED));
    ui.add_space(14.0);
}

/// Method list shared by both erase pages. Returns true when the choice changed.
fn method_picker(ui: &mut Ui, method: &mut Method) -> bool {
    let mut changed = false;
    for m in Method::ALL {
        let badge = if m.pass_count() == 1 { "1 pass".to_string() } else { format!("{} passes", m.pass_count()) };
        let title = if m == Method::Random { format!("{}  ·  recommended", m.name()) } else { m.name().to_string() };
        if option_row(ui, *method == m, &title, &badge, m.description()).clicked() && *method != m {
            *method = m;
            changed = true;
        }
        ui.add_space(2.0);
    }
    changed
}

fn two_columns(ui: &mut Ui, app: &mut EraserApp, left: fn(&mut EraserApp, &mut Ui), right: fn(&mut EraserApp, &mut Ui)) {
    let avail = ui.available_rect_before_wrap();
    let gap = 22.0;
    let side = SIDE_W.min(avail.width() * 0.42);
    let left_rect = Rect::from_min_max(avail.min, pos2(avail.right() - side - gap, avail.bottom()));
    let right_rect = Rect::from_min_max(pos2(avail.right() - side, avail.top()), avail.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(left_rect), |ui| left(app, ui));
    ui.scope_builder(egui::UiBuilder::new().max_rect(right_rect), |ui| {
        ScrollArea::vertical().id_salt("side").auto_shrink([false, false]).show(ui, |ui| right(app, ui));
    });
    ui.allocate_rect(avail, Sense::hover());
}

// ---------------------------------------------------------------------------------------------
// Erase files
// ---------------------------------------------------------------------------------------------

pub fn erase_page(app: &mut EraserApp, ui: &mut Ui) {
    header(ui, "Erase files & folders", "Shred files for good", "Each byte is overwritten, the name is scrambled, then the file is deleted. Nothing is left to recover.");
    two_columns(ui, app, erase_queue, erase_options);
}

fn pick_files(app: &mut EraserApp) {
    if let Some(files) = rfd::FileDialog::new().set_title("Choose files to erase").pick_files() {
        app.add_paths(files);
    }
}

fn pick_folders(app: &mut EraserApp) {
    if let Some(dirs) = rfd::FileDialog::new().set_title("Choose folders to erase").pick_folders() {
        app.add_paths(dirs);
    }
}

fn erase_queue(app: &mut EraserApp, ui: &mut Ui) {
    // Toolbar.
    ui.horizontal(|ui| {
        if ghost_button(ui, ph::FILE_PLUS, "Add files").clicked() {
            pick_files(app);
        }
        if ghost_button(ui, ph::FOLDER_PLUS, "Add folder").clicked() {
            pick_folders(app);
        }
        if !app.queue.is_empty() {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ghost_button(ui, ph::X, "Clear list").clicked() {
                    app.queue.clear();
                }
            });
        }
    });
    ui.add_space(10.0);

    let bottom_h = 46.0;
    let list_h = (ui.available_height() - bottom_h).max(120.0);
    if app.queue.is_empty() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), list_h), Sense::click());
        let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.15);
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(R_LG), lerp_color(SURFACE, SURFACE_2, hover));
        dashed_rect(p, rect.shrink(1.0), lerp_color(BORDER_HI, ACCENT, hover));
        let c = rect.center();
        p.circle_filled(c - vec2(0.0, 46.0), 34.0, with_alpha(ACCENT, (24.0 + 20.0 * hover) as u8));
        p.text(c - vec2(0.0, 46.0), Align2::CENTER_CENTER, ph::FILE_ARROW_DOWN, regular(32.0), ACCENT_HI);
        p.text(c + vec2(0.0, 10.0), Align2::CENTER_CENTER, "Drop files and folders here", bold(18.0), TEXT);
        p.text(c + vec2(0.0, 36.0), Align2::CENTER_CENTER, "or click to choose files", regular(13.0), MUTED);
        p.text(c + vec2(0.0, 58.0), Align2::CENTER_CENTER, "Nothing is erased until you confirm", regular(12.0), FAINT);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            pick_files(app);
        }
    } else {
        Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::same(6)).show(ui, |ui| {
            ui.set_height(list_h - 12.0);
            ScrollArea::vertical().id_salt("queue").auto_shrink([false, false]).show(ui, |ui| {
                let mut remove = None;
                for (i, item) in app.queue.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 50.0), Sense::hover());
                    let p = ui.painter();
                    if resp.hovered() {
                        p.rect_filled(rect, CornerRadius::same(R_MD), SURFACE_2);
                    }
                    let icon = if item.is_dir { ph::FOLDER_SIMPLE } else { ph::FILE };
                    let ic = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), vec2(34.0, 34.0));
                    p.rect_filled(ic, CornerRadius::same(R_SM), with_alpha(if item.is_dir { GOLD } else { BLUE }, 26));
                    p.text(ic.center(), Align2::CENTER_CENTER, icon, regular(18.0), if item.is_dir { GOLD } else { BLUE });
                    let name = item.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| item.path.display().to_string());
                    let text_w = rect.width() - 220.0;
                    let name_g = p.layout(name, semibold(13.5), TEXT, text_w);
                    let name_g = if name_g.rows.len() > 1 { p.layout_no_wrap(truncate(&item.path.file_name().unwrap_or_default().to_string_lossy(), 48), semibold(13.5), TEXT) } else { name_g };
                    p.galley(pos2(rect.left() + 54.0, rect.top() + 8.0), name_g, TEXT);
                    let parent = item.path.parent().map(short_path).unwrap_or_default();
                    p.text(pos2(rect.left() + 54.0, rect.top() + 34.0), Align2::LEFT_CENTER, truncate(&parent, 70), regular(11.5), FAINT);
                    let size = match item.size {
                        Some((b, f)) if item.is_dir => format!("{} · {}", format_bytes(b), plural(f, "file")),
                        Some((b, _)) => format_bytes(b),
                        None => "measuring…".into(),
                    };
                    p.text(pos2(rect.right() - 48.0, rect.center().y), Align2::RIGHT_CENTER, size, semibold(12.5), MUTED);
                    let x = Rect::from_center_size(pos2(rect.right() - 22.0, rect.center().y), vec2(28.0, 28.0));
                    let xr = ui.interact(x, ui.id().with(("rm", i)), Sense::click());
                    let p = ui.painter();
                    if xr.hovered() {
                        p.rect_filled(x, CornerRadius::same(R_SM), with_alpha(DANGER, 40));
                    }
                    p.text(x.center(), Align2::CENTER_CENTER, ph::X, regular(14.0), if xr.hovered() { DANGER } else { FAINT });
                    if xr.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Remove from the list (doesn't erase it)").clicked() {
                        remove = Some(i);
                    }
                }
                if let Some(i) = remove {
                    app.queue.remove(i);
                }
            });
        });
    }

    // Type or paste a path.
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut app.manual_path)
            .hint_text("…or paste a path and press Enter")
            .desired_width(ui.available_width() - 80.0)
            .font(regular(13.0))
            .margin(Margin::symmetric(10, 7));
        let r = ui.add(edit);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ghost_button(ui, ph::PLUS, "Add").clicked() || enter) && !app.manual_path.trim().is_empty() {
            let p = std::path::PathBuf::from(app.manual_path.trim().trim_matches('"'));
            app.add_paths(vec![p]);
            app.manual_path.clear();
        }
    });
}

fn erase_options(app: &mut EraserApp, ui: &mut Ui) {
    card(ui, "Method", ph::SLIDERS, |ui| {
        if method_picker(ui, &mut app.settings.method) {
            app.settings_changed();
        }
    });
    ui.add_space(12.0);
    card(ui, "Options", ph::GEAR_SIX, |ui| {
        let mut changed = toggle_row(ui, "Verify", "Read the last pass back to make sure the disk really stored it.", &mut app.settings.verify);
        ui.add_space(4.0);
        changed |= toggle_row(ui, "Scramble names", "Rename to random names before deleting, so the old name can't be recovered either.", &mut app.settings.rename);
        if changed {
            app.settings_changed();
        }
    });
    ui.add_space(12.0);

    let bytes: u64 = app.queue.iter().filter_map(|q| q.size.map(|s| s.0)).sum();
    let files: u64 = app.queue.iter().filter_map(|q| q.size.map(|s| s.1)).sum();
    let measuring = app.queue.iter().any(|q| q.size.is_none());
    card(ui, "", "", |ui| {
        ui.horizontal(|ui| {
            stat(ui, "Selected", &if measuring { "…".into() } else { format_bytes(bytes) });
            ui.add_space(18.0);
            stat(ui, "Files", &if measuring { "…".into() } else { files.to_string() });
            ui.add_space(18.0);
            stat(ui, "Written", &if measuring { "…".into() } else { format_bytes(crate::core::erase::span(bytes) * app.settings.method.pass_count() as u64) });
        });
        ui.add_space(12.0);
        let n = app.queue.len();
        let label = if n == 0 { "Add something to erase".to_string() } else { format!("{}  Erase {}", ph::LIGHTNING, plural(n as u64, "item")) };
        if primary_button(ui, &label, vec2(ui.available_width(), 46.0), n > 0).clicked() {
            app.confirm = Some(Confirm::Erase);
        }
    });
    ui.add_space(10.0);
    callout(
        ui,
        ph::INFO,
        BLUE,
        "On SSDs and USB sticks the drive itself decides where data lands, so an overwrite can't be guaranteed to hit every old copy. Erasing still removes the file, and the drive's TRIM clears the blocks. For whole SSDs, full-disk encryption is the safest choice.",
    );
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

// ---------------------------------------------------------------------------------------------
// Wipe free space
// ---------------------------------------------------------------------------------------------

pub fn free_space_page(app: &mut EraserApp, ui: &mut Ui) {
    header(ui, "Wipe free space", "Clean up what was deleted before", "Overwrites every unused block of a drive, so files deleted earlier (by any program) can't be recovered. Your files stay untouched.");
    two_columns(ui, app, drive_list, free_options);
}

fn drive_list(app: &mut EraserApp, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(plural(app.drives.len() as u64, "drive")).font(semibold(13.0)).color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ghost_button(ui, ph::ARROW_CLOCKWISE, "Refresh").clicked() {
                app.refresh_drives();
            }
        });
    });
    ui.add_space(8.0);
    ScrollArea::vertical().id_salt("drives").auto_shrink([false, false]).show(ui, |ui| {
        let mut select = None;
        for d in &app.drives {
            let selected = app.selected_drive.as_ref() == Some(&d.mount);
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 96.0), Sense::click());
            let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
            let sel = ui.ctx().animate_bool_with_time(resp.id.with("s"), selected, 0.15);
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::same(R_LG), lerp_color(lerp_color(SURFACE, SURFACE_2, hover), with_alpha(ACCENT, 22), sel));
            p.rect_stroke(rect, CornerRadius::same(R_LG), Stroke::new(1.0_f32, lerp_color(BORDER, ACCENT, sel)), StrokeKind::Inside);
            let icon = if d.removable { ph::USB } else { ph::HARD_DRIVE };
            let ic = Rect::from_center_size(pos2(rect.left() + 38.0, rect.center().y), vec2(48.0, 48.0));
            p.rect_filled(ic, CornerRadius::same(R_MD), with_alpha(lerp_color(BLUE, ACCENT, sel), 30));
            p.text(ic.center(), Align2::CENTER_CENTER, icon, regular(26.0), lerp_color(BLUE, ACCENT_HI, sel));
            let x = rect.left() + 76.0;
            p.text(pos2(x, rect.top() + 22.0), Align2::LEFT_CENTER, &d.label, bold(15.0), TEXT);
            let mut meta = vec![d.mount.display().to_string(), d.fs.clone()];
            match d.kind {
                DriveKind::Ssd => meta.push("SSD".into()),
                DriveKind::Hdd => meta.push("HDD".into()),
                DriveKind::Unknown => {}
            }
            if d.removable {
                meta.push("removable".into());
            }
            if d.read_only {
                meta.push("read-only".into());
            }
            p.text(pos2(x, rect.top() + 42.0), Align2::LEFT_CENTER, meta.join("  ·  "), regular(12.0), MUTED);
            let bar = Rect::from_min_size(pos2(x, rect.top() + 60.0), vec2(rect.right() - x - 20.0, 6.0));
            p.rect_filled(bar, CornerRadius::same(255), with_alpha(Color32::WHITE, 16));
            let used = d.used_fraction();
            let col = if used > 0.9 { DANGER } else { lerp_color(BLUE, ACCENT, sel) };
            p.rect_filled(Rect::from_min_size(bar.min, vec2((bar.width() * used).max(6.0), 6.0)), CornerRadius::same(255), col);
            p.text(pos2(x, rect.top() + 80.0), Align2::LEFT_CENTER, format!("{} free of {}", format_bytes(d.available), format_bytes(d.total)), semibold(12.0), lerp_color(MUTED, TEXT, sel));
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                select = Some(d.mount.clone());
            }
            ui.add_space(8.0);
        }
        if let Some(m) = select {
            app.selected_drive = Some(m);
        }
        if app.drives.is_empty() {
            ui.label(RichText::new("No drives found.").color(MUTED));
        }
    });
}

fn free_options(app: &mut EraserApp, ui: &mut Ui) {
    card(ui, "Method", ph::SLIDERS, |ui| {
        if method_picker(ui, &mut app.settings.free_method) {
            app.settings_changed();
        }
    });
    ui.add_space(12.0);
    card(ui, "Options", ph::GEAR_SIX, |ui| {
        if toggle_row(
            ui,
            "Clean the file table",
            "Also overwrite leftovers of small deleted files, which NTFS keeps inside its file table where a normal fill never reaches.",
            &mut app.settings.clean_file_table,
        ) {
            app.settings_changed();
        }
    });
    ui.add_space(12.0);
    let drive = app.selected_drive().cloned();
    card(ui, "", "", |ui| {
        if let Some(d) = &drive {
            ui.horizontal(|ui| {
                stat(ui, "Free space", &format_bytes(d.available));
                ui.add_space(18.0);
                stat(ui, "To write", &format_bytes(d.available * app.settings.free_method.pass_count() as u64));
            });
            ui.add_space(12.0);
        }
        let enabled = drive.as_ref().is_some_and(|d| !d.read_only && d.available > 0);
        let label = match &drive {
            Some(d) => format!("{}  Wipe free space on {}", ph::BROOM, d.mount.display()),
            None => "Choose a drive".into(),
        };
        if primary_button(ui, &label, vec2(ui.available_width(), 46.0), enabled).clicked() {
            if let Some(d) = drive.clone() {
                app.confirm = Some(Confirm::FreeSpace(d));
            }
        }
    });
    ui.add_space(10.0);
    if drive.as_ref().is_some_and(|d| d.kind == DriveKind::Ssd) {
        callout(
            ui,
            ph::LIGHTNING,
            GOLD,
            "This is an SSD. Its controller hides spare blocks that no program can write to, and TRIM usually wipes deleted blocks on its own. Wiping free space still helps, but one Random pass is plenty; more passes only wear the drive.",
        );
        ui.add_space(8.0);
    }
    callout(ui, ph::INFO, BLUE, "Temporary fill files go into a hidden .fde-wipe folder on the drive and are deleted as soon as the wipe finishes or is cancelled. The drive is full in the meantime.");
}

// ---------------------------------------------------------------------------------------------
// Running job and result
// ---------------------------------------------------------------------------------------------

pub fn job_view(app: &mut EraserApp, ui: &mut Ui) {
    let Some(job) = &app.job else { return };
    let prog = job.progress.clone();
    let done = prog.bytes_done.load(Relaxed);
    let total = prog.bytes_total.load(Relaxed);
    let frac = prog.fraction();
    let elapsed = job.started.elapsed();
    let eta = if job.speed > 1.0 && total > done { Some(Duration::from_secs_f64((total - done) as f64 / job.speed)) } else { None };
    let is_free = job.kind == JobKind::FreeSpace;
    let method = job.method;
    let speed = job.speed;
    let drive = job.drive.clone();

    ui.add_space((ui.available_height() * 0.5 - 270.0).max(0.0));
    ui.vertical_centered(|ui| {
        Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::same(28)).show(ui, |ui| {
            ui.set_width(560.0_f32.min(ui.available_width()));
            ui.vertical_centered(|ui| {
                kicker(ui, if is_free { "Wiping free space" } else { "Erasing" });
                let title = match (&drive, is_free) {
                    (Some(d), true) => format!("Wiping {d}"),
                    _ => format!("{} · {}", method.name(), plural(method.pass_count() as u64, "pass")),
                };
                ui.label(RichText::new(title).font(bold(20.0)).color(TEXT));
                ui.add_space(10.0);
                progress_ring(ui, 200.0, frac, &format_bytes(done));
                ui.add_space(6.0);
                ui.label(RichText::new(prog.phase()).font(semibold(13.5)).color(ACCENT_HI));
                let current = prog.current();
                if !current.is_empty() && !is_free {
                    ui.label(RichText::new(truncate(&current, 80)).font(mono(11.5)).color(FAINT));
                }
            });
            ui.add_space(16.0);
            ui.columns(4, |c| {
                stat(&mut c[0], "Speed", &format!("{}/s", format_bytes(speed as u64)));
                stat(&mut c[1], "Elapsed", &format_duration(elapsed));
                stat(&mut c[2], "Remaining", &eta.map(format_duration).unwrap_or_else(|| "—".into()));
                let items = if is_free {
                    let pass = prog.pass.load(Relaxed).max(1);
                    format!("{pass} / {}", prog.passes.load(Relaxed).max(1))
                } else {
                    format!("{} / {}", prog.items_done.load(Relaxed), prog.items_total.load(Relaxed))
                };
                stat(&mut c[3], if is_free { "Pass" } else { "Items" }, &items);
            });
            ui.add_space(16.0);
            if is_free {
                callout(ui, ph::WARNING, GOLD, "Your drive is temporarily full. That's expected: the space comes back as soon as the wipe finishes or you cancel.");
                ui.add_space(12.0);
            }
            ui.vertical_centered(|ui| {
                if prog.cancelled() {
                    ui.label(RichText::new("Stopping and cleaning up…").font(semibold(13.0)).color(MUTED));
                } else if danger_button(ui, ph::STOP, "Cancel").clicked() {
                    prog.cancel();
                }
            });
        });
    });
}

pub fn result_view(app: &mut EraserApp, ui: &mut Ui) {
    let Some(r) = &app.result else { return };
    let (icon, color) = if r.cancelled {
        (ph::X_CIRCLE, GOLD)
    } else if r.ok {
        (ph::CHECK_CIRCLE, GREEN)
    } else {
        (ph::WARNING_CIRCLE, DANGER)
    };
    let mut dismiss = false;
    let mut retry_failed = false;
    ui.add_space((ui.available_height() * 0.5 - 230.0).max(0.0));
    ui.vertical_centered(|ui| {
        Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::same(28)).show(ui, |ui| {
            ui.set_width(560.0_f32.min(ui.available_width()));
            ui.vertical_centered(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(84.0, 84.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 42.0, with_alpha(color, 30));
                ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, regular(48.0), color);
                ui.add_space(6.0);
                ui.label(RichText::new(&r.title).font(bold(21.0)).color(TEXT));
                ui.add(egui::Label::new(RichText::new(&r.subtitle).font(regular(13.5)).color(MUTED)).wrap());
            });
            if !r.stats.is_empty() {
                ui.add_space(16.0);
                let n = r.stats.len();
                ui.columns(n, |c| {
                    for (i, (k, v)) in r.stats.iter().enumerate() {
                        stat(&mut c[i], k, v);
                    }
                });
            }
            if !r.failures.is_empty() {
                ui.add_space(12.0);
                let label = format!("{}  {} that could not be erased", if app.show_failures { ph::CARET_DOWN } else { ph::CARET_RIGHT }, plural(r.failures.len() as u64, "item"));
                if ui.add(egui::Label::new(RichText::new(label).font(semibold(13.0)).color(DANGER)).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    app.show_failures = !app.show_failures;
                }
                if app.show_failures {
                    Frame::new().fill(SURFACE_2).corner_radius(CornerRadius::same(R_MD)).inner_margin(Margin::same(10)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                            for f in &r.failures {
                                ui.label(RichText::new(f.path.display().to_string()).font(mono(11.5)).color(TEXT));
                                ui.label(RichText::new(&f.error).font(regular(11.5)).color(MUTED));
                                ui.add_space(4.0);
                            }
                        });
                    });
                }
            }
            ui.add_space(18.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, "Done", vec2(130.0, 38.0), true).clicked() {
                    dismiss = true;
                }
                if r.kind == JobKind::Files && !r.failures.is_empty() && ghost_button(ui, ph::ARROW_CLOCKWISE, "Try the failed items again").clicked() {
                    retry_failed = true;
                }
            });
        });
    });
    if retry_failed {
        let paths: Vec<_> = app.result.as_ref().map(|r| r.failures.iter().map(|f| f.path.clone()).filter(|p| std::fs::symlink_metadata(p).is_ok()).collect()).unwrap_or_default();
        app.result = None;
        app.queue.clear();
        app.add_paths(paths);
    } else if dismiss {
        app.result = None;
    }
}

// ---------------------------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------------------------

pub fn history_page(app: &mut EraserApp, ui: &mut Ui) {
    header(ui, "History", "What you've erased", "Only counts and sizes are kept, never file names. A list of what you shredded would defeat the point.");
    let total_bytes: u64 = app.history.iter().map(|h| h.bytes).sum();
    let files: u64 = app.history.iter().map(|h| h.items).sum();
    let wipes = app.history.iter().filter(|h| h.kind == JobKind::FreeSpace).count() as u64;
    ui.horizontal(|ui| {
        for (k, v) in [("Destroyed in total", format_bytes(total_bytes)), ("Files erased", files.to_string()), ("Free-space wipes", wipes.to_string())] {
            Frame::new().fill(SURFACE).stroke(Stroke::new(1.0_f32, BORDER)).corner_radius(CornerRadius::same(R_LG)).inner_margin(Margin::symmetric(18, 14)).show(ui, |ui| {
                ui.set_min_width(170.0);
                stat(ui, k, &v);
            });
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !app.history.is_empty() && danger_button(ui, ph::TRASH, "Clear history").clicked() {
                app.confirm = Some(Confirm::ClearHistory);
            }
        });
    });
    ui.add_space(14.0);
    if app.history.is_empty() {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(ph::CLOCK_COUNTER_CLOCKWISE).font(regular(42.0)).color(FAINT));
            ui.label(RichText::new(if app.settings.keep_history { "Nothing erased yet." } else { "History is turned off in Settings." }).font(semibold(15.0)).color(MUTED));
        });
        return;
    }
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for h in app.history.iter().rev() {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::same(R_MD), if resp.hovered() { SURFACE_2 } else { SURFACE });
            let (icon, col) = match h.kind {
                JobKind::Files => (ph::FILE_X, ACCENT_HI),
                JobKind::FreeSpace => (ph::HARD_DRIVES, BLUE),
            };
            let ic = Rect::from_center_size(pos2(rect.left() + 30.0, rect.center().y), vec2(36.0, 36.0));
            p.rect_filled(ic, CornerRadius::same(R_SM), with_alpha(col, 26));
            p.text(ic.center(), Align2::CENTER_CENTER, icon, regular(19.0), col);
            let title = match h.kind {
                JobKind::Files => format!("{} erased", plural(h.items, "file")),
                JobKind::FreeSpace => format!("Free space wiped on {}", h.drive.as_deref().unwrap_or("a drive")),
            };
            p.text(pos2(rect.left() + 60.0, rect.top() + 19.0), Align2::LEFT_CENTER, title, semibold(13.5), TEXT);
            let sub = format!("{}  ·  {}  ·  {}", h.when.format("%d %b %Y, %H:%M"), h.method.name(), format_duration(Duration::from_secs_f64(h.seconds)));
            p.text(pos2(rect.left() + 60.0, rect.top() + 39.0), Align2::LEFT_CENTER, sub, regular(12.0), MUTED);
            let (status, sc) = if h.cancelled {
                ("Cancelled", GOLD)
            } else if h.failures > 0 {
                ("With errors", DANGER)
            } else {
                ("Completed", GREEN)
            };
            let g = p.layout_no_wrap(status.to_string(), semibold(11.0), sc);
            let chip_r = Rect::from_min_size(pos2(rect.right() - g.size().x - 30.0, rect.center().y - 11.0), g.size() + vec2(16.0, 8.0));
            p.rect_filled(chip_r, CornerRadius::same(255), with_alpha(sc, 26));
            p.galley(chip_r.min + vec2(8.0, 4.0), g, sc);
            p.text(pos2(chip_r.left() - 16.0, rect.center().y), Align2::RIGHT_CENTER, format_bytes(h.bytes), bold(14.0), TEXT);
            ui.add_space(4.0);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------

pub fn settings_page(app: &mut EraserApp, ui: &mut Ui) {
    header(ui, "Settings", "Make it yours", "Defaults, updates and where the app keeps its files.");
    tab_strip(ui, &[(ph::SLIDERS, "Erasing"), (ph::ARROW_CIRCLE_UP, "Updates"), (ph::FOLDER_OPEN, "Storage"), (ph::HEART, "About")], &mut app.settings_tab);
    ui.add_space(14.0);
    let tab = app.settings_tab;
    ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        if tab == 0 { card(ui, "Erasing", ph::SLIDERS, |ui| {
            let mut changed = false;
            ui.horizontal(|ui| {
                let w = ui.available_width() - 300.0; // room for the slider and its value box
                ui.vertical(|ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("Files at the same time").font(semibold(13.5)));
                    ui.label(RichText::new("More is faster on SSDs with many small files. Use 1 for old hard disks.").font(regular(12.0)).color(MUTED));
                });
                changed |= ui.add(egui::Slider::new(&mut app.settings.threads, 1..=8)).changed();
            });
            ui.add_space(6.0);
            changed |= toggle_row(ui, "Hold to confirm", "Hold the erase button for a second instead of clicking it, so nothing is erased by accident.", &mut app.settings.hold_to_confirm);
            ui.add_space(4.0);
            changed |= toggle_row(ui, "Keep history", "Remember what was erased (counts and sizes only) on the History page.", &mut app.settings.keep_history);
            if changed {
                app.settings_changed();
            }
        }); }

        if tab == 1 { card(ui, "Updates", ph::ARROW_CIRCLE_UP, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Version {}", update::CURRENT_VERSION)).font(bold(15.0)));
                    let status = match &app.update {
                        UpdateState::None => "Updates come from GitHub and are verified with SHA-256.".to_string(),
                        UpdateState::Checking => "Checking for updates…".into(),
                        UpdateState::UpToDate => "You have the latest version.".into(),
                        UpdateState::Available(i) => format!("Version {} is available.", i.version),
                        UpdateState::Downloading { done, total, .. } => format!("Downloading… {} of {}", format_bytes(*done), format_bytes(*total)),
                        UpdateState::Ready { version, .. } => format!("Version {version} is ready to install."),
                        UpdateState::Failed(e) => format!("Update failed: {e}"),
                    };
                    let color = if matches!(app.update, UpdateState::Failed(_)) { DANGER } else { MUTED };
                    ui.label(RichText::new(status).font(regular(12.5)).color(color));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| match &app.update {
                    UpdateState::Available(_) | UpdateState::Ready { .. } | UpdateState::Downloading { .. } => {
                        if primary_button(ui, &format!("{}  Update", ph::ARROW_CIRCLE_UP), vec2(130.0, 36.0), true).clicked() {
                            app.show_update_dialog = true;
                        }
                    }
                    UpdateState::Checking => {
                        ui.spinner();
                    }
                    _ => {
                        if ghost_button(ui, ph::ARROW_CLOCKWISE, "Check for updates").clicked() {
                            app.check_updates(true);
                        }
                    }
                });
            });
            if !install::running_installed() {
                ui.add_space(6.0);
                callout(ui, ph::INFO, BLUE, "This copy isn't installed, so it can't update itself. Install it with the setup (Windows) or by running the downloaded file (Linux, macOS).");
            }
            ui.add_space(6.0);
            if toggle_row(ui, "Check automatically", "Look for a new version when the app starts and every few hours.", &mut app.settings.check_updates) {
                app.settings_changed();
            }
        }); }

        if tab == 2 { card(ui, "Storage", ph::FOLDER_OPEN, |ui| {
            ui.label(RichText::new("Everything the app saves stays in its own folder: settings, history, window position and downloaded updates. Nothing goes to AppData, ~/.local/share or the temp folder.").font(regular(12.5)).color(MUTED));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(app.paths.root.display().to_string()).font(mono(12.0)).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ghost_button(ui, ph::ARROW_SQUARE_OUT, "Open").clicked() {
                        let _ = open::that(&app.paths.root);
                    }
                });
            });
            if install::running_installed() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Uninstall").font(semibold(13.5)));
                        ui.label(RichText::new("Removes the app, its shortcuts and what it saved.").font(regular(12.0)).color(MUTED));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if danger_button(ui, ph::SIGN_OUT, "Uninstall").clicked() {
                            app.confirm = Some(Confirm::Uninstall);
                        }
                    });
                });
            }
        }); }

        if tab == 3 { card(ui, "About", ph::HEART, |ui| {
            ui.horizontal(|ui| {
                if let Some(icon) = app.app_icon() {
                    ui.add(egui::Image::new(icon).fit_to_exact_size(vec2(54.0, 54.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Fast Disk Eraser").font(bold(17.0)));
                    ui.label(RichText::new("Made by Akulav. No telemetry, no ads, no accounts.").font(regular(12.5)).color(MUTED));
                    ui.horizontal(|ui| {
                        ui.hyperlink_to(format!("{}  GitHub", ph::GITHUB_LOGO), update::REPO_URL);
                        ui.label(RichText::new("·").color(FAINT));
                        ui.hyperlink_to("Releases", format!("{}/releases", update::REPO_URL));
                        ui.label(RichText::new("·").color(FAINT));
                        ui.hyperlink_to("Report a problem", format!("{}/issues", update::REPO_URL));
                    });
                });
            });
        }); }
        ui.add_space(20.0);
    });
}
