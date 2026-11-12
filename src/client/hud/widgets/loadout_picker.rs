//! wash-fps `client/hud/widgets/LoadoutPicker.ts` (`FPSLoadoutPicker`) and
//! its `.fps-loadout` styles in `client/hud/styles/WashikiTheme.css`: the kit
//! dialog, shown before a player enters a mode with kits, and from the Esc
//! menu's Loadout key to change kit. The kit tables are SHAG
//! `src/shared/server/gametype/Loadout.ts`; the kit last chosen in a mode is
//! remembered as `Game.rememberLoadout` keeps it (`shag.loadout.<mode>`), in
//! `loadouts.json` beside the cvars.
//!
//! Blocking at the first join: your kit decides what you can do, and being
//! dropped into a match holding a default nobody chose is worse than a
//! moment spent choosing.
//!
//! Not yet: the turning weapon on the stand (`WeaponPreview`, a scene of its
//! own). The stand shows the weapon's slot colour and initials under its name.

use std::path::PathBuf;

use shagger::ui::components::lobby_twin::{gradient_polygon, hsl, hsla};
use shagger::ui::egui_layer::egui;
use shagger::ui::egui_layer::egui::{Color32, Pos2, Rect, Vec2, pos2, vec2};

use super::pause_menu::{KeyKind, galley, galley_wrapped, raised_key};
use super::weapon_slots::weapon_slot_color;
use crate::client::controller_input::UiIntent;
use crate::client::washoi_style::display_family;

/// `PRIMARY_CHOICES`: each kit has exactly one.
pub const PRIMARY_CHOICES: [&str; 3] = ["rlauncher", "voltaic", "electrobolt"];
/// `SECONDARY_CHOICES`: each kit has exactly one.
pub const SECONDARY_CHOICES: [&str; 4] = ["riotgun", "plasmagun", "glauncher", "machinegun"];

/// `LoadoutChoice`: a primary and a secondary, beside the blade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadoutChoice {
    pub primary: &'static str,
    pub secondary: &'static str,
}

/// `DEFAULT_LOADOUT`.
pub const DEFAULT_LOADOUT: LoadoutChoice = LoadoutChoice {
    primary: "rlauncher",
    secondary: "machinegun",
};

impl LoadoutChoice {
    /// The kit when both are choices for their slots.
    #[must_use]
    pub fn parse(primary: &str, secondary: &str) -> Option<Self> {
        Some(Self {
            primary: PRIMARY_CHOICES.into_iter().find(|p| *p == primary)?,
            secondary: SECONDARY_CHOICES.into_iter().find(|s| *s == secondary)?,
        })
    }
}

/// `WEAPON_LABELS`.
#[must_use]
pub fn weapon_label(id: &str) -> &'static str {
    match id {
        "rlauncher" => "Rocket Launcher",
        "voltaic" => "Voltaic",
        "electrobolt" => "Electrobolt",
        "riotgun" => "Riotgun",
        "plasmagun" => "Plasmagun",
        "glauncher" => "Grenade Launcher",
        "machinegun" => "Machinegun",
        _ => "",
    }
}

/// `describeLoadout`: the Loadout key's hint.
#[must_use]
pub fn describe_loadout(choice: LoadoutChoice) -> String {
    format!("{} + {}", weapon_label(choice.primary), weapon_label(choice.secondary))
}

/// Where the kits chosen are kept.
fn store_path() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("loadouts.json"))
}

/// `loadoutStorageKey`'s mode part.
fn mode_key(mode: &str) -> String {
    mode.trim().to_lowercase()
}

/// `rememberedLoadout`: the last kit chosen in `mode` on this machine.
#[must_use]
pub fn remembered(mode: &str) -> Option<LoadoutChoice> {
    let text = std::fs::read_to_string(store_path()?).ok()?;
    let all: serde_json::Value = serde_json::from_str(&text).ok()?;
    let kit = all.get(mode_key(mode))?;
    LoadoutChoice::parse(kit.get("primary")?.as_str()?, kit.get("secondary")?.as_str()?)
}

/// `rememberLoadout`. A store that cannot be written costs only the memory.
pub fn remember(mode: &str, choice: LoadoutChoice) {
    let Some(path) = store_path() else { return };
    let mut all = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    all[mode_key(mode)] = serde_json::json!({ "primary": choice.primary, "secondary": choice.secondary });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, all.to_string());
}

/// What the dialog was closed with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadoutOutcome {
    /// The confirm key: this kit.
    Confirm(LoadoutChoice),
    /// Back or Esc: the kit unchanged.
    Cancel,
}

/// The places a controller can light, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spot {
    Primary(usize),
    Secondary(usize),
    Confirm,
    Back,
}

/// `FPSLoadoutPicker`.
#[derive(Debug)]
pub struct LoadoutPicker {
    open: bool,
    choice: LoadoutChoice,
    /// The weapon on the stand: follows hover, falls back to the primary.
    focus: &'static str,
    mode_name: String,
    /// `LoadoutPickerOptions.timing`: `round` or `respawn`.
    timing: Option<String>,
    /// Opened to change a kit in use, from the Esc menu.
    changing: bool,
    /// It can be backed out of (`onCancel`).
    cancellable: bool,
    /// The spot a controller has lit (`UINavigation`'s focus).
    nav: Option<Spot>,
}

impl Default for LoadoutPicker {
    fn default() -> Self {
        Self {
            open: false,
            choice: DEFAULT_LOADOUT,
            focus: DEFAULT_LOADOUT.primary,
            mode_name: "Search & Destroy".into(),
            timing: None,
            changing: false,
            cancellable: false,
            nav: None,
        }
    }
}

impl LoadoutPicker {
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// `open`: `current` pre-selects the kit already chosen, so reopening it
    /// mid-match is a review rather than a fresh decision. `changing` opens
    /// it from the Esc menu, where Back and Esc return with the kit as it was.
    pub fn open(&mut self, mode_name: &str, current: Option<LoadoutChoice>, timing: Option<&str>, changing: bool) {
        if let Some(current) = current {
            self.choice = current;
        }
        if !mode_name.is_empty() {
            self.mode_name = mode_name.to_owned();
        }
        self.timing = timing.map(str::to_owned);
        self.changing = changing;
        self.cancellable = changing;
        self.focus = self.choice.primary;
        self.nav = None;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.nav = None;
    }

    fn finish(&mut self, outcome: LoadoutOutcome) -> Option<LoadoutOutcome> {
        self.close();
        Some(outcome)
    }

    /// Escape: backs out when the dialog has somewhere to go back to.
    pub fn escape(&mut self) -> Option<LoadoutOutcome> {
        if self.open && self.cancellable {
            return self.finish(LoadoutOutcome::Cancel);
        }
        None
    }

    fn spots(&self) -> Vec<Spot> {
        let mut spots: Vec<Spot> = (0..PRIMARY_CHOICES.len()).map(Spot::Primary).collect();
        spots.extend((0..SECONDARY_CHOICES.len()).map(Spot::Secondary));
        spots.push(Spot::Confirm);
        if self.cancellable {
            spots.push(Spot::Back);
        }
        spots
    }

    /// `UINavigation` on the dialog: up and down walk the options and keys,
    /// left and right cross between the columns, confirm picks the lit
    /// option or presses the lit key, back backs out when it can.
    pub fn nav(&mut self, intent: UiIntent) -> Option<LoadoutOutcome> {
        if !self.open {
            return None;
        }
        let spots = self.spots();
        let at = self.nav.and_then(|n| spots.iter().position(|s| *s == n));
        match intent {
            UiIntent::Up | UiIntent::Down => {
                let next = match (at, intent) {
                    (None, _) => 0,
                    (Some(i), UiIntent::Up) => i.saturating_sub(1),
                    (Some(i), _) => (i + 1).min(spots.len() - 1),
                };
                self.nav = Some(spots[next]);
            }
            UiIntent::Left | UiIntent::Right => {
                self.nav = Some(match self.nav {
                    Some(Spot::Primary(i)) => Spot::Secondary(i.min(SECONDARY_CHOICES.len() - 1)),
                    Some(Spot::Secondary(i)) => Spot::Primary(i.min(PRIMARY_CHOICES.len() - 1)),
                    Some(other) => other,
                    None => Spot::Primary(0),
                });
            }
            UiIntent::Confirm => match self.nav.unwrap_or(Spot::Confirm) {
                Spot::Primary(i) => self.choice.primary = PRIMARY_CHOICES[i],
                Spot::Secondary(i) => self.choice.secondary = SECONDARY_CHOICES[i],
                Spot::Confirm => return self.finish(LoadoutOutcome::Confirm(self.choice)),
                Spot::Back => return self.finish(LoadoutOutcome::Cancel),
            },
            UiIntent::Back => return self.escape(),
        }
        match self.nav {
            Some(Spot::Primary(i)) => self.focus = PRIMARY_CHOICES[i],
            Some(Spot::Secondary(i)) => self.focus = SECONDARY_CHOICES[i],
            _ => {}
        }
        None
    }

    /// The confirm key's label.
    fn confirm_label(&self) -> &'static str {
        if self.changing {
            "Use this kit"
        } else if self.timing.as_deref() == Some("respawn") {
            "Enter the match"
        } else {
            "Enter the round"
        }
    }

    /// Draws the dialog while it is open.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<LoadoutOutcome> {
        if !self.open {
            return None;
        }
        let screen = ctx.content_rect();
        let (w, h) = (screen.width(), screen.height());
        let note = match self.timing.as_deref() {
            _ if !self.changing => None,
            Some("respawn") => Some("Applies from your next spawn"),
            Some(_) => Some("Applies from the next round"),
            None => None,
        };
        // The panel at the CSS's sizes, scaled down as a whole when the
        // window is too short for it (`max-height: 92vh`).
        let natural = 22.0
            + 14.0
            + 14.0
            + 36.0
            + if note.is_some() { 22.0 } else { 0.0 }
            + 14.0
            + 18.0
            + 366.0
            + 22.0
            + 64.0
            + 9.6
            + if self.cancellable { 10.0 + 46.0 + 3.6 } else { 0.0 }
            + 26.0;
        let k = (h * 0.92 / natural).min(1.0);
        let panel_w = (920.0 * k).min(w * 0.94);
        let panel_h = natural * k;
        let panel = Rect::from_min_size(
            pos2(screen.center().x - panel_w / 2.0, screen.center().y - panel_h / 2.0),
            vec2(panel_w, panel_h),
        );
        let mut outcome = None;
        egui::Area::new(egui::Id::unique("loadout-picker"))
            .order(egui::Order::Foreground)
            .fade_in(false)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                let painter = ui.painter().clone();
                // `.fps-loadout`: the veil, which takes the mouse.
                painter.rect_filled(screen, 0.0, hsla(250.0, 45.0, 4.0, 0.82));
                let _ = ui.interact(screen, ui.scope_id().with("veil"), egui::Sense::click());

                // `.fps-loadout-panel`.
                painter.rect_filled(panel.translate(vec2(6.0, 7.0) * k), 0.0, hsla(250.0, 45.0, 2.0, 0.6));
                gradient_polygon(
                    &painter,
                    &corners(panel),
                    &[
                        (0.0, hsla(250.0, 30.0, 12.0, 0.98)),
                        (1.0, hsla(250.0, 35.0, 8.0, 0.98)),
                    ],
                );
                painter.rect_stroke(
                    panel,
                    0.0,
                    egui::Stroke::new(k, hsla(45.0, 90.0, 62.0, 0.28)),
                    egui::StrokeKind::Inside,
                );
                let left = panel.left() + 26.0 * k;
                let right = panel.right() - 26.0 * k;
                let mut y = panel.top() + 22.0 * k;

                // `.fps-loadout-hero`.
                let hero_h = (14.0 + 14.0 + 36.0 + if note.is_some() { 22.0 } else { 0.0 } + 14.0) * k;
                let hero = Rect::from_min_max(pos2(left, y), pos2(right, y + hero_h));
                self.paint_hero(&painter, hero, note, k);
                y = hero.bottom() + 18.0 * k;

                // `.fps-loadout-body`: the columns, and the stand beside them.
                let body_h = 366.0 * k;
                let columns_w = 210.0 * k;
                let mut cy = y;
                for (label, options, primary) in [
                    ("Primary", &PRIMARY_CHOICES[..], true),
                    ("Secondary", &SECONDARY_CHOICES[..], false),
                ] {
                    let heading = galley(
                        &painter,
                        &label.to_uppercase(),
                        egui::FontId::proportional(11.0 * k),
                        hsl(186.0, 70.0, 62.0),
                        11.0 * k * 0.2,
                    );
                    painter.galley(pos2(left, cy), heading, hsl(186.0, 70.0, 62.0));
                    cy += 20.0 * k;
                    for (i, id) in options.iter().enumerate() {
                        let rect = Rect::from_min_size(pos2(left, cy), vec2(columns_w, 40.0 * k));
                        let spot = if primary { Spot::Primary(i) } else { Spot::Secondary(i) };
                        let selected = if primary {
                            self.choice.primary == *id
                        } else {
                            self.choice.secondary == *id
                        };
                        let response =
                            ui.interact(rect, ui.scope_id().with(("loadout-option", *id)), egui::Sense::click());
                        let lit = self.nav == Some(spot);
                        if response.hovered() || lit {
                            // Hovering previews without committing.
                            self.focus = id;
                            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if response.clicked() {
                            if primary {
                                self.choice.primary = id;
                            } else {
                                self.choice.secondary = id;
                            }
                        }
                        paint_option(&painter, rect, id, selected, response.hovered() || lit, k);
                        cy += 46.0 * k;
                    }
                    cy += 10.0 * k;
                }
                let stand = Rect::from_min_max(
                    pos2(left + columns_w + 20.0 * k, y),
                    pos2(right, y + body_h.max(260.0 * k)),
                );
                paint_stand(&painter, stand, self.focus, k);
                y += body_h;

                // `.fps-loadout-confirm`.
                y += 22.0 * k;
                let confirm = Rect::from_min_size(pos2(left, y), vec2(right - left - 12.0 * k, 64.0 * k));
                if raised_key(
                    ui,
                    "loadout-confirm",
                    confirm,
                    KeyKind::Cta,
                    self.confirm_label(),
                    "",
                    false,
                    22.0 * k,
                    self.nav == Some(Spot::Confirm),
                ) {
                    outcome = Some(LoadoutOutcome::Confirm(self.choice));
                }
                y = confirm.bottom() + 9.6 * k;

                // `.fps-loadout-back`.
                if self.cancellable {
                    y += 10.0 * k;
                    let back = Rect::from_min_size(pos2(left, y), vec2(300.0 * k, 46.0 * k));
                    if raised_key(
                        ui,
                        "loadout-back",
                        back,
                        KeyKind::Leave,
                        "Back",
                        "Keep your kit",
                        false,
                        14.0 * k,
                        self.nav == Some(Spot::Back),
                    ) {
                        outcome = Some(LoadoutOutcome::Cancel);
                    }
                }
            });
        if let Some(outcome) = outcome {
            return self.finish(outcome);
        }
        None
    }

    /// `.fps-loadout-hero`: the mode, the heading and, when changing, when
    /// the kit applies.
    fn paint_hero(&self, painter: &egui::Painter, hero: Rect, note: Option<&str>, k: f32) {
        // `clip-path: polygon(0 7%, 96% 0, 100% 88%, 5% 100%)`.
        let at = |x: f32, y: f32| pos2(hero.left() + hero.width() * x, hero.top() + hero.height() * y);
        let shape = [at(0.0, 0.07), at(0.96, 0.0), at(1.0, 0.88), at(0.05, 1.0)];
        let wash = hsla(0.0, 0.0, 100.0, 0.055);
        gradient_polygon(painter, &shape, &[(0.0, wash), (1.0, wash)]);
        across(
            painter,
            shape,
            &[
                (0.0, hsla(45.0, 100.0, 55.0, 0.16)),
                (0.68, Color32::TRANSPARENT),
                (1.0, Color32::TRANSPARENT),
            ],
        );
        painter.add(egui::Shape::closed_line(
            shape.to_vec(),
            egui::Stroke::new(2.0 * k, hsla(45.0, 100.0, 55.0, 0.3)),
        ));
        // `::after`: the dashes at the lower right.
        let stripe = hsl(45.0, 92.0, 64.0).gamma_multiply(0.55);
        let (sx, sy) = (hero.right() - 16.0 * k - 72.0 * k, hero.bottom() - 11.0 * k - 8.0 * k);
        let mut x = 0.0;
        while x < 72.0 {
            let dash = Rect::from_min_size(pos2(sx + x * k, sy), vec2(8.0f32.min(72.0 - x) * k, 8.0 * k));
            painter.rect_filled(dash, 0.0, stripe);
            x += 14.0;
        }

        let left = hero.left() + 40.0 * k;
        let mut y = hero.top() + 14.0 * k;
        let gold = hsl(45.0, 92.0, 64.0);
        let kicker = galley(
            painter,
            &self.mode_name.to_uppercase(),
            egui::FontId::proportional(11.0 * k),
            gold,
            11.0 * k * 0.18,
        );
        painter.galley(pos2(left, y), kicker, gold);
        y += 14.0 * k;
        let text = hsl(180.0, 100.0, 95.0);
        let heading = galley(
            painter,
            "LOADOUT",
            egui::FontId::new(30.0 * k, display_family()),
            text,
            30.0 * k * 0.1,
        );
        painter.galley(
            pos2(left, y) + vec2(2.0, 2.0) * k,
            heading.clone(),
            hsla(0.0, 0.0, 0.0, 0.45),
        );
        painter.galley(pos2(left, y), heading, text);
        y += 36.0 * k;
        if let Some(note) = note {
            let color = hsla(45.0, 100.0, 95.0, 0.7);
            let note = galley_wrapped(
                painter,
                &note.to_uppercase(),
                egui::FontId::proportional(13.0 * k),
                color,
                13.0 * k * 0.12,
                hero.width() - 60.0 * k,
            );
            painter.galley(pos2(left, y + 6.0 * k), note, color);
        }
    }
}

/// A rect's corners, clockwise from the top left.
fn corners(r: Rect) -> [Pos2; 4] {
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
}

/// A left-to-right gradient over a quadrilateral (top left, top right,
/// bottom right, bottom left), the stops at fractions of its width.
fn across(painter: &egui::Painter, quad: [Pos2; 4], stops: &[(f32, Color32)]) {
    let [tl, tr, br, bl] = quad;
    let mut mesh = egui::Mesh::default();
    for (i, (t, color)) in stops.iter().enumerate() {
        mesh.colored_vertex(tl.lerp(tr, *t), *color);
        mesh.colored_vertex(bl.lerp(br, *t), *color);
        if i > 0 {
            let n = u32::try_from(i * 2).unwrap_or(0);
            mesh.add_triangle(n - 2, n, n - 1);
            mesh.add_triangle(n - 1, n, n + 1);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// `.fps-loadout-option`, and `--selected`, at `rect`.
fn paint_option(painter: &egui::Painter, rect: Rect, id: &str, selected: bool, hovered: bool, k: f32) {
    // `:hover` lifts it up and left.
    let rect = if hovered {
        rect.translate(vec2(-2.0, -2.0) * k)
    } else {
        rect
    };
    // `clip-path: polygon(3% 0, 100% 0, 97% 100%, 0 100%)`.
    let cut = rect.width() * 0.03;
    let shape = |o: Vec2| {
        [
            rect.left_top() + vec2(cut, 0.0) + o,
            rect.right_top() + o,
            rect.right_bottom() - vec2(cut, 0.0) + o,
            rect.left_bottom() + o,
        ]
    };
    let shadow = hsla(0.0, 0.0, 0.0, if selected { 0.36 } else { 0.32 });
    gradient_polygon(painter, &shape(vec2(4.0, 5.0) * k), &[(0.0, shadow), (1.0, shadow)]);
    let face = shape(Vec2::ZERO);
    let base = hsla(238.0, 38.0, 8.0, if selected { 0.98 } else { 0.95 });
    gradient_polygon(painter, &face, &[(0.0, base), (1.0, base)]);
    let (wash, accent, border, text) = if selected {
        (
            [
                (0.0, hsla(45.0, 100.0, 55.0, 0.34)),
                (0.62, hsla(45.0, 90.0, 50.0, 0.12)),
            ],
            hsl(45.0, 92.0, 60.0),
            hsla(45.0, 100.0, 60.0, 0.85),
            hsl(45.0, 96.0, 90.0),
        )
    } else {
        (
            [
                (0.0, hsla(180.0, 100.0, 50.0, 0.14)),
                (0.6, hsla(0.0, 0.0, 100.0, 0.05)),
            ],
            hsla(180.0, 100.0, 50.0, 0.4),
            hsla(180.0, 100.0, 50.0, if hovered { 0.7 } else { 0.24 }),
            hsl(180.0, 100.0, 95.0),
        )
    };
    across(painter, face, &[wash[0], wash[1], (1.0, Color32::TRANSPARENT)]);
    // `inset 5px 0 0`: the bar down its left edge.
    let bar = [
        face[0],
        face[0] + vec2(5.0 * k, 0.0),
        face[3] + vec2(5.0 * k, 0.0),
        face[3],
    ];
    gradient_polygon(painter, &bar, &[(0.0, accent), (1.0, accent)]);
    painter.add(egui::Shape::closed_line(
        face.to_vec(),
        egui::Stroke::new(2.0 * k, border),
    ));
    let label = galley(
        painter,
        weapon_label(id),
        egui::FontId::new(15.0 * k, display_family()),
        text,
        15.0 * k * 0.06,
    );
    painter.galley(
        pos2(rect.left() + 16.0 * k, rect.center().y - label.size().y / 2.0),
        label,
        text,
    );
}

/// `.fps-loadout-preview`: the stand, with the weapon's colour and initials
/// in place of its model, and its name at the lower left.
fn paint_stand(painter: &egui::Painter, stand: Rect, weapon: &str, k: f32) {
    // `clip-path: polygon(2% 0, 100% 0, 98% 100%, 0 100%)`.
    let cut = stand.width() * 0.02;
    let shape = [
        stand.left_top() + vec2(cut, 0.0),
        stand.right_top(),
        stand.right_bottom() - vec2(cut, 0.0),
        stand.left_bottom(),
    ];
    let base = hsla(238.0, 40.0, 6.0, 0.75);
    gradient_polygon(painter, &shape, &[(0.0, base), (1.0, base)]);
    // `radial-gradient(ellipse at 50% 62%, cyan 0.14, transparent 62%)`,
    // in the weapon's own colour.
    let [h, s, l] = weapon_slot_color(weapon).unwrap_or([180.0, 100.0, 50.0]);
    let centre = pos2(stand.center().x, stand.top() + stand.height() * 0.62);
    let (rx, ry) = (stand.width() * 0.5 * 0.62, stand.height() * 0.62 * 0.62);
    let mut glow = egui::Mesh::default();
    glow.colored_vertex(centre, hsla(h, s, l, 0.2));
    let steps = 48;
    for i in 0..=steps {
        let a = std::f32::consts::TAU * i as f32 / steps as f32;
        let at = pos2(
            centre.x + rx * a.cos(),
            (centre.y + ry * a.sin()).clamp(stand.top(), stand.bottom()),
        );
        glow.colored_vertex(at, Color32::TRANSPARENT);
        if i > 0 {
            glow.add_triangle(0, i, i + 1);
        }
    }
    painter.add(egui::Shape::mesh(glow));
    painter.add(egui::Shape::closed_line(
        shape.to_vec(),
        egui::Stroke::new(2.0 * k, hsla(180.0, 100.0, 50.0, 0.22)),
    ));
    // The initials, large, in the weapon's colour (`WEAPON_ABBR`).
    let initials: String = weapon_label(weapon)
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.len() < 2 {
        weapon_label(weapon).chars().take(2).collect::<String>().to_uppercase()
    } else {
        initials
    };
    let color = hsla(h, s, l, 0.85);
    let big = galley(
        painter,
        &initials,
        egui::FontId::new(110.0 * k, display_family()),
        color,
        0.0,
    );
    let at = pos2(
        centre.x - big.size().x / 2.0,
        stand.center().y - big.size().y / 2.0 - 10.0 * k,
    );
    painter.galley(at + vec2(5.0, 6.0) * k, big.clone(), hsla(250.0, 45.0, 2.0, 0.6));
    painter.galley(at, big, color);
    // `.fps-loadout-preview-name`.
    let text = hsl(180.0, 100.0, 95.0);
    let name = galley(
        painter,
        &weapon_label(weapon).to_uppercase(),
        egui::FontId::new(22.0 * k, display_family()),
        text,
        22.0 * k * 0.08,
    );
    let at = pos2(stand.left() + 18.0 * k, stand.bottom() - 14.0 * k - name.size().y);
    painter.galley(at + vec2(2.0, 2.0) * k, name.clone(), hsla(0.0, 0.0, 0.0, 0.55));
    painter.galley(at, name, text);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(picker: &mut LoadoutPicker) -> (Option<LoadoutOutcome>, usize) {
        let ctx = egui::Context::default();
        let mut outcome = Some(LoadoutOutcome::Cancel);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            outcome = picker.ui(ui.ctx());
        });
        output.textures_delta.clear();
        (outcome, output.shapes.len())
    }

    #[test]
    fn closed_it_draws_nothing() {
        let mut picker = LoadoutPicker::default();
        assert_eq!(frame(&mut picker), (None, 0));
    }

    #[test]
    fn open_it_draws_and_waits() {
        let mut picker = LoadoutPicker::default();
        picker.open("Free For All", None, Some("respawn"), false);
        let (outcome, shapes) = frame(&mut picker);
        assert_eq!(outcome, None);
        assert!(shapes > 0);
        assert_eq!(picker.confirm_label(), "Enter the match");
    }

    #[test]
    fn the_first_join_cannot_be_backed_out_of() {
        let mut picker = LoadoutPicker::default();
        picker.open("Search & Destroy", None, Some("round"), false);
        assert_eq!(picker.escape(), None);
        assert_eq!(picker.nav(UiIntent::Back), None);
        assert!(picker.is_open());
    }

    #[test]
    fn from_the_menu_back_keeps_the_kit() {
        let mut picker = LoadoutPicker::default();
        let kit = LoadoutChoice::parse("electrobolt", "riotgun").unwrap();
        picker.open("Amphi", Some(kit), Some("round"), true);
        assert_eq!(picker.confirm_label(), "Use this kit");
        assert_eq!(picker.escape(), Some(LoadoutOutcome::Cancel));
        assert!(!picker.is_open());
    }

    #[test]
    fn a_controller_picks_both_and_confirms() {
        let mut picker = LoadoutPicker::default();
        picker.open("Free For All", None, Some("respawn"), false);
        // Down to the Voltaic and pick it.
        picker.nav(UiIntent::Down);
        picker.nav(UiIntent::Down);
        assert_eq!(picker.nav(UiIntent::Confirm), None);
        // Across to the secondaries (the Plasmagun, beside it) and pick it.
        picker.nav(UiIntent::Right);
        assert_eq!(picker.nav(UiIntent::Confirm), None);
        assert_eq!(picker.focus, "plasmagun");
        // Down past the rest to the confirm key.
        for _ in 0..3 {
            picker.nav(UiIntent::Down);
        }
        let kit = LoadoutChoice::parse("voltaic", "plasmagun").unwrap();
        assert_eq!(picker.nav(UiIntent::Confirm), Some(LoadoutOutcome::Confirm(kit)));
    }

    #[test]
    fn kits_parse_by_slot_and_describe() {
        assert_eq!(LoadoutChoice::parse("machinegun", "rlauncher"), None);
        assert_eq!(describe_loadout(DEFAULT_LOADOUT), "Rocket Launcher + Machinegun");
    }
}
