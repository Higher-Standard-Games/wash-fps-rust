//! The scoreboard and the post-game result screen (wash-fps
//! `client/hud/widgets/Scoreboard.ts`, styles `.fps-scoreboard*`,
//! `.fps-versus*` and `.fps-postgame*` in `WashikiTheme.css`, the team
//! colours from `TeamColorBridge.ts`, the way home from `FPSHud.ts`'s
//! post-game and `ReturnToLobby.ts`).
//!
//! Held open with Tab; locked open for the post-game, where it moves to the
//! right column under the winner's plate, the winner's name and stat chips
//! stand on the left, and after six seconds of a single-match flow the
//! "Back to the lobby" key appears under the board. Free-for-all lists
//! everyone; a one-a-side match gets a head-to-head line; team modes get a
//! column per side, coloured by allegiance.
//!
//! Not drawn: the winner's 3D model (`PostGameShowcase`), the backdrop blur
//! and the stamp animations.

use std::cell::Cell;
use std::collections::BTreeMap;

use shagger::shared::json::Value;
use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{
    Fonts, Gradient, Slab, UI_SCALE, cream, cream_dim, drop_print_md, drop_print_sm, hsla, void,
};
use crate::client::hud_state::ScoreboardEntry;
use crate::client::match_state_manager::MatchPhase;
use crate::client::player_model_visual_settings::{
    PlayerTeamSlot, parse_rgb_cvar, player_model_visual_default, team_color_cvar,
};
use crate::client::remote_player_logic::CLIENT_KEY_PREFIX;

/// The two sides of a team mode, in the order the lobby lists them.
const SIDES: [&str; 2] = ["soba", "udon"];
/// `FPSHud`: the post-game opens up after the celebration beat.
const OPEN_UP_SECONDS: f64 = 6.0;

/// A board row: a scoreboard entry, or a post-game row kept for a player
/// who left.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BoardEntry {
    pub entry: ScoreboardEntry,
    pub departed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Name,
    Score,
    Deaths,
    Ping,
}

impl Column {
    /// `COLUMN_TITLES`, upper-cased by the header's `text-transform`.
    fn title(self) -> &'static str {
        match self {
            Self::Name => "PLAYER",
            Self::Score => "SCORE",
            Self::Deaths => "DEATHS",
            Self::Ping => "PING",
        }
    }
    fn flex(self) -> f32 {
        if self == Self::Name { 3.0 } else { 1.0 }
    }
}

const FFA_COLUMNS: [Column; 4] = [Column::Name, Column::Score, Column::Deaths, Column::Ping];
const TEAM_COLUMNS: [Column; 3] = [Column::Name, Column::Score, Column::Ping];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Allegiance {
    Ally,
    Enemy,
}

/// One side's column: its name, score, allegiance and rows.
type TeamColumn = (String, f64, Allegiance, Vec<BoardEntry>);

/// What the board shows below the title plate (`rebuild`).
#[derive(Debug, Clone, Default)]
enum Layout {
    #[default]
    Empty,
    FreeForAll {
        rows: Vec<BoardEntry>,
    },
    /// `renderHeadToHead`: left and right side, each a player (or none).
    HeadToHead {
        sides: Box<[(Option<BoardEntry>, Allegiance); 2]>,
        figure: String,
        round_mode: bool,
    },
    /// `renderTeams`: Soba, then Udon.
    Teams {
        teams: Box<[TeamColumn; 2]>,
    },
}

/// The post-game presentation (`setPostGame`).
#[derive(Debug, Clone, Default)]
struct PostGame {
    title: String,
    /// "player_<id>".
    winner_id: Option<String>,
    winner_name: Option<String>,
    /// Everyone seen since the lock, so a player who leaves keeps a row.
    roster: BTreeMap<String, ScoreboardEntry>,
    /// Seconds since it began, for the open-up beat.
    age: f64,
}

/// `FPSScoreboard`.
#[derive(Debug, Default)]
pub struct Scoreboard {
    /// Tab held.
    pub held: bool,
    visible: bool,
    post_game: Option<PostGame>,
    /// The way home is offered (single-match flow, after the beat).
    return_offered: bool,
    /// Where the "Back to the lobby" key was drawn, for clicks.
    return_rect: Cell<Option<[f32; 4]>>,
    local_player_id: String,
    round: u32,
    carrier: Option<String>,
    layout: Layout,
    spectators: Vec<BoardEntry>,
    /// The winner's stat chips: label and value.
    winner_stats: Vec<(&'static str, String)>,
}

/// `toClientKey`.
fn to_client_key(id: &str) -> String {
    format!("{CLIENT_KEY_PREFIX}{id}")
}

/// `TeamColorBridge.toCssColor`: the hue the player chose for a side, at
/// full brightness.
fn team_color(slot: PlayerTeamSlot) -> [f32; 4] {
    let value = shagger::core::cvar::with_cvars(|c| c.get_string(team_color_cvar(slot)));
    let c = parse_rgb_cvar(&value, player_model_visual_default(slot));
    let peak = c.x.max(c.y).max(c.z);
    if !peak.is_finite() || peak <= 0.0 {
        return [1.0; 4];
    }
    let channel = |v: f32| (v / peak).clamp(0.0, 1.0);
    [channel(c.x), channel(c.y), channel(c.z), 1.0]
}

fn allegiance_color(a: Allegiance) -> [f32; 4] {
    team_color(match a {
        Allegiance::Ally => PlayerTeamSlot::Ally,
        Allegiance::Enemy => PlayerTeamSlot::Enemy,
    })
}

/// `--w-text`, `--w-text-muted`.
fn text() -> [f32; 4] {
    hsla(180.0, 100.0, 95.0, 1.0)
}

/// The post-game title: "<winner> wins!" or "Draw!", upper-cased.
#[must_use]
pub fn post_game_title(winner: Option<&Value>) -> String {
    let name = winner
        .filter(|w| w.get("kind").and_then(|k| k.as_str()) != Some("draw"))
        .and_then(|w| w.get("name").and_then(|n| n.as_str()))
        .filter(|n| !n.is_empty());
    match name {
        Some(name) => format!("{name} wins!").to_uppercase(),
        None => "DRAW!".to_owned(),
    }
}

/// `renderWinnerStats`: Score, Frags, Deaths and K/D for the winner.
#[must_use]
pub fn winner_stats(entry: &ScoreboardEntry) -> Vec<(&'static str, String)> {
    let deaths = entry.deaths;
    let mut stats = vec![("Score", format!("{}", entry.score))];
    if let Some(frags) = entry.frags {
        stats.push(("Frags", format!("{frags}")));
    }
    stats.push(("Deaths", format!("{deaths}")));
    if let Some(frags) = entry.frags {
        let kd = if deaths > 0.0 { frags / deaths } else { frags };
        stats.push(("K/D", format!("{kd:.2}")));
    }
    stats
}

/// `collectEntries`: on the post-game board, everyone seen since the lock.
fn collect_entries(
    live: &[ScoreboardEntry],
    roster: Option<&mut BTreeMap<String, ScoreboardEntry>>,
) -> Vec<BoardEntry> {
    let mut entries: Vec<BoardEntry> = live
        .iter()
        .map(|e| BoardEntry {
            entry: e.clone(),
            departed: false,
        })
        .collect();
    let Some(roster) = roster else { return entries };
    for e in live {
        roster.insert(e.player_id.clone(), e.clone());
    }
    for (id, remembered) in roster.iter() {
        if !live.iter().any(|e| &e.player_id == id) {
            entries.push(BoardEntry {
                entry: remembered.clone(),
                departed: true,
            });
        }
    }
    entries
}

impl FpsHudWidget for Scoreboard {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        let state = frame.state;
        // `FPSHud`'s phase change: into and out of the post-game.
        let post_game = m.phase() == MatchPhase::PostGame;
        match (&mut self.post_game, post_game) {
            (None, true) => {
                let winner_name = m
                    .winner()
                    .filter(|w| w.get("kind").and_then(|k| k.as_str()) != Some("draw"))
                    .and_then(|w| w.get("name").and_then(|n| n.as_str()))
                    .map(str::to_owned);
                self.post_game = Some(PostGame {
                    title: post_game_title(m.winner()),
                    winner_id: m.resolve_winner_player_id().map(|id| to_client_key(&id)),
                    winner_name,
                    ..PostGame::default()
                });
            }
            (Some(_), false) => {
                self.post_game = None;
                self.return_offered = false;
            }
            (Some(p), true) => {
                p.age += frame.dt;
                if p.age >= OPEN_UP_SECONDS && m.single_flow() && !state.watcher {
                    self.return_offered = true;
                }
            }
            (None, false) => {}
        }
        self.visible = self.held || self.post_game.is_some();
        if !self.visible {
            self.return_rect.set(None);
            return;
        }
        self.local_player_id = state.local_player_id.clone();
        self.round = m.round();
        self.carrier = m
            .bomb()
            .and_then(|b| b.get("carrier"))
            .and_then(|c| c.as_str())
            .map(str::to_owned);
        let entries = collect_entries(&state.scoreboard, self.post_game.as_mut().map(|p| &mut p.roster));
        self.winner_stats = self
            .post_game
            .as_ref()
            .and_then(|p| p.winner_id.as_ref())
            .and_then(|id| entries.iter().find(|e| &e.entry.player_id == id))
            .map(|e| winner_stats(&e.entry))
            .unwrap_or_default();
        self.spectators.clear();
        // Team-ness comes from the gametype, not from team scores.
        if !state.team_based {
            let (spectators, rows): (Vec<_>, Vec<_>) = entries
                .into_iter()
                .partition(|e| e.entry.team_id.as_deref() == Some("spectator"));
            self.spectators = spectators;
            self.layout = Layout::FreeForAll { rows };
            return;
        }
        let on_side = |e: &BoardEntry| e.entry.team_id.as_deref().is_some_and(|t| SIDES.contains(&t));
        let (players, spectators): (Vec<_>, Vec<_>) = entries.into_iter().partition(on_side);
        self.spectators = spectators;
        let side_of = |side: &str| -> Vec<BoardEntry> {
            players
                .iter()
                .filter(|e| e.entry.team_id.as_deref() == Some(side))
                .cloned()
                .collect()
        };
        // Coloured by allegiance: your side is the ally colour.
        let local_team = players
            .iter()
            .find(|e| e.entry.player_id == self.local_player_id)
            .and_then(|e| e.entry.team_id.clone());
        let allegiance = |side: &str| {
            if local_team.as_deref() == Some(side) {
                Allegiance::Ally
            } else {
                Allegiance::Enemy
            }
        };
        let team_score = |side: &str| state.team_scores.get(side).copied().unwrap_or(0.0);
        if SIDES.iter().all(|s| side_of(s).len() <= 1) && !players.is_empty() {
            let round_mode = self.round > 0;
            let winner_id = self.post_game.as_ref().and_then(|p| p.winner_id.clone());
            let winner_side = winner_id.map(|id| {
                if side_of("udon").iter().any(|e| e.entry.player_id == id) {
                    "udon"
                } else {
                    "soba"
                }
            });
            let order = if winner_side == Some("udon") {
                ["udon", "soba"]
            } else {
                SIDES
            };
            let figure = |side: &str| -> f64 {
                if round_mode {
                    team_score(side)
                } else {
                    side_of(side).first().map_or(0.0, |e| e.entry.score)
                }
            };
            self.layout = Layout::HeadToHead {
                sides: Box::new(order.map(|s| (side_of(s).into_iter().next(), allegiance(s)))),
                figure: format!("{} \u{2014} {}", figure(order[0]), figure(order[1])),
                round_mode,
            };
        } else {
            self.layout = Layout::Teams {
                teams: Box::new(SIDES.map(|s| {
                    let name = state
                        .team_names
                        .get(s)
                        .filter(|n| !n.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("Team {s}"));
                    (name, team_score(s), allegiance(s), side_of(s))
                })),
            };
        }
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Fills the screen with the veil and lays the board out on it.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, _x: f32, _y: f32) {
        if !self.visible {
            return;
        }
        let [vw, vh] = hud.size();
        let post = self.post_game.as_ref();
        // `--scoreboard-scale`: 1.5 for Tab, 1 on the post-game, both
        // outside the HUD's zoom.
        let s = if post.is_some() { 1.0 } else { 1.5 };
        let pen = Pen {
            fonts,
            s,
            post: post.is_some(),
        };
        if post.is_some() {
            // Clear over the stage, darkening under the board.
            let band = |from: f32, to: f32, a: [f32; 4], b: [f32; 4], hud: &mut Hud2d| {
                hud.gradient_rect(vw * from, 0.0, vw * (to - from), vh, a, b);
            };
            let clear = hsla(248.0, 30.0, 4.0, 0.0);
            band(0.34, 0.5, clear, hsla(248.0, 30.0, 4.0, 0.72), hud);
            band(
                0.5,
                0.8,
                hsla(248.0, 30.0, 4.0, 0.72),
                hsla(248.0, 30.0, 4.0, 0.92),
                hud,
            );
            hud.rect(vw * 0.8, 0.0, vw * 0.2, vh, hsla(248.0, 30.0, 4.0, 0.92));
        } else {
            hud.rect(0.0, 0.0, vw, vh, hsla(248.0, 30.0, 4.0, 0.82));
        }
        let (w, pad_top) = if post.is_some() {
            ((820.0 * s).min(vw * 0.5), 92.0 * s)
        } else {
            ((680.0 * s).min(vw * 0.92), 64.0 * s)
        };
        let (pad_x, pad_bottom) = (38.0 * s, 30.0 * s);
        let inner = w - 2.0 * pad_x;
        let content_h = self.content(hud, &pen, 0.0, 0.0, inner, false);
        let max_h = if post.is_some() {
            vh - 0.08 * vh - (0.12 * vh + 108.0)
        } else {
            vh * 0.82
        };
        let h = (pad_top + content_h + pad_bottom).min(max_h);
        let (bx, by) = if post.is_some() {
            // Right column on the 5vw gutter, centred in the space above
            // the way home.
            let top = 0.08 * vh;
            let bottom = vh - (0.12 * vh + 108.0);
            (vw - 0.05 * vw - w, top + ((bottom - top) - h).max(0.0) / 2.0)
        } else {
            ((vw - w) / 2.0, (vh - h) / 2.0)
        };
        Slab {
            clip: &[[0.0, 0.02], [0.985, 0.0], [1.0, 0.97], [0.015, 1.0]],
            background: Gradient::new(
                172.0,
                &[
                    (hsla(250.0, 30.0, 10.0, 0.97), None),
                    (void(), Some(0.4)),
                    (void(), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: drop_print_md(),
        }
        .draw(hud, bx, by, w, h);
        self.title_plate(hud, &pen, bx, by);
        self.content(hud, &pen, bx + pad_x, by + pad_top, inner, true);
        if let Some(p) = post {
            self.stage(hud, &pen, p, vw, vh);
        }
        self.return_rect.set(None);
        if self.return_offered {
            self.return_key(hud, fonts, vw, vh);
        }
    }
}

/// Sizes and fonts at the board's scale.
struct Pen<'a> {
    fonts: &'a Fonts,
    s: f32,
    post: bool,
}

impl Pen<'_> {
    /// Display text at `px` board pixels; the fonts add the HUD's zoom.
    fn display(&self, px: f32, color: [f32; 4]) -> TextStyle {
        self.fonts.display(px * self.s / UI_SCALE, color)
    }
    fn body(&self, px: f32, color: [f32; 4]) -> TextStyle {
        self.fonts.body(px * self.s / UI_SCALE, color)
    }
}

impl Scoreboard {
    /// Whether the post-game screen holds the screen.
    #[must_use]
    pub fn post_game(&self) -> bool {
        self.post_game.is_some()
    }

    /// The way home is on screen: the mouse should be free to press it.
    #[must_use]
    pub fn return_offered(&self) -> bool {
        self.return_offered
    }

    /// Whether a click at (`x`, `y`) CSS pixels pressed "Back to the lobby".
    #[must_use]
    pub fn return_hit(&self, x: f32, y: f32) -> bool {
        self.return_rect
            .get()
            .is_some_and(|[rx, ry, rw, rh]| x >= rx && x <= rx + rw && y >= ry && y <= ry + rh)
    }

    fn title_plate(&self, hud: &mut Hud2d, pen: &Pen<'_>, bx: f32, by: f32) {
        let s = pen.s;
        let (title, size, pad, colors) = match &self.post_game {
            Some(p) => (
                p.title.as_str(),
                30.0,
                [28.0, 44.0, 12.0, 14.0],
                [
                    hsla(45.0, 100.0, 55.0, 1.0),
                    hsla(36.0, 96.0, 46.0, 1.0),
                    hsla(246.0, 42.0, 10.0, 1.0),
                ],
            ),
            None => (
                "SCOREBOARD",
                18.0,
                [20.0, 30.0, 8.0, 10.0],
                [
                    hsla(48.0, 100.0, 60.0, 1.0),
                    hsla(42.0, 96.0, 50.0, 1.0),
                    hsla(250.0, 45.0, 8.0, 1.0),
                ],
            ),
        };
        let style = pen.display(size, colors[2]).spacing(size * s * 0.12);
        let (ascent, descent) = hud.line_metrics(&style);
        let tw = hud.measure(title, &style) + (pad[0] + pad[1]) * s;
        let th = ascent + descent + (pad[2] + pad[3]) * s;
        Slab {
            clip: &[[0.0, 0.08], [1.0, 0.0], [0.96, 1.0], [0.03, 0.92]],
            background: Gradient::new(100.0, &[(colors[0], None), (colors[1], Some(1.0))]),
            skew_deg: -5.0,
            shadow: None,
        }
        .draw(hud, bx + 22.0 * s, by + 16.0 * s, tw, th);
        hud.text(
            bx + 22.0 * s + pad[0] * s,
            by + 16.0 * s + pad[2] * s + ascent,
            title,
            &style,
        );
    }

    /// Lays out (and with `paint`, draws) everything below the title from
    /// (`x`, `y`), `w` wide; returns the height.
    fn content(&self, hud: &mut Hud2d, pen: &Pen<'_>, x: f32, y: f32, w: f32, paint: bool) -> f32 {
        let s = pen.s;
        let mut cy = y;
        match &self.layout {
            Layout::Empty => {}
            Layout::FreeForAll { rows } => {
                cy += self.table(hud, pen, x, cy, w, &FFA_COLUMNS, rows, None, paint);
            }
            Layout::HeadToHead {
                sides,
                figure,
                round_mode,
            } => {
                cy += self.round_line(hud, pen, x, cy, w, paint);
                cy += self.head_to_head(hud, pen, x, cy, w, sides, figure, *round_mode, paint);
            }
            Layout::Teams { teams } => {
                cy += self.round_line(hud, pen, x, cy, w, paint);
                let gap = 24.0 * s;
                let col_w = (w - gap) / 2.0;
                let mut tallest: f32 = 0.0;
                for (i, (name, score, allegiance, rows)) in teams.iter().enumerate() {
                    let cx = x + i as f32 * (col_w + gap);
                    let mut ty = cy;
                    ty += self.team_title(hud, pen, cx, ty, col_w, name, *score, *allegiance, paint);
                    ty += self.table(hud, pen, cx, ty, col_w, &TEAM_COLUMNS, rows, Some(*allegiance), paint);
                    tallest = tallest.max(ty - cy);
                }
                cy += tallest;
            }
        }
        cy += self.spectator_block(hud, pen, x, cy, w, paint);
        cy - y
    }

    /// `ROUND n` over a round mode's board, but not on the post-game.
    fn round_line(&self, hud: &mut Hud2d, pen: &Pen<'_>, x: f32, y: f32, w: f32, paint: bool) -> f32 {
        if self.round == 0 || pen.post {
            return 0.0;
        }
        let s = pen.s;
        let style = pen
            .display(15.0, hsla(45.0, 90.0, 62.0, 1.0))
            .spacing(15.0 * s * 0.18)
            .shadow([s, s], hsla(250.0, 45.0, 4.0, 0.7))
            .align(Align::Center);
        let (a, d) = hud.line_metrics(&style);
        if paint {
            hud.text(x + w / 2.0, y + a, &format!("ROUND {}", self.round), &style);
        }
        a + d + 6.0 * s
    }

    #[allow(clippy::too_many_arguments)]
    fn team_title(
        &self,
        hud: &mut Hud2d,
        pen: &Pen<'_>,
        x: f32,
        y: f32,
        w: f32,
        name: &str,
        score: f64,
        allegiance: Allegiance,
        paint: bool,
    ) -> f32 {
        let s = pen.s;
        let name_style = pen
            .display(15.0, cream())
            .spacing(15.0 * s * 0.15)
            .shadow([s, 2.0 * s], ink());
        let score_style = pen.display(24.0, cream()).shadow([s, 2.0 * s], ink());
        let name = name.to_uppercase();
        let score = format!("{score}");
        let (a, d) = hud.line_metrics(&score_style);
        let name_w = hud.measure(&name, &name_style);
        let score_w = hud.measure(&score, &score_style);
        let pw = name_w + 14.0 * s + score_w + 48.0 * s;
        let ph = 6.0 * s + a + d + 8.0 * s;
        if paint {
            let px = x + (w - pw) / 2.0;
            let side = allegiance_color(allegiance);
            let background = match allegiance {
                Allegiance::Ally => Gradient::new(100.0, &[(side, None), (hsla(255.0, 30.0, 10.0, 0.9), Some(1.0))]),
                Allegiance::Enemy => Gradient::new(
                    115.0,
                    &[
                        (side, None),
                        (hsla(210.0, 62.0, 18.0, 0.92), Some(0.7)),
                        (hsla(210.0, 62.0, 18.0, 0.92), Some(1.0)),
                    ],
                ),
            };
            Slab {
                clip: &[[0.02, 0.0], [1.0, 0.08], [0.97, 1.0], [0.0, 0.9]],
                background,
                skew_deg: -5.0,
                shadow: None,
            }
            .draw(hud, px, y, pw, ph);
            let baseline = y + 6.0 * s + a;
            hud.text(px + 24.0 * s, baseline, &name, &name_style);
            hud.text(px + 24.0 * s + name_w + 14.0 * s, baseline, &score, &score_style);
        }
        ph + 10.0 * s
    }

    /// The header and the rows of one table.
    #[allow(clippy::too_many_arguments)]
    fn table(
        &self,
        hud: &mut Hud2d,
        pen: &Pen<'_>,
        x: f32,
        y: f32,
        w: f32,
        columns: &[Column],
        rows: &[BoardEntry],
        allegiance: Option<Allegiance>,
        paint: bool,
    ) -> f32 {
        let s = pen.s;
        let header_style = pen.body(11.0, cream_dim()).spacing(11.0 * s * 0.18);
        let (ha, hd) = hud.line_metrics(&header_style);
        let header_h = 16.0 * s + ha + hd;
        let pad_x = if pen.post { 18.0 * s } else { 14.0 * s };
        let edges = column_edges(columns, x + pad_x, x + w - pad_x);
        if paint {
            for (i, column) in columns.iter().enumerate() {
                let (cx, align) = column_anchor(&edges, *column, i);
                hud.text_in_box(cx, y, header_h, column.title(), &header_style.align(align));
            }
            hud.rect(x, y + header_h - 1.0, w, 1.0, hsla(42.0, 96.0, 55.0, 0.35));
        }
        let mut cy = y + header_h;
        let (row_px, score_px, ping_px, row_pad) = if pen.post {
            (20.0, 26.0, 14.0, 12.0)
        } else {
            (15.0, 15.0, 12.0, 8.0)
        };
        let row_style = pen.body(row_px, text());
        let (ra, rd) = hud.line_metrics(&pen.display(score_px, cream()));
        let row_h = 2.0 * row_pad * s + ra + rd;
        for (i, row) in rows.iter().enumerate() {
            if paint {
                let me = row.entry.player_id == self.local_player_id;
                let carrier = self.carrier.as_deref() == Some(row.entry.player_id.as_str()) && allegiance.is_some();
                let fade = if row.departed { 0.45 } else { 1.0 };
                let gold = hsla(42.0, 96.0, 56.0, 1.0);
                if me {
                    hud.gradient_rect(
                        x,
                        cy,
                        w * 0.55,
                        row_h,
                        hsla(42.0, 96.0, 56.0, 0.22),
                        hsla(42.0, 96.0, 50.0, 0.08),
                    );
                    hud.gradient_rect(
                        x + w * 0.55,
                        cy,
                        w * 0.45,
                        row_h,
                        hsla(42.0, 96.0, 50.0, 0.08),
                        hsla(42.0, 96.0, 50.0, 0.0),
                    );
                } else if carrier {
                    hud.gradient_rect(
                        x,
                        cy,
                        w * 0.62,
                        row_h,
                        [0.96, 0.77, 0.09, 0.16],
                        [0.96, 0.77, 0.09, 0.0],
                    );
                } else if i % 2 == 0 {
                    hud.rect(x, cy, w, row_h, hsla(250.0, 30.0, 14.0, 0.35 * fade));
                }
                hud.rect(x, cy + row_h - 1.0, w, 1.0, hsla(250.0, 30.0, 25.0, 0.35));
                // The left edge: yours gold, the bomb's gold, else the side's.
                let edge = if me || carrier {
                    Some(if carrier { [0.96, 0.77, 0.09, 1.0] } else { gold })
                } else {
                    allegiance.map(allegiance_color)
                };
                if let Some(c) = edge {
                    hud.rect(x, cy, 3.0, row_h, c);
                }
                let faded = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * fade];
                for (k, column) in columns.iter().enumerate() {
                    let (cx, align) = column_anchor(&edges, *column, k);
                    let (label, style) = match column {
                        Column::Name => {
                            let color = if me { cream() } else { text() };
                            (row.entry.name.clone(), tinted(row_style, faded(color)))
                        }
                        Column::Score => {
                            let color = if me { hsla(42.0, 96.0, 62.0, 1.0) } else { cream() };
                            (
                                format!("{}", row.entry.score),
                                pen.display(score_px, faded(color)).shadow([s, 2.0 * s], ink()),
                            )
                        }
                        Column::Deaths => (
                            format!("{}", row.entry.deaths),
                            tinted(row_style, faded(if pen.post { muted() } else { text() })),
                        ),
                        Column::Ping => (
                            ping_text(row),
                            pen.body(ping_px, faded(if pen.post { dim() } else { cream_dim() })),
                        ),
                    };
                    let mut name_x = cx;
                    if *column == Column::Name && carrier {
                        // `.fps-scoreboard-carrier`: the bomb's mark.
                        let r = row_px * s * 0.31;
                        let (cxm, cym) = (cx + r, cy + row_h / 2.0);
                        hud.radial_gradient(cxm, cym, &[(r, [0.96, 0.77, 0.09, 1.0]), (r, [0.96, 0.77, 0.09, 1.0])]);
                        name_x += 2.0 * r + row_px * s * 0.5;
                    }
                    hud.text_in_box(name_x, cy, row_h, &label, &style.align(align));
                }
            }
            cy += row_h;
        }
        cy - y
    }

    #[allow(clippy::too_many_arguments)]
    fn head_to_head(
        &self,
        hud: &mut Hud2d,
        pen: &Pen<'_>,
        x: f32,
        y: f32,
        w: f32,
        sides: &[(Option<BoardEntry>, Allegiance); 2],
        figure: &str,
        round_mode: bool,
        paint: bool,
    ) -> f32 {
        let s = pen.s;
        let (name_px, figure_px) = if pen.post { (28.0, 64.0) } else { (22.0, 44.0) };
        let figure_style = pen
            .display(figure_px, cream())
            .shadow([2.0 * s, 3.0 * s], ink())
            .align(Align::Center);
        let label_style = pen
            .body(11.0, hsla(45.0, 90.0, 62.0, 1.0))
            .spacing(11.0 * s * 0.22)
            .align(Align::Center);
        let name_style = pen.body(name_px, cream());
        let line_style = pen
            .body(13.0, if pen.post { muted() } else { text() })
            .spacing(13.0 * s * 0.06);
        let ping_style = pen
            .body(12.0, if pen.post { dim() } else { cream_dim() })
            .spacing(12.0 * s * 0.08);
        let (na, nd) = hud.line_metrics(&name_style);
        let (la, ld) = hud.line_metrics(&line_style);
        let (pa, pd) = hud.line_metrics(&ping_style);
        let side_h = 2.0 * 10.0 * s + na + nd + 4.0 * s + la + ld + 4.0 * s + pa + pd;
        let (fa, fd) = hud.line_metrics(&figure_style);
        let (ba, bd) = hud.line_metrics(&label_style);
        let score_h = fa + fd + 2.0 * s + ba + bd;
        let h = 10.0 * s + side_h.max(score_h) + 6.0 * s;
        if !paint {
            return h;
        }
        let gap = 20.0 * s;
        let figure_w = hud
            .measure(figure, &figure_style)
            .max(hud.measure("ROUNDS", &label_style));
        let side_w = ((w - figure_w - 2.0 * gap) / 2.0).max(0.0);
        let top = y + 10.0 * s;
        let mid = top + side_h.max(score_h) / 2.0;
        // The figure, and what it counts.
        let fy = mid - score_h / 2.0;
        hud.text(x + w / 2.0, fy + fa, figure, &figure_style);
        let label = if round_mode { "ROUNDS" } else { "FRAGS" };
        hud.text(x + w / 2.0, fy + fa + fd + 2.0 * s + ba, label, &label_style);
        for (i, (player, allegiance)) in sides.iter().enumerate() {
            let left = i == 0;
            let sx = if left { x } else { x + w - side_w };
            let sy = mid - side_h / 2.0;
            let fade = if player.as_ref().is_some_and(|p| p.departed) {
                0.45
            } else {
                1.0
            };
            let faded = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * fade];
            let bar_x = if left { sx } else { sx + side_w - 3.0 };
            hud.rect(bar_x, sy, 3.0, side_h, faded(allegiance_color(*allegiance)));
            let (tx, align) = if left {
                (sx + 16.0 * s, Align::Left)
            } else {
                (sx + side_w - 16.0 * s, Align::Right)
            };
            let me = player
                .as_ref()
                .is_some_and(|p| p.entry.player_id == self.local_player_id);
            let name = player
                .as_ref()
                .map_or("Waiting\u{2026}".to_owned(), |p| p.entry.name.clone());
            let name_color = if me { hsla(42.0, 96.0, 62.0, 1.0) } else { cream() };
            let mut ly = sy + 10.0 * s + na;
            hud.text(tx, ly, &name, &tinted(name_style, faded(name_color)).align(align));
            if let Some(p) = player {
                let line = if round_mode {
                    format!("{} frags \u{b7} {} deaths", p.entry.score, p.entry.deaths)
                } else {
                    format!("{} deaths", p.entry.deaths)
                };
                ly += nd + 4.0 * s + la;
                let line_color = if pen.post { muted() } else { text() };
                hud.text(tx, ly, &line, &tinted(line_style, faded(line_color)).align(align));
                ly += ld + 4.0 * s + pa;
                let ping_color = if pen.post { dim() } else { cream_dim() };
                hud.text(
                    tx,
                    ly,
                    &ping_text(p),
                    &tinted(ping_style, faded(ping_color)).align(align),
                );
            }
        }
        h
    }

    /// `appendSpectators`: "SPECTATORS (n)" and their chips.
    fn spectator_block(&self, hud: &mut Hud2d, pen: &Pen<'_>, x: f32, y: f32, w: f32, paint: bool) -> f32 {
        if self.spectators.is_empty() {
            return 0.0;
        }
        let s = pen.s;
        let title_style = pen.body(11.0, cream_dim()).spacing(11.0 * s * 0.2).align(Align::Center);
        let (ta, td) = hud.line_metrics(&title_style);
        let chip_style = pen.body(12.0, muted());
        let (ca, cd) = hud.line_metrics(&chip_style);
        let chip_h = 3.0 * s + ca + cd + 4.0 * s;
        let gap = 8.0 * s;
        // Chips wrap, centred: lay them out in lines first.
        let widths: Vec<f32> = self
            .spectators
            .iter()
            .map(|e| hud.measure(&e.entry.name, &chip_style) + 24.0 * s)
            .collect();
        let mut lines: Vec<Vec<usize>> = vec![Vec::new()];
        let mut line_w = 0.0;
        for (i, cw) in widths.iter().enumerate() {
            let last = lines.last_mut().expect("one line");
            if !last.is_empty() && line_w + gap + cw > w {
                lines.push(vec![i]);
                line_w = *cw;
            } else {
                if !last.is_empty() {
                    line_w += gap;
                }
                line_w += cw;
                lines.last_mut().expect("one line").push(i);
            }
        }
        let h = 14.0 * s + 10.0 * s + ta + td + 8.0 * s + lines.len() as f32 * (chip_h + gap) - gap;
        if !paint {
            return h;
        }
        let top = y + 14.0 * s;
        hud.rect(x, top, w, 1.0, hsla(190.0, 100.0, 72.0, 0.18));
        let mut cy = top + 10.0 * s;
        hud.text(
            x + w / 2.0,
            cy + ta,
            &format!("SPECTATORS ({})", self.spectators.len()),
            &title_style,
        );
        cy += ta + td + 8.0 * s;
        for line in &lines {
            let total: f32 = line.iter().map(|i| widths[*i]).sum::<f32>() + gap * (line.len() as f32 - 1.0);
            let mut cx = x + (w - total) / 2.0;
            for i in line {
                let e = &self.spectators[*i];
                let me = e.entry.player_id == self.local_player_id;
                let background = if me {
                    Gradient::new(
                        100.0,
                        &[
                            (hsla(42.0, 96.0, 56.0, 0.4), None),
                            (hsla(42.0, 96.0, 46.0, 0.2), Some(1.0)),
                        ],
                    )
                } else {
                    Gradient::solid(hsla(250.0, 30.0, 14.0, 0.75))
                };
                Slab {
                    clip: &[[0.04, 0.0], [1.0, 0.06], [0.96, 1.0], [0.0, 0.92]],
                    background,
                    skew_deg: -5.0,
                    shadow: None,
                }
                .draw(hud, cx, cy, widths[*i], chip_h);
                let color = if me { cream() } else { muted() };
                hud.text(
                    cx + 12.0 * s,
                    cy + 3.0 * s + ca,
                    &e.entry.name,
                    &tinted(chip_style, color),
                );
                cx += widths[*i] + gap;
            }
            cy += chip_h + gap;
        }
        h
    }

    /// The stage caption: the winner's giant name and stat chips, top left.
    fn stage(&self, hud: &mut Hud2d, pen: &Pen<'_>, p: &PostGame, vw: f32, vh: f32) {
        let s = pen.s;
        let mut y = 0.08 * vh;
        let x = 0.04 * vw;
        if let Some(name) = &p.winner_name {
            let style = pen
                .display(64.0, hsla(45.0, 100.0, 62.0, 0.34))
                .spacing(64.0 * s * 0.06)
                .shadow([2.0 * s, 3.0 * s], hsla(250.0, 45.0, 4.0, 0.5));
            let (a, d) = hud.line_metrics(&style);
            hud.text(x, y + a, &name.to_uppercase(), &style);
            y += a + d + 18.0 * s;
        }
        if self.winner_stats.is_empty() {
            return;
        }
        let label_style = pen.body(14.0, hsla(246.0, 30.0, 30.0, 1.0)).spacing(14.0 * s * 0.12);
        let value_style = pen.display(24.0, hsla(246.0, 42.0, 12.0, 1.0));
        let (va, vd) = hud.line_metrics(&value_style);
        let chip_h = 6.0 * s + va + vd + 8.0 * s;
        let mut cx = x;
        for (label, value) in &self.winner_stats {
            let label = label.to_uppercase();
            let lw = hud.measure(&label, &label_style);
            let cw = 18.0 * s + lw + 14.0 * s + hud.measure(value, &value_style) + 18.0 * s;
            if cx + cw > 0.42 * vw && cx > x {
                cx = x;
                y += chip_h + 10.0 * s;
            }
            Slab {
                clip: &[[0.02, 0.0], [1.0, 0.0], [0.98, 1.0], [0.0, 1.0]],
                background: Gradient::new(
                    178.0,
                    &[
                        (cream(), None),
                        (hsla(41.0, 92.0, 85.0, 1.0), Some(0.78)),
                        (hsla(41.0, 92.0, 85.0, 1.0), Some(1.0)),
                    ],
                ),
                skew_deg: -4.0,
                shadow: drop_print_sm(),
            }
            .draw(hud, cx, y, cw, chip_h);
            let baseline = y + 6.0 * s + va;
            hud.text(cx + 18.0 * s, baseline, &label, &label_style);
            hud.text(cx + 18.0 * s + lw + 14.0 * s, baseline, value, &value_style);
            cx += cw + 10.0 * s;
        }
    }

    /// `.fps-scoreboard-return.raised-key--cta`: on the board's right edge,
    /// 12vh up.
    fn return_key(&self, hud: &mut Hud2d, fonts: &Fonts, vw: f32, vh: f32) {
        let label = "BACK TO THE LOBBY";
        let style = fonts.display(30.0 / UI_SCALE, hsla(250.0, 45.0, 8.0, 1.0)).spacing(3.0);
        let (a, d) = hud.line_metrics(&style);
        let w = 46.0 + hud.measure(label, &style) + 52.0;
        let h = 76.0_f32.max(a + d + 20.0);
        let depth = 9.6;
        let x = vw - 0.05 * vw - w - depth * 0.75;
        let y = vh - 0.12 * vh - h - depth;
        let cut = [[0.05, 0.0], [1.0, 0.0], [0.95, 1.0], [0.0, 1.0]];
        // The key's side, under and behind the face.
        Slab {
            clip: &cut,
            background: Gradient::new(
                180.0,
                &[
                    (hsla(36.0, 92.0, 40.0, 1.0), None),
                    (hsla(28.0, 88.0, 25.0, 1.0), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: Some(([depth * 0.75 / UI_SCALE + 2.0, 5.0], hsla(250.0, 70.0, 3.0, 0.62))),
        }
        .draw(hud, x + depth * 0.75, y + depth, w, h);
        Slab {
            clip: &cut,
            background: Gradient::new(
                180.0,
                &[
                    (hsla(51.0, 100.0, 73.0, 1.0), None),
                    (hsla(46.0, 100.0, 57.0, 1.0), Some(0.58)),
                    (hsla(42.0, 100.0, 50.0, 1.0), Some(1.0)),
                ],
            ),
            skew_deg: 0.0,
            shadow: None,
        }
        .draw(hud, x, y, w, h);
        hud.rect(x + 0.05 * w, y, 0.95 * w, 2.0, hsla(0.0, 0.0, 100.0, 0.55));
        hud.text(x + 46.0, y + (h + a - d) / 2.0, label, &style);
        self.return_rect.set(Some([x, y, w + depth, h + depth]));
    }
}

/// A style in another colour.
fn tinted(style: TextStyle, color: [f32; 4]) -> TextStyle {
    TextStyle { color, ..style }
}

fn ink() -> [f32; 4] {
    hsla(250.0, 40.0, 5.0, 1.0)
}
fn muted() -> [f32; 4] {
    hsla(220.0, 10.0, 55.0, 1.0)
}
fn dim() -> [f32; 4] {
    hsla(220.0, 10.0, 35.0, 1.0)
}

/// `writePings`: "<n>ms", or LEFT for a departed player.
fn ping_text(row: &BoardEntry) -> String {
    if row.departed {
        "LEFT".to_owned()
    } else {
        format!("{}ms", row.entry.ping)
    }
}

/// The column edges, by flex.
fn column_edges(columns: &[Column], left: f32, right: f32) -> Vec<f32> {
    let total: f32 = columns.iter().map(|c| c.flex()).sum();
    let mut edges = vec![left];
    for c in columns {
        edges.push(edges.last().copied().unwrap_or(left) + (right - left) * c.flex() / total);
    }
    edges
}

/// Where a column's text goes: the name left, the ping right, the rest
/// centred.
fn column_anchor(edges: &[f32], column: Column, i: usize) -> (f32, Align) {
    match column {
        Column::Name => (edges[i], Align::Left),
        Column::Ping => (edges[i + 1], Align::Right),
        _ => ((edges[i] + edges[i + 1]) / 2.0, Align::Center),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::json::parse;

    fn json(text: &str) -> Value {
        parse(text).expect("json")
    }

    fn entry(id: &str, score: f64) -> ScoreboardEntry {
        ScoreboardEntry {
            player_id: id.to_owned(),
            name: id.to_owned(),
            score,
            ..ScoreboardEntry::default()
        }
    }

    #[test]
    fn titles_the_post_game_by_its_winner() {
        assert_eq!(
            post_game_title(Some(&json(r#"{"kind": "player", "name": "Wraith"}"#))),
            "WRAITH WINS!"
        );
        assert_eq!(post_game_title(Some(&json(r#"{"kind": "draw"}"#))), "DRAW!");
        assert_eq!(post_game_title(None), "DRAW!");
    }

    #[test]
    fn the_winner_stats_divide_frags_by_deaths() {
        let mut e = entry("player_1", 12.0);
        e.frags = Some(9.0);
        e.deaths = 4.0;
        assert_eq!(
            winner_stats(&e),
            vec![
                ("Score", "12".into()),
                ("Frags", "9".into()),
                ("Deaths", "4".into()),
                ("K/D", "2.25".into())
            ]
        );
        e.deaths = 0.0;
        assert_eq!(winner_stats(&e)[3], ("K/D", "9.00".into()));
        e.frags = None;
        assert_eq!(winner_stats(&e), vec![("Score", "12".into()), ("Deaths", "0".into())]);
    }

    #[test]
    fn a_player_who_leaves_the_post_game_keeps_a_row() {
        let mut roster = BTreeMap::new();
        let both = [entry("player_1", 5.0), entry("player_2", 3.0)];
        let rows = collect_entries(&both, Some(&mut roster));
        assert!(rows.iter().all(|r| !r.departed));
        let rows = collect_entries(&both[..1], Some(&mut roster));
        assert_eq!(rows.len(), 2);
        assert!(rows[1].departed && rows[1].entry.player_id == "player_2");
        assert_eq!(ping_text(&rows[1]), "LEFT");
        // Off the post-game, nobody is remembered.
        assert_eq!(collect_entries(&both[..1], None).len(), 1);
    }
}
