//! Custom-drawn widgets: buttons, chips, toggles, cards, progress bars and rings.

use egui::epaint::{Mesh, PathShape, Vertex, WHITE_UV};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, Frame, Margin, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui};

use super::theme::*;

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

pub fn hgradient(painter: &egui::Painter, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = Mesh::default();
    for (pos, c) in [(rect.left_top(), left), (rect.right_top(), right), (rect.right_bottom(), right), (rect.left_bottom(), left)] {
        mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: c });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
}

/// Soft glow: concentric translucent rounded rects.
pub fn glow(painter: &egui::Painter, rect: Rect, radius: u8, color: Color32, strength: f32) {
    for i in 1u8..=6 {
        let grow = i as f32 * 2.5;
        let a = (strength * (1.0 - i as f32 / 7.0) * 38.0) as u8;
        painter.rect_filled(rect.expand(grow), CornerRadius::same(radius.saturating_add(i * 2)), color.gamma_multiply_u8(a));
    }
}

/// Rounded rect with a horizontal gradient: rounded caps in the end colors, gradient between.
pub fn hgradient_rounded(p: &egui::Painter, rect: Rect, left: Color32, right: Color32, radius: u8) {
    let r = radius as f32;
    if rect.width() <= 2.0 * r {
        p.rect_filled(rect, CornerRadius::same(radius), lerp_color(left, right, 0.5));
        return;
    }
    // Each cap is clipped to its own strip so translucent colors don't overlap (no visible bands).
    let left_strip = Rect::from_min_max(rect.min, pos2(rect.min.x + r, rect.max.y));
    let right_strip = Rect::from_min_max(pos2(rect.max.x - r, rect.min.y), rect.max);
    p.with_clip_rect(left_strip.intersect(p.clip_rect())).rect_filled(
        Rect::from_min_max(rect.min, pos2(rect.min.x + 2.0 * r, rect.max.y)),
        CornerRadius { nw: radius, sw: radius, ne: 0, se: 0 },
        left,
    );
    p.with_clip_rect(right_strip.intersect(p.clip_rect())).rect_filled(
        Rect::from_min_max(pos2(rect.max.x - 2.0 * r, rect.min.y), rect.max),
        CornerRadius { nw: 0, sw: 0, ne: radius, se: radius },
        right,
    );
    hgradient(p, Rect::from_min_max(pos2(rect.min.x + r, rect.min.y), pos2(rect.max.x - r, rect.max.y)), left, right);
}

/// Small rounded label, e.g. "SSD" or "3 passes".
pub fn chip(ui: &mut Ui, icon: &str, text: &str, color: Color32) -> Response {
    let label = if icon.is_empty() { text.to_string() } else { format!("{icon}  {text}") };
    let galley = ui.painter().layout_no_wrap(label, semibold(11.5), color);
    let size = galley.size() + vec2(16.0, 8.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), with_alpha(color, 26));
    p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0_f32, with_alpha(color, 70)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, color);
    resp
}

/// Big ember button with a glow on hover; the main action of a page.
pub fn primary_button(ui: &mut Ui, text: &str, size: egui::Vec2, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hover = ui.ctx().animate_bool_with_time(resp.id, enabled && resp.hovered(), 0.12);
    let p = ui.painter();
    if enabled {
        glow(p, rect, R_MD, ACCENT, 0.3 + 0.5 * hover);
        hgradient_rounded(p, rect, lerp_color(ACCENT, ACCENT_HI, hover * 0.6), lerp_color(ACCENT_LO, ACCENT, hover * 0.4), R_MD);
        p.rect_stroke(rect, CornerRadius::same(R_MD), Stroke::new(1.0_f32, with_alpha(ACCENT_HI, 150)), StrokeKind::Inside);
    } else {
        p.rect_filled(rect, CornerRadius::same(R_MD), SURFACE_3);
    }
    p.text(rect.center(), Align2::CENTER_CENTER, text, bold(14.5), if enabled { Color32::WHITE } else { FAINT });
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A button that must be held down for `secs`; returns true once on completion. The fill
/// sweeps across while it's held and runs back when released early.
pub fn hold_button(ui: &mut Ui, text: &str, size: egui::Vec2, secs: f32, held_since: &mut Option<f64>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let now = ui.input(|i| i.time);
    let down = resp.is_pointer_button_down_on() || (resp.has_focus() && ui.input(|i| i.key_down(egui::Key::Enter) || i.key_down(egui::Key::Space)));
    let mut done = false;
    let t = if down {
        let start = *held_since.get_or_insert(now);
        let t = ((now - start) as f32 / secs).min(1.0);
        if t >= 1.0 {
            done = true;
            *held_since = None;
        }
        ui.ctx().request_repaint();
        t
    } else {
        *held_since = None;
        0.0
    };
    let shown = ui.ctx().animate_value_with_time(resp.id.with("fill"), t, if down { 0.0 } else { 0.25 });
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.12);
    let p = ui.painter();
    glow(p, rect, R_MD, DANGER, 0.3 + 0.5 * hover + shown);
    p.rect_filled(rect, CornerRadius::same(R_MD), with_alpha(DANGER, 40));
    p.rect_stroke(rect, CornerRadius::same(R_MD), Stroke::new(1.0_f32, with_alpha(DANGER, 160)), StrokeKind::Inside);
    if shown > 0.0 {
        let fill = Rect::from_min_max(rect.min, pos2(rect.min.x + rect.width() * shown, rect.max.y));
        p.with_clip_rect(fill).rect_filled(rect, CornerRadius::same(R_MD), lerp_color(ACCENT_LO, DANGER, shown));
    }
    let label = if down { "Keep holding…" } else { text };
    p.text(rect.center(), Align2::CENTER_CENTER, label, bold(14.5), Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    done
}

/// Secondary button: subtle surface, optional icon.
pub fn ghost_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    let label = if icon.is_empty() {
        text.to_string()
    } else if text.is_empty() {
        icon.to_string()
    } else {
        format!("{icon}  {text}")
    };
    let galley = ui.painter().layout_no_wrap(label, semibold(13.0), TEXT);
    let size = vec2(galley.size().x + 26.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(R_MD), with_alpha(Color32::WHITE, (15.0 + 14.0 * hover) as u8));
    p.rect_stroke(rect, CornerRadius::same(R_MD), Stroke::new(1.0_f32, with_alpha(Color32::WHITE, (38.0 + 22.0 * hover) as u8)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, TEXT);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Destructive variant of [`ghost_button`].
pub fn danger_button(ui: &mut Ui, icon: &str, text: &str) -> Response {
    let label = if icon.is_empty() { text.to_string() } else { format!("{icon}  {text}") };
    let galley = ui.painter().layout_no_wrap(label, semibold(13.0), DANGER);
    let size = vec2(galley.size().x + 26.0, 34.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(R_MD), with_alpha(DANGER, (18.0 + 30.0 * hover) as u8));
    p.rect_stroke(rect, CornerRadius::same(R_MD), Stroke::new(1.0_f32, with_alpha(DANGER, 110)), StrokeKind::Inside);
    p.galley(rect.center() - galley.size() / 2.0, galley, lerp_color(DANGER, Color32::WHITE, hover * 0.3));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// iOS-style switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = vec2(38.0, 22.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(255), lerp_color(with_alpha(Color32::WHITE, 34), ACCENT, t));
    p.rect_stroke(rect, CornerRadius::same(255), Stroke::new(1.0_f32, with_alpha(Color32::WHITE, (46.0 * (1.0 - t)) as u8)), StrokeKind::Inside);
    let x = egui::lerp(rect.left() + 11.0..=rect.right() - 11.0, t);
    p.circle_filled(pos2(x, rect.center().y), 8.0, Color32::WHITE);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Title + description on the left, a toggle on the right. Returns true when it changed.
pub fn toggle_row(ui: &mut Ui, title: &str, desc: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let w = ui.available_width() - 50.0;
        ui.vertical(|ui| {
            ui.set_width(w);
            ui.label(RichText::new(title).font(semibold(13.5)).color(TEXT));
            if !desc.is_empty() {
                ui.label(RichText::new(desc).font(regular(12.0)).color(MUTED));
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| changed = toggle(ui, on).changed());
    });
    changed
}

/// Card container with an icon and a title.
pub fn card<R>(ui: &mut Ui, title: &str, icon: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    glass()
        .inner_margin(Margin::same(18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(icon).font(regular(17.0)).color(ACCENT_HI));
                    ui.label(RichText::new(title).font(bold(15.0)).color(TEXT));
                });
                ui.add_space(4.0);
            }
            add(ui)
        })
        .inner
}

/// Rounded, selectable option row (method pickers, etc.).
pub fn option_row(ui: &mut Ui, selected: bool, title: &str, badge: &str, desc: &str) -> Response {
    let width = ui.available_width();
    let desc_galley = ui.painter().layout(desc.to_string(), regular(12.0), MUTED, width - 44.0);
    let height = 30.0 + desc_galley.size().y;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let hover = ui.ctx().animate_bool_with_time(resp.id, resp.hovered(), 0.1);
    let sel = ui.ctx().animate_bool_with_time(resp.id.with("sel"), selected, 0.15);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(R_MD), lerp_color(with_alpha(Color32::WHITE, (8.0 + 8.0 * hover) as u8), with_alpha(ACCENT, 30), sel));
    p.rect_stroke(
        rect,
        CornerRadius::same(R_MD),
        Stroke::new(1.0_f32, lerp_color(with_alpha(Color32::WHITE, 24), with_alpha(ACCENT, 170), sel)),
        StrokeKind::Inside,
    );
    // Radio dot.
    let c = pos2(rect.left() + 18.0, rect.top() + 17.0);
    p.circle_stroke(c, 7.0, Stroke::new(1.5_f32, lerp_color(FAINT, ACCENT, sel)));
    if sel > 0.0 {
        p.circle_filled(c, 3.8 * sel, ACCENT);
    }
    p.text(pos2(rect.left() + 34.0, rect.top() + 17.0), Align2::LEFT_CENTER, title, semibold(13.5), TEXT);
    if !badge.is_empty() {
        let g = p.layout_no_wrap(badge.to_string(), semibold(11.0), lerp_color(MUTED, ACCENT_HI, sel));
        let r = Rect::from_min_size(pos2(rect.right() - g.size().x - 22.0, rect.top() + 8.0), g.size() + vec2(12.0, 6.0));
        p.rect_filled(r, CornerRadius::same(255), with_alpha(Color32::WHITE, 10));
        p.galley(r.min + vec2(6.0, 3.0), g, MUTED);
    }
    p.galley(pos2(rect.left() + 34.0, rect.top() + 28.0), desc_galley, MUTED);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Thin progress bar.
pub fn progress_bar(ui: &mut Ui, fraction: f32, height: f32, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = ui.painter();
    let r = CornerRadius::same(255);
    p.rect_filled(rect, r, with_alpha(Color32::WHITE, 18));
    let w = rect.width() * fraction.clamp(0.0, 1.0);
    if w > 0.5 {
        p.rect_filled(Rect::from_min_size(rect.min, vec2(w.max(height), height)), r, color);
    }
}

fn arc_points(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let steps = ((to - from).abs() * radius / 3.0).ceil().max(2.0) as usize;
    (0..=steps).map(|i| from + (to - from) * i as f32 / steps as f32).map(|a| center + vec2(a.cos(), a.sin()) * radius).collect()
}

/// Circular progress with the percentage in the middle. `fraction = None` spins.
pub fn progress_ring(ui: &mut Ui, size: f32, fraction: Option<f32>, sub: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let radius = size * 0.5 - 10.0;
    let width = 10.0;
    p.circle_stroke(c, radius, Stroke::new(width, with_alpha(Color32::WHITE, 14)));
    let start = -std::f32::consts::FRAC_PI_2;
    let t = ui.input(|i| i.time) as f32;
    let (a0, a1) = match fraction {
        Some(f) => (start, start + std::f32::consts::TAU * f.clamp(0.0, 1.0)),
        None => {
            let s = t * 4.0;
            (s, s + 1.6)
        }
    };
    if a1 - a0 > 0.01 {
        // Glow, then the ember arc with a lighter head.
        for (w, a) in [(width + 10.0, 22u8), (width + 5.0, 40)] {
            p.add(PathShape::line(arc_points(c, radius, a0, a1), Stroke::new(w, with_alpha(ACCENT, a))));
        }
        let pts = arc_points(c, radius, a0, a1);
        let n = pts.len();
        for (i, seg) in pts.windows(2).enumerate() {
            let col = lerp_color(ACCENT_LO, ACCENT_HI, i as f32 / n.max(1) as f32);
            p.line_segment([seg[0], seg[1]], Stroke::new(width, col));
        }
        p.circle_filled(pts[0], width / 2.0, ACCENT_LO);
        p.circle_filled(*pts.last().unwrap(), width / 2.0, ACCENT_HI);
    }
    let text = match fraction {
        Some(f) => format!("{:.0}%", (f * 100.0).floor()),
        None => "…".into(),
    };
    p.text(c - vec2(0.0, 8.0), Align2::CENTER_CENTER, text, black(size * 0.2), TEXT);
    if !sub.is_empty() {
        p.text(c + vec2(0.0, size * 0.13), Align2::CENTER_CENTER, sub, semibold(12.5), MUTED);
    }
}

/// Dashed rounded rectangle (drop zones).
pub fn dashed_rect(p: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    let pts = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    for w in pts.windows(2) {
        p.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, 8.0, 6.0));
    }
}

/// Small caps label above a section.
pub fn kicker(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text.to_uppercase()).font(bold(11.0)).color(ACCENT_HI).extra_letter_spacing(1.6));
}

/// One label/value pair in a stats grid.
pub fn stat(ui: &mut Ui, label: &str, value: &str) {
    ui.vertical(|ui| {
        ui.label(RichText::new(label.to_uppercase()).font(bold(10.5)).color(FAINT).extra_letter_spacing(1.2));
        ui.label(RichText::new(value).font(bold(17.0)).color(TEXT));
    });
}

/// Info / warning box.
pub fn callout(ui: &mut Ui, icon: &str, color: Color32, text: &str) {
    Frame::new()
        .fill(with_alpha(color, 18))
        .stroke(Stroke::new(1.0_f32, with_alpha(color, 60)))
        .corner_radius(CornerRadius::same(R_MD))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icon).font(regular(16.0)).color(color));
                ui.add(egui::Label::new(RichText::new(text).font(regular(12.5)).color(lerp_color(MUTED, TEXT, 0.4))).wrap());
            });
        });
}
