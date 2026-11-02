//! The scoreboard (wash-fps `client/hud/widgets/Scoreboard.ts`, styles
//! `.fps-scoreboard*`): held open with Tab, and locked open after the
//! match. Free-for-all lists everyone; team modes list each side under its
//! name and score. The post-game stage and the duel's versus card are not
//! drawn yet.

use shagger::ui::hud_2d::{Align, Hud2d, TextStyle};

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, cream, cream_dim, drop_print_md, hsla, ink_text_sm, void};
use crate::client::hud_state::ScoreboardEntry;
use crate::client::match_state_manager::MatchPhase;

/// `--scoreboard-scale`: the overlay sits outside the HUD's zoom.
const SCALE: f32 = 1.5;
const SIDES: [&str; 2] = ["red", "blue"];

#[derive(Debug, Clone, Default)]
struct Section {
    title: Option<(String, f64)>,
    rows: Vec<ScoreboardEntry>,
}

/// `FPSScoreboard`.
#[derive(Debug, Default)]
pub struct Scoreboard {
    /// Tab held.
    pub held: bool,
    visible: bool,
    title: String,
    sections: Vec<Section>,
    spectators: Vec<String>,
    with_deaths: bool,
    subject: String,
}

fn text_color() -> [f32; 4] {
    hsla(180.0, 100.0, 95.0, 1.0)
}

impl FpsHudWidget for Scoreboard {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        let post_game = m.phase() == MatchPhase::PostGame;
        self.visible = self.held || post_game;
        if !self.visible {
            return;
        }
        let state = frame.state;
        self.title = if post_game { "FINAL STANDINGS" } else { "SCOREBOARD" }.to_owned();
        self.subject = state.subject().to_owned();
        self.sections.clear();
        self.spectators = state
            .scoreboard
            .iter()
            .filter(|e| e.team_id.as_deref() == Some("spectator"))
            .map(|e| e.name.clone())
            .collect();
        let players = state
            .scoreboard
            .iter()
            .filter(|e| e.team_id.as_deref() != Some("spectator"));
        if state.team_based {
            self.with_deaths = false;
            for side in SIDES {
                let name = state
                    .team_names
                    .get(side)
                    .cloned()
                    .unwrap_or_else(|| format!("Team {side}"));
                let score = state.team_scores.get(side).copied().unwrap_or(0.0);
                self.sections.push(Section {
                    title: Some((name.to_uppercase(), score)),
                    rows: players
                        .clone()
                        .filter(|e| e.team_id.as_deref() == Some(side))
                        .cloned()
                        .collect(),
                });
            }
        } else {
            self.with_deaths = true;
            self.sections.push(Section {
                title: None,
                rows: players.cloned().collect(),
            });
        }
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Fills the screen from (`x`, `y`) with the overlay and centres the
    /// board.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, _x: f32, _y: f32) {
        if !self.visible {
            return;
        }
        let s = SCALE;
        let [vw, vh] = hud.size();
        hud.rect(0.0, 0.0, vw, vh, hsla(248.0, 30.0, 4.0, 0.82));
        let row_h = 15.0 * s * 1.25 + 16.0 * s;
        let header_h = 11.0 * s * 1.25 + 16.0 * s;
        let mut content_h = 0.0;
        for section in &self.sections {
            if section.title.is_some() {
                content_h += 15.0 * s * 1.3 + 14.0 * s + 10.0 * s;
            }
            content_h += header_h + row_h * section.rows.len() as f32 + 12.0 * s;
        }
        if !self.spectators.is_empty() {
            content_h += header_h + row_h;
        }
        let w = (680.0 * s).min(vw * 0.92);
        let h = (64.0 * s + content_h + 30.0 * s).min(vh * 0.82);
        let (bx, by) = ((vw - w) / 2.0, (vh - h) / 2.0);
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
        // The title tab.
        let title_style = fonts
            .display(18.0 / 1.15 * s, hsla(250.0, 45.0, 8.0, 1.0))
            .spacing(18.0 * s * 0.12);
        let tw = hud.measure(&self.title, &title_style) + 50.0 * s;
        let th = 18.0 * s + 18.0 * s;
        Slab {
            clip: &[[0.0, 0.08], [1.0, 0.0], [0.96, 1.0], [0.03, 0.92]],
            background: Gradient::new(
                100.0,
                &[
                    (hsla(48.0, 100.0, 60.0, 1.0), None),
                    (hsla(42.0, 96.0, 50.0, 1.0), None),
                ],
            ),
            skew_deg: -5.0,
            shadow: None,
        }
        .draw(hud, bx + 22.0 * s, by + 16.0 * s, tw, th);
        hud.text_in_box(bx + 42.0 * s, by + 24.0 * s, th - 18.0 * s, &self.title, &title_style);

        let (left, right) = (bx + 38.0 * s, bx + w - 38.0 * s);
        let mut y = by + 64.0 * s;
        let bottom = by + h - 30.0 * s;
        for section in &self.sections {
            if let Some((name, score)) = &section.title {
                let style = ink_text_sm(fonts.display(15.0 / 1.15 * s, cream()).spacing(15.0 * s * 0.15));
                let line = format!("{name}   {score}");
                hud.text_in_box(
                    (left + right) / 2.0,
                    y,
                    15.0 * s * 1.3 + 14.0 * s,
                    &line,
                    &style.align(Align::Center),
                );
                y += 15.0 * s * 1.3 + 14.0 * s + 10.0 * s;
            }
            y = self.header(hud, fonts, left, right, y, header_h);
            for (i, entry) in section.rows.iter().enumerate() {
                if y + row_h > bottom {
                    break;
                }
                let me = entry.player_id == self.subject || entry.entity_id == self.subject;
                if me {
                    hud.gradient_rect(
                        left,
                        y,
                        right - left,
                        row_h,
                        hsla(42.0, 96.0, 56.0, 0.22),
                        hsla(42.0, 96.0, 50.0, 0.0),
                    );
                } else if i % 2 == 0 {
                    hud.rect(left, y, right - left, row_h, hsla(250.0, 30.0, 14.0, 0.35));
                }
                hud.rect(left, y + row_h - 1.0, right - left, 1.0, hsla(250.0, 30.0, 25.0, 0.35));
                self.row(hud, fonts, left, right, y, row_h, entry);
                y += row_h;
            }
            y += 12.0 * s;
        }
        if !self.spectators.is_empty() && y + header_h + row_h <= bottom {
            let style = fonts.body(11.0 / 1.15 * s, cream_dim()).spacing(11.0 * s * 0.18);
            let title = format!("SPECTATORS ({})", self.spectators.len());
            hud.text_in_box(left + 14.0 * s, y, header_h, &title, &style);
            y += header_h;
            let names = self.spectators.join(", ");
            let style = fonts.body(15.0 / 1.15 * s, hsla(220.0, 10.0, 55.0, 1.0));
            hud.text_in_box(left + 14.0 * s, y, row_h, &names, &style);
        }
    }
}

impl Scoreboard {
    /// The column edges: name flex 3, the rest flex 1 each.
    fn columns(&self, left: f32, right: f32) -> Vec<f32> {
        let s = SCALE;
        let (l, r) = (left + 14.0 * s, right - 14.0 * s);
        let flex: &[f32] = if self.with_deaths {
            &[3.0, 1.0, 1.0, 1.0]
        } else {
            &[3.0, 1.0, 1.0]
        };
        let total: f32 = flex.iter().sum();
        let mut edges = vec![l];
        for f in flex {
            edges.push(edges.last().copied().unwrap_or(l) + (r - l) * f / total);
        }
        edges
    }

    fn header(&self, hud: &mut Hud2d, fonts: &Fonts, left: f32, right: f32, y: f32, h: f32) -> f32 {
        let s = SCALE;
        let style = fonts.body(11.0 / 1.15 * s, cream_dim()).spacing(11.0 * s * 0.18);
        let c = self.columns(left, right);
        let titles: &[&str] = if self.with_deaths {
            &["PLAYER", "SCORE", "DEATHS", "PING"]
        } else {
            &["PLAYER", "SCORE", "PING"]
        };
        for (i, title) in titles.iter().enumerate() {
            let (x, align) = column_anchor(&c, i, titles.len());
            hud.text_in_box(x, y, h, title, &style.align(align));
        }
        hud.rect(left, y + h - 1.0, right - left, 1.0, hsla(42.0, 96.0, 55.0, 0.35));
        y + h
    }

    #[allow(clippy::too_many_arguments)]
    fn row(&self, hud: &mut Hud2d, fonts: &Fonts, left: f32, right: f32, y: f32, h: f32, e: &ScoreboardEntry) {
        let s = SCALE;
        let c = self.columns(left, right);
        let body = |px: f32, color| -> TextStyle { fonts.body(px / 1.15 * s, color) };
        let mut cells: Vec<(String, TextStyle)> = vec![(e.name.clone(), body(15.0, text_color()))];
        cells.push((format!("{}", e.score), fonts.display(15.0 / 1.15 * s, cream())));
        if self.with_deaths {
            cells.push((format!("{}", e.deaths), body(15.0, hsla(220.0, 10.0, 55.0, 1.0))));
        }
        cells.push((format!("{}ms", e.ping), body(12.0, hsla(220.0, 10.0, 35.0, 1.0))));
        let n = cells.len();
        for (i, (text, style)) in cells.into_iter().enumerate() {
            let (x, align) = column_anchor(&c, i, n);
            hud.text_in_box(x, y, h, &text, &style.align(align));
        }
    }
}

/// Where a column's text goes: the name left, the last right, the rest
/// centred.
fn column_anchor(edges: &[f32], i: usize, n: usize) -> (f32, Align) {
    if i == 0 {
        (edges[0], Align::Left)
    } else if i == n - 1 {
        (edges[n], Align::Right)
    } else {
        ((edges[i] + edges[i + 1]) / 2.0, Align::Center)
    }
}
