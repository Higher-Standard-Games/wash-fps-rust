//! wash-fps `client/HomeView.ts` and `styles/HomeView.css`: the lobby's
//! first page. Three full-height cards, Quick play, Campaign and Play
//! online, each doing exactly what its key in the rail does; the page only
//! puts the three ways into the game first, big and plain, so a new player
//! meets a choice before the mode list, the rooms and the chat.
//!
//! The card under the pointer widens, lifts and glows, its art drifting
//! closer. Each card's text sits on its colour, which breaks up into the
//! art along its top: pink blocks, purple lines, a gold halftone. The
//! cards are slanted like the rail's keys (`clip-path`), so everything on
//! a card is cut to its face here ([`Face`]).
//!
//! The pictures are built into the game, as the campaign's are: Quick
//! play's ten match shots (SHAG `public/images/home`) and the campaign
//! banner's street and squad. Each is decoded on a thread of its own the
//! first time it is wanted; only the shot on screen and the next are kept.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{SystemTime, UNIX_EPOCH};

use shagger::ui::components::lobby_twin::{clamp_css, hsl, hsla, text};
use shagger::ui::components::outline_text::{OutlineStyle, paint_outline_text_styled};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::client::campaign::campaign_view::{SQUAD, STREET, cover_uv, handheld};
use crate::client::washoi_style::{DISPLAY_FONT, display_family};

/// Quick play's match shots, and where the fighter stands across each (%).
const QUICK_PLAY_SHOTS: [(&[u8], f32); 10] = [
    (include_bytes!("../../assets/home/quick-01.webp"), 36.0),
    (include_bytes!("../../assets/home/quick-02.webp"), 24.0),
    (include_bytes!("../../assets/home/quick-03.webp"), 52.0),
    (include_bytes!("../../assets/home/quick-04.webp"), 26.0),
    (include_bytes!("../../assets/home/quick-05.webp"), 33.0),
    (include_bytes!("../../assets/home/quick-06.webp"), 94.0),
    (include_bytes!("../../assets/home/quick-07.webp"), 25.0),
    (include_bytes!("../../assets/home/quick-08.webp"), 35.0),
    (include_bytes!("../../assets/home/quick-09.webp"), 38.0),
    (include_bytes!("../../assets/home/quick-10.webp"), 78.0),
];
/// Quick play's ground: block size (px), and how full each row of blocks
/// is, from the solid edge up.
const BLOCK: f32 = 18.0;
const BLOCK_ROWS: [f64; 5] = [0.9, 0.65, 0.4, 0.2, 0.07];
const BLOCK_COLUMNS: usize = 56;
/// Campaign's ground: gap then line (px), from the solid edge up, thinning
/// like its stripes.
const STRIPES: [(f32, f32); 6] = [
    (3.0, 10.0),
    (5.0, 7.0),
    (7.0, 5.0),
    (9.0, 3.0),
    (11.0, 2.0),
    (13.0, 1.0),
];
/// Play online's ground: halftone dot spacing (px) and radius per row, from
/// the solid edge up.
const DOT: f32 = 12.0;
const DOT_RADII: [f32; 7] = [6.4, 5.4, 4.5, 3.6, 2.8, 2.0, 1.2];

/// How long a shot holds, and how long the change to the next one takes.
const SHOT_SECS: f64 = 7.0;
const FADE_SECS: f64 = 1.5;
/// How far a shot drifts from its centre each way (% of the card), and the
/// zoom that keeps its edges out of sight.
const DRIFT: f32 = 3.0;
const DRIFT_ZOOM: f32 = 1.14;
/// Play online's four modes, in their own colours, behind the card's text.
const MODES: [(&str, (f32, f32, f32)); 4] = [
    ("Duel", (182.0, 95.0, 52.0)),
    ("Amphi", (100.0, 85.0, 52.0)),
    ("FFA", (46.0, 100.0, 57.0)),
    ("S&D", (322.0, 95.0, 62.0)),
];

/// `--cut`: how far the card's top-left and bottom-right corners are cut.
const CUT: f32 = 30.0;
/// Orbitron's baseline below the top of a CSS line box of `line_height`
/// (ascent 1.011, descent 0.243 em, the content area centred in the box),
/// less egui's own (the ascent), in em: where a galley goes in that box.
fn galley_top_in_line(line_height: f32) -> f32 {
    (line_height - 1.254) / 2.0
}

/// What a card asks of the lobby (`HomeActions`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeAction {
    QuickPlay,
    Campaign,
    PlayOnline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Quick,
    Campaign,
    Online,
}

impl Kind {
    const ALL: [Self; 3] = [Self::Quick, Self::Campaign, Self::Online];

    fn title(self) -> &'static str {
        match self {
            Self::Quick => "Quick play",
            Self::Campaign => "Campaign",
            Self::Online => "Play online",
        }
    }

    fn caption(self) -> &'static str {
        match self {
            Self::Quick => "Free For All",
            Self::Campaign => "Single player challenge",
            Self::Online => "Multiplayer lobbies",
        }
    }

    /// `--accent`: the top edge and the glow.
    fn accent(self) -> Color32 {
        match self {
            Self::Quick => hsl(322.0, 95.0, 62.0),
            Self::Campaign => hsl(276.0, 90.0, 64.0),
            Self::Online => hsl(46.0, 100.0, 57.0),
        }
    }

    /// The text's ground.
    fn ground(self) -> Color32 {
        match self {
            Self::Campaign => hsl(272.0, 62.0, 34.0),
            _ => self.accent(),
        }
    }

    fn action(self) -> HomeAction {
        match self {
            Self::Quick => HomeAction::QuickPlay,
            Self::Campaign => HomeAction::Campaign,
            Self::Online => HomeAction::PlayOnline,
        }
    }
}

/// `--ink`.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 5.0, a)
}

/// `color-mix(in srgb, a p%, b)`.
fn mix(a: Color32, p: f32, b: Color32) -> Color32 {
    let m = |x: u8, y: u8| (f32::from(x) * p + f32::from(y) * (1.0 - p)).round() as u8;
    Color32::from_rgba_unmultiplied(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), m(a.a(), b.a()))
}

/// A small xorshift, seeded from the clock: the blocks' scatter, the shots'
/// order and their drift (`Math.random`).
struct Random(u64);

impl Random {
    fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0x2545_f491, |d| d.as_nanos() as u64);
        Self(seed | 1)
    }

    fn next(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

// ── The pictures ──

/// One decoded picture, by its name: `quick-<n>`, `street` or `squad`.
type Decoded = (String, egui::ColorImage);

fn color_image(image: &image::RgbaImage) -> egui::ColorImage {
    let size = [image.width() as usize, image.height() as usize];
    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw())
}

/// `filter: brightness(b) saturate(s)`.
fn filtered(mut image: image::RgbaImage, brightness: f32, saturation: f32) -> image::RgbaImage {
    for p in image.pixels_mut() {
        let [r, g, b, a] = p.0.map(f32::from);
        let grey = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        let f = |c: f32| ((grey + (c - grey) * saturation) * brightness).clamp(0.0, 255.0) as u8;
        p.0 = [f(r), f(g), f(b), a as u8];
    }
    image
}

fn decode(name: &str) -> Option<egui::ColorImage> {
    let read = |bytes: &[u8]| image::load_from_memory(bytes).ok().map(|i| i.to_rgba8());
    match name {
        // `.home__street`: brightness(0.5) saturate(0.85).
        "street" => read(STREET).map(|i| color_image(&filtered(i, 0.5, 0.85))),
        "squad" => read(SQUAD).map(|i| color_image(&i)),
        _ => {
            let n: usize = name.strip_prefix("quick-")?.parse().ok()?;
            read(QUICK_PLAY_SHOTS.get(n)?.0).map(|i| color_image(&i))
        }
    }
}

/// The page's pictures as egui textures, each decoded on its own thread
/// the first time it is wanted.
struct HomeArt {
    send: Sender<Decoded>,
    receiver: Receiver<Decoded>,
    requested: HashSet<String>,
    textures: HashMap<String, egui::TextureHandle>,
    /// The context the textures belong to: each screen's window brings its
    /// own, and the page outlives them.
    ctx: Option<egui::Context>,
}

impl HomeArt {
    fn new() -> Self {
        let (send, receiver) = channel();
        Self {
            send,
            receiver,
            requested: HashSet::new(),
            textures: HashMap::new(),
            ctx: None,
        }
    }

    fn want(&mut self, name: &str) {
        if !self.requested.insert(name.to_owned()) {
            return;
        }
        let (send, owned) = (self.send.clone(), name.to_owned());
        let spawned = std::thread::Builder::new().name("home-art".into()).spawn(move || {
            if let Some(image) = decode(&owned) {
                let _ = send.send((owned, image));
            }
        });
        if spawned.is_err() {
            self.requested.remove(name);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if self.ctx.as_ref() != Some(ctx) {
            self.textures.clear();
            self.requested.clear();
            // What was decoded for the old context is decoded again.
            while self.receiver.try_recv().is_ok() {}
            self.ctx = Some(ctx.clone());
        }
        while let Ok((name, image)) = self.receiver.try_recv() {
            if self.requested.contains(&name) {
                let texture = ctx.load_texture(&name, image, egui::TextureOptions::LINEAR);
                self.textures.insert(name, texture);
                ctx.request_repaint();
            }
        }
    }

    fn texture(&self, name: &str) -> Option<&egui::TextureHandle> {
        self.textures.get(name)
    }

    /// Lets go of the shots not named in `keep`.
    fn keep_shots(&mut self, keep: &[String]) {
        let drop: Vec<String> = self
            .requested
            .iter()
            .filter(|n| n.starts_with("quick-") && !keep.contains(n))
            .cloned()
            .collect();
        for name in drop {
            self.requested.remove(&name);
            self.textures.remove(&name);
        }
    }
}

fn shot_name(index: usize) -> String {
    format!("quick-{index}")
}

/// A shot on screen: which, since when, and the way it drifts.
#[derive(Debug, Clone, Copy)]
struct Layer {
    shot: usize,
    since: f64,
    /// The drift's direction, radians.
    angle: f32,
}

/// `ShotReel`: the match shots one after another, in random order. Each
/// drifts slowly one way, then the next fades in over it. The next shot
/// loads only when its turn comes, and the reel waits while Home is off
/// screen (it moves only while drawn).
struct ShotReel {
    front: Option<Layer>,
    back: Option<Layer>,
    pending: Option<usize>,
}

impl ShotReel {
    fn update(&mut self, now: f64, art: &mut HomeArt, random: &mut Random) {
        let due = self.front.is_none_or(|f| now - f.since >= SHOT_SECS);
        if self.pending.is_none() && due {
            let shot = self.pick(random);
            art.want(&shot_name(shot));
            self.pending = Some(shot);
        }
        if let Some(shot) = self.pending
            && art.texture(&shot_name(shot)).is_some()
        {
            self.back = self.front.take();
            self.front = Some(Layer {
                shot,
                since: now,
                angle: (random.next() * std::f64::consts::TAU) as f32,
            });
            self.pending = None;
        }
        let keep: Vec<String> = [self.front, self.back]
            .iter()
            .flatten()
            .map(|l| l.shot)
            .chain(self.pending)
            .map(shot_name)
            .collect();
        art.keep_shots(&keep);
    }

    /// Any shot but the one on screen.
    fn pick(&self, random: &mut Random) -> usize {
        let current = self.front.map(|f| f.shot);
        loop {
            let index = (random.next() * QUICK_PLAY_SHOTS.len() as f64) as usize % QUICK_PLAY_SHOTS.len();
            if Some(index) != current {
                return index;
            }
        }
    }
}

// ── Drawing on a slanted card ──

/// Sutherland–Hodgman: `poly` cut to the convex `clip`, wound clockwise on
/// screen (y down).
fn clip_to(poly: &[Pos2], clip: &[Pos2]) -> Vec<Pos2> {
    let mut out = poly.to_vec();
    for (i, &a) in clip.iter().enumerate() {
        let b = clip[(i + 1) % clip.len()];
        let side = |p: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let input = std::mem::take(&mut out);
        for (j, &p) in input.iter().enumerate() {
            let q = input[(j + 1) % input.len()];
            let (sp, sq) = (side(p), side(q));
            if sp >= 0.0 {
                out.push(p);
            }
            if (sp >= 0.0) != (sq >= 0.0) {
                let t = sp / (sp - sq);
                out.push(p + (q - p) * t);
            }
        }
        if out.len() < 3 {
            return Vec::new();
        }
    }
    out
}

fn rect_poly(r: Rect) -> [Pos2; 4] {
    [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
}

/// The card's face (`.home__face`): `clip-path: polygon(var(--cut) 0, 100%
/// 0, calc(100% - var(--cut)) 100%, 0 100%)`.
fn face_poly(r: Rect) -> [Pos2; 4] {
    [
        pos2(r.left() + CUT, r.top()),
        r.right_top(),
        pos2(r.right() - CUT, r.bottom()),
        r.left_bottom(),
    ]
}

/// A painter that keeps to a card's face.
struct Face<'a> {
    painter: &'a Painter,
    poly: [Pos2; 4],
}

impl Face<'_> {
    fn fill(&self, poly: &[Pos2], color: Color32) {
        let clipped = clip_to(poly, &self.poly);
        if clipped.len() >= 3 {
            self.painter.add(Shape::convex_polygon(clipped, color, Stroke::NONE));
        }
    }

    fn rect(&self, r: Rect, color: Color32) {
        self.fill(&rect_poly(r), color);
    }

    /// A linear gradient over `r`, down it or across it.
    fn gradient(&self, r: Rect, stops: &[(f32, Color32)], down: bool) {
        let mut mesh = Mesh::default();
        for pair in stops.windows(2) {
            let ((a, ca), (b, cb)) = (pair[0], pair[1]);
            let band = if down {
                Rect::from_min_max(
                    pos2(r.left(), r.top() + r.height() * a),
                    pos2(r.right(), r.top() + r.height() * b),
                )
            } else {
                Rect::from_min_max(
                    pos2(r.left() + r.width() * a, r.top()),
                    pos2(r.left() + r.width() * b, r.bottom()),
                )
            };
            let clipped = clip_to(&rect_poly(band), &self.poly);
            if clipped.len() < 3 {
                continue;
            }
            let base = mesh.vertices.len() as u32;
            for p in &clipped {
                let t = if down {
                    (p.y - band.top()) / band.height().max(0.001)
                } else {
                    (p.x - band.left()) / band.width().max(0.001)
                };
                mesh.colored_vertex(*p, ca.lerp_to_gamma(cb, t.clamp(0.0, 1.0)));
            }
            for i in 1..clipped.len() as u32 - 1 {
                mesh.add_triangle(base, base + i, base + i + 1);
            }
        }
        self.painter.add(mesh);
    }

    /// A picture drawn over `r` (its `uv` part), cut to the face.
    fn image(&self, texture: egui::TextureId, r: Rect, uv: Rect, tint: Color32) {
        let clipped = clip_to(&rect_poly(r), &self.poly);
        if clipped.len() < 3 {
            return;
        }
        let mut mesh = Mesh::with_texture(texture);
        for p in &clipped {
            let f = vec2((p.x - r.left()) / r.width(), (p.y - r.top()) / r.height());
            mesh.vertices.push(egui::epaint::Vertex {
                pos: *p,
                uv: pos2(uv.left() + f.x * uv.width(), uv.top() + f.y * uv.height()),
                color: tint,
            });
        }
        for i in 1..clipped.len() as u32 - 1 {
            mesh.add_triangle(0, i, i + 1);
        }
        self.painter.add(mesh);
    }

    fn circle(&self, centre: Pos2, radius: f32, color: Color32) {
        let points: Vec<Pos2> = (0..14)
            .map(|i| {
                let a = i as f32 / 14.0 * std::f32::consts::TAU;
                centre + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        self.fill(&points, color);
    }
}

/// `object-fit: cover` with `object-position: x% 50%`: where a picture of
/// `size` lands over `frame`.
fn cover_rect(size: egui::Vec2, frame: Rect, x: f32) -> Rect {
    let scale = (frame.width() / size.x).max(frame.height() / size.y);
    let shown = size * scale;
    Rect::from_min_size(
        pos2(
            frame.left() + (frame.width() - shown.x) * x,
            frame.top() + (frame.height() - shown.y) / 2.0,
        ),
        shown,
    )
}

/// `r` scaled by `s` about `centre`.
fn scaled(r: Rect, centre: Pos2, s: f32) -> Rect {
    Rect::from_min_max(centre + (r.min - centre) * s, centre + (r.max - centre) * s)
}

/// A CSS transition, roughly: `value` eased toward `target`, `tau` seconds
/// to cover most of the way.
fn ease(value: &mut f32, target: f32, dt: f32, tau: f32) {
    *value += (target - value.to_owned()) * (1.0 - (-dt / tau).exp());
    if (target - *value).abs() < 0.001 {
        *value = target;
    }
}

/// One card's motion: under the pointer it widens (`flex-grow`), lifts
/// (`translateY(-6px)`, the glow) and draws its art closer (`scale(1.06)`).
#[derive(Debug, Clone, Copy)]
struct Motion {
    grow: f32,
    lift: f32,
    zoom: f32,
    /// Play online's mode words step aside and fill in.
    modes: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            grow: 1.0,
            lift: 0.0,
            zoom: 0.0,
            modes: 0.0,
        }
    }
}

/// `HomeView`.
pub struct HomeView {
    art: HomeArt,
    reel: ShotReel,
    random: Random,
    /// Quick play's blocks, a new scatter each load: row by row from the
    /// solid edge up, whether each column has its block.
    blocks: Vec<[bool; BLOCK_COLUMNS]>,
    motion: [Motion; 3],
    quick_enabled: bool,
    /// The last frame's time, for the transitions.
    last: Option<f64>,
}

impl Default for HomeView {
    fn default() -> Self {
        Self::new()
    }
}

impl HomeView {
    #[must_use]
    pub fn new() -> Self {
        let mut random = Random::new();
        let blocks = BLOCK_ROWS
            .iter()
            .map(|&fill| std::array::from_fn(|_| random.next() < fill))
            .collect();
        Self {
            art: HomeArt::new(),
            reel: ShotReel {
                front: None,
                back: None,
                pending: None,
            },
            random,
            blocks,
            motion: [Motion::default(); 3],
            quick_enabled: false,
            last: None,
        }
    }

    /// Quick play is off when its key in the rail is: no lobby, or another
    /// room open.
    pub fn set_quick_play_enabled(&mut self, enabled: bool) {
        self.quick_enabled = enabled;
    }

    /// Whether Quick play's card can be pressed.
    #[must_use]
    pub fn quick_play_enabled(&self) -> bool {
        self.quick_enabled
    }

    /// The page in `content`; the card pressed, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui, content: Rect, screen: Rect) -> Option<HomeAction> {
        let now = ui.input(|i| i.time);
        let dt = self.last.map_or(0.0, |t| (now - t).clamp(0.0, 0.25) as f32);
        self.last = Some(now);
        self.art.poll(ui.ctx());
        self.art.want("street");
        self.art.want("squad");
        self.reel.update(now, &mut self.art, &mut self.random);

        // A narrow window: the cards stack, each a band across.
        let stacked = screen.width() <= 900.0;
        let gap = 16.0;
        // `--card-w`: a card's width while a neighbour grows; titles and
        // mode words size from it, so they keep their lines at every moment.
        let card_w = if stacked {
            260.0
        } else {
            (content.width() - 2.0 * gap) / 3.35
        };
        let sizes = Sizes {
            card_w,
            title: clamp_css(20.0, (card_w - 88.0) / 6.3, 60.0),
            caption: clamp_css(11.0, card_w * 0.05, 18.0),
            foot: clamp_css(26.0, screen.height() * 0.05, 46.0),
            modes: if stacked {
                20.0
            } else {
                clamp_css(26.0, (card_w - 64.0) / 3.8, 112.0)
            },
            stacked,
        };
        let grows: f32 = self.motion.iter().map(|m| m.grow).sum();
        let room = if stacked { content.height() } else { content.width() } - 2.0 * gap;
        let mut at = if stacked { content.top() } else { content.left() };
        let mut action = None;
        for (i, kind) in Kind::ALL.into_iter().enumerate() {
            let share = room * self.motion[i].grow / grows;
            let rect = if stacked {
                Rect::from_min_size(pos2(content.left(), at), vec2(content.width(), share))
            } else {
                Rect::from_min_size(pos2(at, content.top()), vec2(share, content.height()))
            };
            at += share + gap;
            let enabled = kind != Kind::Quick || self.quick_enabled;
            let response = ui.interact(rect, egui::Id::NULL.with(("home-card", i)), egui::Sense::click());
            let hot = enabled && response.hovered();
            if response.hovered() {
                ui.ctx().set_cursor_icon(if enabled {
                    egui::CursorIcon::PointingHand
                } else {
                    egui::CursorIcon::NotAllowed
                });
            }
            if enabled && response.clicked() {
                action = Some(kind.action());
            }
            let m = &mut self.motion[i];
            let on = if hot { 1.0 } else { 0.0 };
            ease(
                &mut m.grow,
                if hot { if stacked { 1.3 } else { 1.35 } } else { 1.0 },
                dt,
                0.16,
            );
            ease(&mut m.lift, on, dt, 0.1);
            ease(&mut m.zoom, on, dt, 0.4);
            ease(&mut m.modes, on, dt, 0.15);
            let m = *m;
            let mut painter = ui.painter().clone();
            if !enabled {
                painter.multiply_opacity(0.45);
            }
            self.card(&painter, rect, kind, m, &sizes, now);
        }
        if self
            .motion
            .iter()
            .any(|m| m.grow != 1.0 || m.lift != 0.0 || m.zoom != 0.0)
        {
            ui.ctx().request_repaint();
        }
        action
    }

    fn card(&self, painter: &Painter, rect: Rect, kind: Kind, m: Motion, s: &Sizes, now: f64) {
        let lift = if s.stacked { 0.0 } else { 6.0 * m.lift };
        let r = rect.translate(vec2(0.0, -lift));
        let accent = kind.accent();
        // `drop-shadow(7px 9px 0 ...)`, and the accent's glow under the pointer.
        if m.lift > 0.0 {
            for step in (1..=6).rev() {
                let grow = step as f32 * 4.5;
                let a = 0.55 * m.lift * (1.0 - step as f32 / 7.0) * 0.22;
                painter.add(Shape::convex_polygon(
                    face_poly(r.expand(grow)).to_vec(),
                    accent.gamma_multiply(a),
                    Stroke::NONE,
                ));
            }
        }
        painter.add(Shape::convex_polygon(
            face_poly(r.translate(vec2(7.0, 9.0))).to_vec(),
            hsla(250.0, 50.0, 3.0, 0.7),
            Stroke::NONE,
        ));
        let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
        let face = Face {
            painter: &clip,
            poly: face_poly(r),
        };
        // The face's own ground.
        match kind {
            Kind::Quick => face.rect(r, ink(1.0)),
            Kind::Campaign => face.rect(r, hsl(268.0, 52.0, 9.0)),
            Kind::Online => face.gradient(r, &[(0.0, hsl(250.0, 35.0, 13.0)), (1.0, hsl(250.0, 45.0, 6.0))], true),
        }
        // `.home__art`, drawn closer under the pointer.
        let zoom = 1.0 + 0.06 * m.zoom;
        match kind {
            Kind::Quick => self.shots(&face, r, zoom, now),
            Kind::Campaign => self.campaign_art(&face, r, zoom, now),
            Kind::Online => modes_art(&clip, r, zoom, m.modes, s),
        }
        // `.home__shade`: the art fades to ink at the foot.
        face.gradient(
            r,
            &[
                (0.0, ink(0.0)),
                (0.3, ink(0.0)),
                (0.58, ink(0.55)),
                (0.88, ink(0.92)),
                (1.0, ink(0.92)),
            ],
            true,
        );
        self.ground(&face, r, kind, s);
        text_block(&clip, r, kind, s);
        // `.home__face::after`: the accent along the top, brighter when chosen.
        face.rect(
            Rect::from_min_size(r.min, vec2(r.width(), 12.0)),
            accent.gamma_multiply(0.6 + 0.4 * m.lift),
        );
    }

    /// Quick play's shots: the one on screen over the one it replaces.
    fn shots(&self, face: &Face<'_>, r: Rect, zoom: f32, now: f64) {
        let front_in = self
            .reel
            .front
            .map_or(1.0, |f| ((now - f.since) / FADE_SECS).clamp(0.0, 1.0) as f32);
        let layers = [
            self.reel.back.filter(|_| front_in < 1.0).map(|l| (l, 1.0 - front_in)),
            self.reel.front.map(|l| (l, front_in)),
        ];
        for (layer, opacity) in layers.into_iter().flatten() {
            let Some(t) = self.art.texture(&shot_name(layer.shot)) else {
                continue;
            };
            let focus = QUICK_PLAY_SHOTS[layer.shot].1 / 100.0;
            let base = cover_rect(t.size_vec2(), r, focus);
            // The drift, linear through both fades: `translate(-d, -d)` to
            // `translate(d, d)` of the card, at `scale(1.14)`.
            let p = ((now - layer.since) / (SHOT_SECS + 2.0 * FADE_SECS)).clamp(0.0, 1.0) as f32;
            let d = vec2(layer.angle.cos(), layer.angle.sin()) * DRIFT / 100.0;
            let shift = vec2(d.x * r.width(), d.y * r.height()) * (2.0 * p - 1.0);
            let drawn = scaled(scaled(base, r.center(), DRIFT_ZOOM).translate(shift), r.center(), zoom);
            let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            face.image(t.id(), drawn, full, Color32::WHITE.gamma_multiply(opacity));
        }
    }

    /// Campaign's art: the campaign page's street and squad, drifting like
    /// a handheld camera, in its purple stripes.
    fn campaign_art(&self, face: &Face<'_>, r: Rect, zoom: f32, now: f64) {
        let (far, near) = handheld(now as f32);
        let centre = r.center();
        if let Some(t) = self.art.texture("street") {
            // `inset: -20px -30px`, cover.
            let frame = r.expand2(vec2(30.0, 20.0)).translate(far);
            let size = t.size_vec2();
            let uv = cover_uv([size.x, size.y], frame.size(), (0.5, 0.5));
            face.image(t.id(), scaled(frame, centre, zoom), uv, Color32::WHITE);
        }
        if let Some(t) = self.art.texture("squad") {
            // `left: 50%; bottom: 4%; height: 86%`, centred.
            let size = t.size_vec2();
            let h = r.height() * 0.86;
            let w = h * size.x / size.y.max(1.0);
            let bottom = r.bottom() - r.height() * 0.04;
            let drawn = Rect::from_min_size(pos2(centre.x - w / 2.0, bottom - h), vec2(w, h)).translate(near);
            let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            face.image(t.id(), scaled(drawn, centre, zoom), full, Color32::WHITE);
        }
        // `.home__stripes`: fine lines over a purple wash.
        let mut y = r.top();
        while y < r.bottom() {
            face.rect(
                Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), 2.0)),
                hsla(276.0, 70.0, 70.0, 0.07),
            );
            y += 5.0;
        }
        face.gradient(
            r,
            &[
                (0.0, hsla(278.0, 56.0, 30.0, 0.42)),
                (0.55, hsla(262.0, 52.0, 16.0, 0.25)),
                (1.0, hsla(262.0, 52.0, 16.0, 0.55)),
            ],
            false,
        );
    }

    /// `.home__ground`: the text's colour, solid below and breaking into the
    /// art along its top edge.
    fn ground(&self, face: &Face<'_>, r: Rect, kind: Kind, s: &Sizes) {
        let edge_h = match kind {
            Kind::Quick => BLOCK * BLOCK_ROWS.len() as f32,
            Kind::Campaign => STRIPES.iter().map(|(g, l)| g + l).sum(),
            Kind::Online => DOT * DOT_RADII.len() as f32,
        };
        // Two title lines, the gap, a caption line, the foot, and room above.
        let height = s.title * 1.9 + 12.0 + s.caption * 1.2 + s.foot + 26.0 + edge_h;
        let top = r.bottom() - height;
        let color = kind.ground();
        face.rect(Rect::from_min_max(pos2(r.left(), top + edge_h), r.max), color);
        match kind {
            // Rows of square blocks, thinning upward.
            Kind::Quick => {
                let tile = BLOCK * BLOCK_COLUMNS as f32;
                let mut x0 = r.left();
                while x0 < r.right() {
                    for (row, cols) in self.blocks.iter().enumerate() {
                        let y = top + (BLOCK_ROWS.len() - 1 - row) as f32 * BLOCK;
                        for (col, &on) in cols.iter().enumerate() {
                            let x = x0 + col as f32 * BLOCK;
                            if on && x < r.right() {
                                face.rect(Rect::from_min_size(pos2(x, y), vec2(BLOCK, BLOCK)), color);
                            }
                        }
                    }
                    x0 += tile;
                }
            }
            // Full-width lines, thinner and further apart going up.
            Kind::Campaign => {
                let mut bottom = top + edge_h;
                for (gap, line) in STRIPES {
                    bottom -= gap + line;
                    face.rect(
                        Rect::from_min_size(pos2(r.left(), bottom), vec2(r.width(), line)),
                        color,
                    );
                }
            }
            // A halftone, its dots shrinking going up, each row set half a
            // dot along.
            Kind::Online => {
                for (row, &radius) in DOT_RADII.iter().enumerate() {
                    let cy = top + edge_h - DOT * row as f32 - DOT / 2.0;
                    let mut cx = r.left() + if row % 2 == 0 { DOT / 2.0 } else { 0.0 };
                    while cx - radius < r.right() {
                        face.circle(pos2(cx, cy), radius, color);
                        cx += DOT;
                    }
                }
            }
        }
    }
}

/// The type sizes, from a resting card.
struct Sizes {
    card_w: f32,
    title: f32,
    caption: f32,
    /// `--text-foot`: the text's distance from the card's foot.
    foot: f32,
    modes: f32,
    stacked: bool,
}

/// `.home__text`: the title, one word to a line, over its caption.
fn text_block(painter: &Painter, r: Rect, kind: Kind, s: &Sizes) {
    let left = r.left() + CUT + 20.0;
    // Clear of the slanted right edge, which reaches in by the cut at the foot.
    let right = r.right() - CUT - 8.0;
    let bright = kind != Kind::Campaign;
    let (title_color, title_shadow, shadow_at) = if bright {
        (ink(1.0), mix(kind.accent(), 0.4, Color32::WHITE), vec2(3.0, 3.0))
    } else {
        (hsl(45.0, 100.0, 95.0), hsla(250.0, 50.0, 4.0, 0.65), vec2(0.0, 4.0))
    };
    let caption_color = if bright { ink(0.78) } else { hsl(285.0, 100.0, 86.0) };
    // The caption wraps at a resting card's width, so its lines stay put as
    // the card grows.
    let wrap = (s.card_w - 88.0).min(right - left).max(40.0);
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &kind.caption().to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(s.caption, display_family()),
            color: caption_color,
            extra_letter_spacing: s.caption * 0.16,
            line_height: Some(s.caption * 1.2),
            ..Default::default()
        },
    );
    job.wrap.max_width = wrap;
    let caption = painter.layout_job(job);
    let bottom = r.bottom() - s.foot;
    let caption_top = bottom - caption.size().y;
    painter.galley(pos2(left, caption_top), caption, caption_color);
    let words: Vec<&str> = kind.title().split(' ').collect();
    let line = s.title * 0.95;
    let title_top = caption_top - 12.0 - line * words.len() as f32;
    for (i, word) in words.iter().enumerate() {
        let g = text(
            painter,
            &word.to_uppercase(),
            s.title,
            display_family(),
            title_color,
            s.title * 0.01,
        );
        let y = title_top + line * i as f32 + galley_top_in_line(0.95) * s.title;
        painter.galley_with_override_text_color(pos2(left, y) + shadow_at, g.clone(), title_shadow);
        painter.galley(pos2(left, y), g, title_color);
    }
}

/// Play online's art: the four modes' names in their colours, outlined
/// and slanted, stacked behind the text; under the pointer they step
/// aside and fill in.
fn modes_art(painter: &Painter, r: Rect, zoom: f32, on: f32, s: &Sizes) {
    let size = s.modes * zoom;
    let line = size * 0.95;
    let step = line + size * 0.05;
    // `skewX(-8deg)` about each word's middle.
    let slant = (8.0f32).to_radians().tan();
    let baseline_in_line = galley_top_in_line(0.95) * size + 1.011 * size;
    let centre = r.center();
    let words = MODES.len() as f32;
    for (i, (name, (h, sat, l))) in MODES.iter().enumerate() {
        let colour = hsl(*h, *sat, *l);
        let word = name.to_uppercase();
        let (left, top) = if s.stacked {
            // A small column on the right, its words set right.
            let w = shagger::ui::components::outline_text::outline_text_width(DISPLAY_FONT, &word, size, 0.0);
            let block = step * words - size * 0.05;
            (r.right() - CUT - 14.0 - w, r.center().y - block / 2.0 + step * i as f32)
        } else {
            let top = r.top() + r.width() * 0.09;
            (r.left() + CUT + 18.0, top + step * i as f32)
        };
        let baseline_y = top + baseline_in_line;
        let middle = top + line / 2.0;
        let at = pos2(left + 14.0 * on - slant * (baseline_y - middle), baseline_y);
        // The art's zoom is about the card's middle.
        let at = centre + (at - centre) * zoom;
        let opacity = 0.55 + 0.3 * on;
        let style = OutlineStyle {
            stroke: Stroke::new(2.0, colour.gamma_multiply(opacity)),
            fill: colour.gamma_multiply(0.22 * on * opacity),
            slant,
        };
        paint_outline_text_styled(painter, DISPLAY_FONT, &word, size, 0.0, at, &style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_three_cards_in_order_with_their_captions() {
        let labels: Vec<String> = Kind::ALL
            .iter()
            .map(|k| format!("{}: {}", k.title(), k.caption()))
            .collect();
        assert_eq!(
            labels,
            [
                "Quick play: Free For All",
                "Campaign: Single player challenge",
                "Play online: Multiplayer lobbies",
            ]
        );
        assert_eq!(
            Kind::ALL.map(Kind::action),
            [HomeAction::QuickPlay, HomeAction::Campaign, HomeAction::PlayOnline]
        );
    }

    #[test]
    fn every_shot_decodes() {
        for i in 0..QUICK_PLAY_SHOTS.len() {
            let image = decode(&shot_name(i)).expect("a shot");
            assert_eq!(image.size, [1280, 720]);
        }
        assert!(decode("street").is_some());
        assert!(decode("squad").is_some());
        assert!(decode("quick-10").is_none());
    }

    #[test]
    fn the_reel_never_repeats_the_shot_on_screen_and_waits_for_its_picture() {
        let mut art = HomeArt::new();
        let mut reel = ShotReel {
            front: None,
            back: None,
            pending: None,
        };
        let mut random = Random::new();
        reel.update(0.0, &mut art, &mut random);
        let first = reel.pending.expect("a shot asked for");
        assert!(reel.front.is_none());
        // Its picture arrives: on screen.
        let ctx = egui::Context::default();
        art.poll(&ctx);
        art.textures.insert(
            shot_name(first),
            ctx.load_texture(
                "t",
                egui::ColorImage::filled([1, 1], Color32::BLACK),
                egui::TextureOptions::LINEAR,
            ),
        );
        reel.update(0.1, &mut art, &mut random);
        assert_eq!(reel.front.map(|f| f.shot), Some(first));
        // Held for its seven seconds.
        reel.update(6.0, &mut art, &mut random);
        assert!(reel.pending.is_none());
        reel.update(7.2, &mut art, &mut random);
        let next = reel.pending.expect("the next shot");
        assert_ne!(next, first);
        for _ in 0..200 {
            assert_ne!(reel.pick(&mut random), first);
        }
    }

    #[test]
    fn a_shape_is_cut_to_the_slanted_face() {
        let face = face_poly(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0)));
        // The top-left corner is cut away.
        let corner = clip_to(&rect_poly(Rect::from_min_size(pos2(0.0, 0.0), vec2(10.0, 10.0))), &face);
        let area = |p: &[Pos2]| {
            (0..p.len())
                .map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % p.len()]);
                    a.x * b.y - b.x * a.y
                })
                .sum::<f32>()
                .abs()
                / 2.0
        };
        assert!(area(&corner) < 100.0 * 0.5);
        // The middle is whole.
        let middle = clip_to(
            &rect_poly(Rect::from_min_size(pos2(80.0, 40.0), vec2(20.0, 20.0))),
            &face,
        );
        assert!((area(&middle) - 400.0).abs() < 0.01);
    }
}
