//! wash-fps `client/hud/widgets/PauseMenu.ts` (`FPSPauseMenu`) and
//! `client/styles/MatchMenu.css`: the Esc menu, the lobby's gold rail drawn
//! over the match, which does not stop for it and stays in view to its
//! right. Escape or a lost mouse opens it (`FPSHud`'s `pointerlockchange`),
//! Resume or Escape takes the mouse back, and Leave match goes back to the
//! lobby. The keys are SHAG's `RaisedKey` (`styles/RaisedKey.css`).
//!
//! Settings opens the lobby page's settings over the match, when the page
//! can (`settings`); Back there returns to this menu.
//!
//! Not yet: Spectate and Join (the native match has no spectator state to
//! go to), and the mode's keys (Loadout, with no kit picker yet). The
//! veil's `backdrop-filter: blur(3px)` is not drawn: egui paints over the
//! frame and cannot read it.

use std::sync::Arc;

use shagger::ui::components::lobby_twin::{clamp_css as clamp, gradient_polygon, horizontal_gradient, hsl, hsla};
use shagger::ui::egui_layer::egui;
use shagger::ui::egui_layer::egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};

use crate::client::controller_input::UiIntent;
use crate::client::washoi_style::display_family;

/// What a key on the menu asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseMenuAction {
    /// Back to the fight: close and take the mouse again.
    Resume,
    /// Controls and video: the lobby page's settings over the match.
    Settings,
    /// Back to the lobby.
    Leave,
}

/// The menu and whether it is up.
#[derive(Default)]
pub struct PauseMenu {
    pub open: bool,
    /// The Settings key is offered (the lobby page can show them).
    pub settings: bool,
    /// When this opening was first drawn (egui seconds), for the slide-in.
    shown_at: Option<f64>,
    /// `buildRailBrand`'s logo, sized for the rail, and the height it was made for.
    logo: Option<(egui::TextureHandle, u32)>,
    /// The key a controller has lit (`UINavigation`'s focus), by its place
    /// on the rail.
    focus: Option<usize>,
}

/// `fps-match-menu__note`.
pub const PAUSE_MENU_NOTE: &str = "The match keeps going while you're here.";

/// The lobby's logo (`/data/washoi.png`, washoi.net's `src/assets/washoi.png`).
const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/brand/washoi.png");

/// `--ink`, the rail's lettering.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 8.0, a)
}

/// `--cream`.
fn cream(a: f32) -> Color32 {
    hsla(45.0, 100.0, 95.0, a)
}

/// Text in one face, size and colour, `spacing` points between letters.
fn galley(painter: &egui::Painter, text: &str, font: egui::FontId, color: Color32, spacing: f32) -> Arc<egui::Galley> {
    galley_wrapped(painter, text, font, color, spacing, f32::INFINITY)
}

fn galley_wrapped(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    spacing: f32,
    width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..Default::default()
        },
    );
    job.wrap.max_width = width;
    painter.layout_job(job)
}

/// The face's colours: (gradient stops, label, hint).
struct KeyLook {
    face: [(f32, Color32); 3],
    side: [Color32; 2],
    text: Color32,
    sub: Color32,
    shadow: Color32,
}

/// `RaisedKey`'s variants this menu uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyKind {
    /// Gold, the rail's keys (`raised-key--nav`).
    Nav,
    /// Ink at rest, gold lettering: the first key (`raised-key--ink`).
    Ink,
    /// Cream and small: Leave (`raised-key--sm raised-key--cream`).
    Leave,
}

impl KeyKind {
    fn look(self, hovered: bool, pressed: bool) -> KeyLook {
        if pressed {
            // `:active`
            return KeyLook {
                face: [
                    (0.0, hsl(272.0, 76.0, 58.0)),
                    (0.5, hsl(269.0, 73.0, 51.5)),
                    (1.0, hsl(266.0, 70.0, 45.0)),
                ],
                side: [hsl(266.0, 72.0, 34.0), hsl(258.0, 72.0, 19.0)],
                text: hsl(45.0, 100.0, 96.0),
                sub: hsla(45.0, 100.0, 96.0, 0.72),
                shadow: hsla(268.0, 80.0, 14.0, 0.5),
            };
        }
        if hovered {
            // `:hover`: the violet lift, which is also the focus ring.
            return KeyLook {
                face: [
                    (0.0, hsl(278.0, 92.0, 75.0)),
                    (0.55, hsl(273.0, 80.0, 61.0)),
                    (1.0, hsl(268.0, 72.0, 50.0)),
                ],
                side: [hsl(266.0, 72.0, 34.0), hsl(258.0, 72.0, 19.0)],
                text: hsl(45.0, 100.0, 96.0),
                sub: hsla(45.0, 100.0, 96.0, 0.72),
                shadow: hsla(268.0, 80.0, 14.0, 0.5),
            };
        }
        match self {
            Self::Nav => KeyLook {
                face: [
                    (0.0, hsl(51.0, 100.0, 73.0)),
                    (0.55, hsl(46.0, 100.0, 58.0)),
                    (1.0, hsl(42.0, 100.0, 50.0)),
                ],
                side: [hsl(36.0, 92.0, 40.0), hsl(28.0, 88.0, 25.0)],
                text: ink(1.0),
                sub: ink(0.62),
                shadow: hsla(28.0, 90.0, 14.0, 0.42),
            },
            Self::Ink => KeyLook {
                face: [
                    (0.0, hsl(250.0, 28.0, 21.0)),
                    (0.6, hsl(250.0, 40.0, 10.0)),
                    (1.0, hsl(252.0, 44.0, 6.0)),
                ],
                side: [hsl(252.0, 44.0, 5.0), hsl(252.0, 44.0, 3.0)],
                text: hsl(46.0, 100.0, 60.0),
                sub: hsla(46.0, 100.0, 60.0, 0.62),
                shadow: hsla(28.0, 90.0, 14.0, 0.5),
            },
            Self::Leave => KeyLook {
                face: [
                    (0.0, hsl(45.0, 100.0, 97.0)),
                    (0.6, hsl(42.0, 62.0, 90.0)),
                    (1.0, hsl(40.0, 42.0, 82.0)),
                ],
                side: [hsl(36.0, 26.0, 60.0), hsl(30.0, 22.0, 42.0)],
                text: ink(1.0),
                sub: ink(0.62),
                shadow: hsla(28.0, 90.0, 14.0, 0.42),
            },
        }
    }

    /// `--key-d`: how far the face stands off its side.
    fn depth(self) -> f32 {
        if self == Self::Leave { 3.6 } else { 6.0 }
    }
}

/// One `RaisedKey` in `rect` (the face at rest); true when clicked.
/// `stacked` sets the hint under the label, as a narrow rail does.
#[allow(clippy::too_many_arguments)]
fn raised_key(
    ui: &mut egui::Ui,
    id: &str,
    rect: Rect,
    kind: KeyKind,
    label: &str,
    hint: &str,
    stacked: bool,
    label_size: f32,
    focused: bool,
) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(id), egui::Sense::click());
    let hovered = response.hovered() || focused;
    let pressed = response.is_pointer_button_down_on();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let look = kind.look(hovered, pressed);
    let d = kind.depth();
    let dx = d * 0.75;
    let press = if pressed {
        1.0
    } else if hovered {
        -0.35
    } else {
        0.0
    };
    // `--key-cut`: 3 % of the width off the face's lower left and upper right.
    let cut = rect.width() * 0.03;
    let shape = |offset: Vec2| {
        [
            rect.left_top() + vec2(cut, 0.0) + offset,
            rect.right_top() + offset,
            rect.right_bottom() + vec2(-cut, 0.0) + offset,
            rect.left_bottom() + offset,
        ]
    };
    let painter = ui.painter();
    // The cast shadow: `drop-shadow`, softened by three widening copies.
    let shadow_at = vec2(dx * (1.0 - press) + 2.0, d * (1.0 - press) + 5.0);
    for (grow, alpha) in [(4.0, 0.25), (2.0, 0.35), (0.0, 0.45)] {
        let [a, b, c, e] = shape(shadow_at);
        let g = vec2(grow, grow);
        let corners = [a - g, b + vec2(grow, -grow), c + g, e + vec2(-grow, grow)];
        let color = look.shadow.gamma_multiply(alpha);
        gradient_polygon(painter, &corners, &[(0.0, color), (1.0, color)]);
    }
    // The side it stands on.
    gradient_polygon(
        painter,
        &shape(vec2(dx, d)),
        &[(0.0, look.side[0]), (1.0, look.side[1])],
    );
    // The face, and its lit top edge and shaded foot.
    let lift = vec2(dx * press, d * press);
    let face = shape(lift);
    gradient_polygon(painter, &face, &look.face);
    let band = |from: f32, to: f32| {
        let at = |t: f32| face[0].lerp(face[3], t);
        let at_right = |t: f32| face[1].lerp(face[2], t);
        let h = rect.height();
        [at(from / h), at_right(from / h), at_right(to / h), at(to / h)]
    };
    let white = Color32::from_white_alpha(140);
    gradient_polygon(painter, &band(0.0, 2.0), &[(0.0, white), (1.0, white)]);
    let foot = hsla(250.0, 60.0, 5.0, 0.14);
    let h = rect.height();
    gradient_polygon(painter, &band(h - 3.0, h), &[(0.0, foot), (1.0, foot)]);
    // `raised-key__label` and `__hint`, uppercase.
    let label = galley(
        painter,
        &label.to_uppercase(),
        egui::FontId::new(label_size, display_family()),
        look.text,
        label_size * 0.07,
    );
    let hint_size = if kind == KeyKind::Leave { 12.0 } else { 13.0 };
    let hint = galley(
        painter,
        &hint.to_uppercase(),
        egui::FontId::proportional(hint_size),
        look.sub,
        hint_size * 0.12,
    );
    let (pad_left, pad_right) = if kind == KeyKind::Leave {
        (20.0, 16.0)
    } else {
        (28.0, 24.0)
    };
    let inner = Rect::from_min_max(
        rect.min + lift + vec2(pad_left, 0.0),
        rect.max + lift - vec2(pad_right, 0.0),
    );
    if stacked {
        let gap = 5.0;
        let total = label.size().y + gap + hint.size().y;
        let top = inner.center().y - total / 2.0;
        painter.galley(pos2(inner.left(), top), label, look.text);
        painter.galley(pos2(inner.left(), top + total - hint.size().y), hint, look.sub);
    } else {
        let label_y = inner.center().y - label.size().y / 2.0;
        let hint_y = inner.center().y - hint.size().y / 2.0;
        let hint_x = inner.right() - hint.size().x;
        painter.galley(pos2(inner.left(), label_y), label, look.text);
        painter.galley(pos2(hint_x, hint_y), hint, look.sub);
    }
    response.clicked()
}

/// The ease of `fps-match-menu-in` (`cubic-bezier(0.2, 0.9, 0.3, 1)`),
/// close enough: an ease-out that starts fast.
fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

impl PauseMenu {
    /// `open`.
    pub fn open(&mut self) {
        self.open = true;
    }

    /// `close`.
    pub fn close(&mut self) {
        self.open = false;
        self.shown_at = None;
        self.focus = None;
    }

    /// The rail's keys, in order.
    fn keys(&self) -> Vec<PauseMenuAction> {
        let mut keys = vec![PauseMenuAction::Resume];
        if self.settings {
            keys.push(PauseMenuAction::Settings);
        }
        keys.push(PauseMenuAction::Leave);
        keys
    }

    /// `UINavigation.handle` on the rail: up and down step through the
    /// keys (the first is lit at the first step), confirm presses the lit
    /// key (the first, before any step) and back resumes.
    pub fn nav(&mut self, intent: UiIntent) -> Option<PauseMenuAction> {
        if !self.open {
            return None;
        }
        let keys = self.keys();
        let last = keys.len() - 1;
        match intent {
            UiIntent::Up => self.focus = Some(self.focus.map_or(0, |f| f.saturating_sub(1))),
            UiIntent::Down => self.focus = Some(self.focus.map_or(0, |f| (f + 1).min(last))),
            UiIntent::Left | UiIntent::Right => {}
            UiIntent::Confirm => return keys.get(self.focus.unwrap_or(0)).copied(),
            UiIntent::Back => return Some(PauseMenuAction::Resume),
        }
        None
    }

    /// The logo's texture at `height` pixels, made again when the size changes.
    fn logo(&mut self, ctx: &egui::Context, height: u32) -> Option<egui::TextureId> {
        let stale = self
            .logo
            .as_ref()
            .is_none_or(|(_, made)| made.abs_diff(height) > height / 10);
        if stale {
            let image = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png).ok()?;
            let width = (image.width() as f32 * height as f32 / image.height() as f32).round() as u32;
            let image = image
                .resize_exact(width.max(1), height.max(1), image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let texture = ctx.load_texture("pause-menu-logo", color, egui::TextureOptions::LINEAR);
            self.logo = Some((texture, height));
        }
        self.logo.as_ref().map(|(t, _)| t.id())
    }

    /// Draws the menu while it is open: `kicker` names the match
    /// (gametype · map) and `name` the player on the plate.
    pub fn ui(&mut self, ctx: &egui::Context, kicker: &str, name: &str) -> Option<PauseMenuAction> {
        if !self.open {
            return None;
        }
        let now = ctx.input(|i| i.time);
        let since = (now - *self.shown_at.get_or_insert(now)) as f32;
        let screen = ctx.content_rect();
        let (w, h) = (screen.width(), screen.height());
        let skew = if w <= 650.0 { 26.0 } else { 44.0 };
        let rail_w = if w <= 650.0 {
            w.min(380.0)
        } else {
            clamp(300.0, w * 0.26, 520.0)
        };
        let logo_h = clamp(96.0, h * 0.19, 205.0);
        let logo = self.logo(ctx, (logo_h * ctx.pixels_per_point()).round() as u32);
        let settings = self.settings;
        let focus = self.focus;
        let leave_at = self.keys().len() - 1;
        let mut action = None;
        egui::Area::new(egui::Id::unique("pause-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                let painter = ui.painter().clone();
                // `fps-match-menu`: the veil over the match, fading in.
                let veil = ease_out(since / 0.18);
                let shade = |a: f32| hsla(250.0, 45.0, 5.0, a * veil);
                let r = screen;
                horizontal_gradient(
                    &painter,
                    r,
                    &[(0.0, shade(0.55)), (0.55, shade(0.18)), (1.0, shade(0.3))],
                );

                // `fps-match-menu__rail`: in from the left as it fades in.
                let slide = ease_out(since / 0.24);
                let x0 = r.left() - 36.0 * (1.0 - slide);
                let fade = slide;
                let alpha = |c: Color32| c.gamma_multiply(fade);
                let rail = [
                    pos2(x0, r.top()),
                    pos2(x0 + rail_w, r.top()),
                    pos2(x0 + rail_w - skew, r.bottom()),
                    pos2(x0, r.bottom()),
                ];
                gradient_polygon(
                    &painter,
                    &rail,
                    &[
                        (0.0, alpha(hsl(46.0, 100.0, 57.0))),
                        (1.0, alpha(hsl(41.0, 96.0, 48.0))),
                    ],
                );
                // The radial glow at its head (`radial-gradient(120% 55% at 25% 0%)`).
                let mut glow = egui::Mesh::default();
                let centre = pos2(x0 + rail_w * 0.25, r.top());
                let (rx, ry) = (rail_w * 1.2 * 0.62, h * 0.55 * 0.62);
                glow.colored_vertex(centre, alpha(hsla(54.0, 100.0, 78.0, 0.55)));
                let steps = 24;
                for i in 0..=steps {
                    let a = std::f32::consts::PI * i as f32 / steps as f32;
                    let at = pos2(centre.x + rx * a.cos(), centre.y + ry * a.sin());
                    // Kept inside the rail's slanted edge.
                    let edge = x0 + rail_w - skew * ((at.y - r.top()) / h);
                    glow.colored_vertex(pos2(at.x.min(edge).max(x0), at.y), Color32::TRANSPARENT);
                    if i > 0 {
                        glow.add_triangle(0, i, i + 1);
                    }
                }
                painter.add(egui::Shape::mesh(glow));
                // The violet edge along the slant.
                gradient_polygon(
                    &painter,
                    &[
                        pos2(x0 + rail_w - 8.0, r.top()),
                        pos2(x0 + rail_w, r.top()),
                        pos2(x0 + rail_w - skew, r.bottom()),
                        pos2(x0 + rail_w - skew - 8.0, r.bottom()),
                    ],
                    &[
                        (0.0, alpha(hsl(276.0, 90.0, 64.0))),
                        (1.0, alpha(hsl(262.0, 75.0, 42.0))),
                    ],
                );

                let left = x0 + 26.0;
                let right = x0 + rail_w - skew - 20.0;
                let content_w = right - left;
                let mut y = r.top() + clamp(22.0, h * 0.04, 40.0);

                // `fps-match-menu__head`: the logo, the kicker and the note.
                match logo {
                    Some(texture) => {
                        let aspect = 936.0 / 468.0;
                        let size = vec2(logo_h * aspect, logo_h);
                        let size = if size.x > content_w + 8.0 {
                            size * ((content_w + 8.0) / size.x)
                        } else {
                            size
                        };
                        let at = Rect::from_min_size(pos2(left - 8.0, y - 8.0), size);
                        let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                        // `drop-shadow(0 6px 10px)`, as a soft copy beneath.
                        let shadow = hsla(28.0, 90.0, 14.0, 0.18 * fade);
                        for d in [4.0, 6.0, 8.0] {
                            painter.image(texture, at.translate(vec2(0.0, d)), uv, shadow);
                        }
                        painter.image(texture, at, uv, Color32::WHITE.gamma_multiply(fade));
                        y = at.bottom();
                    }
                    None => {
                        let brand = galley(
                            &painter,
                            "WASHIKI WASHOI",
                            egui::FontId::new(clamp(24.0, w * 0.026, 44.0), display_family()),
                            alpha(ink(1.0)),
                            0.0,
                        );
                        painter.galley(pos2(left, y + 14.0), brand.clone(), alpha(ink(1.0)));
                        y += 14.0 + brand.size().y + 6.0;
                    }
                }
                y += clamp(12.0, h * 0.024, 26.0);
                let kicker = galley(
                    &painter,
                    &kicker.to_uppercase(),
                    egui::FontId::new(12.0, display_family()),
                    alpha(ink(1.0)),
                    12.0 * 0.14,
                );
                let kicker_h = kicker.size().y.max(14.0);
                painter.galley(pos2(left, y), kicker, alpha(ink(1.0)));
                y += kicker_h;
                if h > 720.0 {
                    let note = galley_wrapped(
                        &painter,
                        PAUSE_MENU_NOTE,
                        egui::FontId::proportional(16.0),
                        alpha(ink(0.72)),
                        0.0,
                        content_w.min(28.0 * 8.5),
                    );
                    y += 8.0;
                    let note_h = note.size().y;
                    painter.galley(pos2(left, y), note, alpha(ink(0.72)));
                    y += note_h;
                }

                // `fps-match-menu__keys`.
                y += clamp(20.0, h * 0.04, 42.0);
                let key_w = content_w - 12.0;
                let stacked = w <= 1500.0;
                let label_size = clamp(15.0, w * 0.011, 21.0);
                let mut keys: Vec<(&str, &str, &str, PauseMenuAction)> =
                    vec![("resume", "Resume", "Back to the fight", PauseMenuAction::Resume)];
                if settings {
                    keys.push(("settings", "Settings", "Controls and video", PauseMenuAction::Settings));
                }
                for (i, (id, label, hint, act)) in keys.into_iter().enumerate() {
                    // The first key is inked: what Esc most likely came for.
                    let kind = if i == 0 { KeyKind::Ink } else { KeyKind::Nav };
                    let rect = Rect::from_min_size(pos2(left, y), vec2(key_w, 66.0));
                    if raised_key(ui, id, rect, kind, label, hint, stacked, label_size, focus == Some(i)) {
                        action = Some(act);
                    }
                    y += 66.0 + 6.0 + 8.0;
                }

                // `fps-match-menu__foot`: the plate, then Leave, at the bottom.
                // A narrow rail stacks Leave's hint under its label too.
                let leave_h = if stacked { 54.0 } else { 46.0 };
                let leave_top = r.bottom() - 22.0 - (3.6 + 6.0) - leave_h;
                let foot_w = content_w - 12.0;
                let name_g = galley(
                    &painter,
                    name,
                    egui::FontId::new(15.0, display_family()),
                    alpha(cream(1.0)),
                    15.0 * 0.04,
                );
                let where_g = galley(
                    &painter,
                    "In a match",
                    egui::FontId::proportional(12.0),
                    alpha(cream(0.6)),
                    0.0,
                );
                let plate_h = 11.0 + name_g.size().y + 3.0 + where_g.size().y + 11.0;
                let plate_top = leave_top - 14.0 - plate_h;
                if plate_top > y {
                    gradient_polygon(
                        &painter,
                        &[
                            pos2(left, plate_top),
                            pos2(left + foot_w, plate_top),
                            pos2(left + foot_w - 12.0, plate_top + plate_h),
                            pos2(left, plate_top + plate_h),
                        ],
                        &[
                            (0.0, alpha(hsl(250.0, 28.0, 16.0))),
                            (1.0, alpha(hsl(252.0, 44.0, 7.0))),
                        ],
                    );
                    let name_h = name_g.size().y;
                    painter.galley(pos2(left + 16.0, plate_top + 11.0), name_g, alpha(cream(1.0)));
                    painter.galley(
                        pos2(left + 16.0, plate_top + 11.0 + name_h + 3.0),
                        where_g,
                        alpha(cream(0.6)),
                    );
                }
                let leave = Rect::from_min_size(pos2(left, leave_top.max(y)), vec2(foot_w, leave_h));
                if raised_key(
                    ui,
                    "leave",
                    leave,
                    KeyKind::Leave,
                    "Leave match",
                    "Back to the lobby",
                    stacked,
                    14.0,
                    focus == Some(leave_at),
                ) {
                    action = Some(PauseMenuAction::Leave);
                }
            });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One frame of `menu`: what it asked and how many shapes it drew.
    fn frame(menu: &mut PauseMenu) -> (Option<PauseMenuAction>, usize) {
        let ctx = egui::Context::default();
        let mut action = Some(PauseMenuAction::Leave);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            action = menu.ui(ui.ctx(), "Deathmatch · dm2", "Player");
        });
        output.textures_delta.clear();
        (action, output.shapes.len())
    }

    #[test]
    fn a_closed_menu_draws_nothing() {
        assert_eq!(frame(&mut PauseMenu::default()), (None, 0));
    }

    #[test]
    fn an_open_menu_draws_and_waits_for_a_key() {
        let mut menu = PauseMenu::default();
        menu.open();
        let (action, shapes) = frame(&mut menu);
        // Nothing pressed, nothing asked.
        assert_eq!(action, None);
        assert!(shapes > 0);
        menu.close();
        assert!(!menu.open);
    }

    #[test]
    fn the_logo_decodes() {
        let image = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png).unwrap();
        assert_eq!((image.width(), image.height()), (936, 468));
    }

    #[test]
    fn a_controller_walks_the_rail() {
        let mut menu = PauseMenu {
            settings: true,
            ..PauseMenu::default()
        };
        assert_eq!(menu.nav(UiIntent::Confirm), None);
        menu.open();
        // Before a step, A presses the first key; B resumes.
        assert_eq!(menu.nav(UiIntent::Confirm), Some(PauseMenuAction::Resume));
        assert_eq!(menu.nav(UiIntent::Back), Some(PauseMenuAction::Resume));
        menu.nav(UiIntent::Down);
        menu.nav(UiIntent::Down);
        assert_eq!(menu.nav(UiIntent::Confirm), Some(PauseMenuAction::Settings));
        menu.nav(UiIntent::Down);
        menu.nav(UiIntent::Down);
        assert_eq!(menu.nav(UiIntent::Confirm), Some(PauseMenuAction::Leave));
        menu.nav(UiIntent::Up);
        assert_eq!(menu.nav(UiIntent::Confirm), Some(PauseMenuAction::Settings));
    }
}
