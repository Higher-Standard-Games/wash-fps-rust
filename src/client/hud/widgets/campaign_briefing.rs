//! What to do in a campaign mission (wash-fps
//! `client/hud/widgets/CampaignBriefing.ts`, styles
//! `.fps-campaign-briefing`): a yellow plate in the upper left while the
//! opening countdown holds everyone still (and a round mode's first freeze
//! after it). It goes when the countdown ends, and never shows in a match
//! that is not a campaign one.

use shagger::ui::hud_2d::{Hud2d, TextStyle};

use crate::client::campaign::campaign_session::campaign_mission_id;
use crate::client::campaign::campaign_story::{CampaignBriefing as Briefing, campaign_story};
use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, Gradient, Slab, UI_SCALE, hsla};
use crate::client::match_state_manager::MatchPhase;

/// The slide in and out (`transition: 0.3s ease`).
const FADE_SECONDS: f32 = 0.3;

/// `FPSCampaignBriefing`.
#[derive(Debug, Default)]
pub struct CampaignBriefing {
    briefing: Option<&'static Briefing>,
    /// `story.line`: one line of story under the goal.
    line: &'static str,
    /// 0 hidden, 1 shown: the slide's progress.
    shown: f32,
    show: bool,
}

fn ink() -> [f32; 4] {
    hsla(250.0, 45.0, 8.0, 1.0)
}

/// `text` broken into lines no wider than `width`.
fn wrap(hud: &Hud2d, text: &str, style: &TextStyle, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && hud.measure(&candidate, style) > width {
            lines.push(std::mem::replace(&mut line, word.to_owned()));
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

impl FpsHudWidget for CampaignBriefing {
    fn update(&mut self, frame: &HudFrame<'_>) {
        let m = frame.match_state;
        // The opening countdown only: a round mode counts down again before
        // every later round.
        let story = (m.phase() == MatchPhase::Countdown && m.round() <= 1)
            .then(campaign_mission_id)
            .flatten()
            .and_then(|id| campaign_story(&id));
        self.show = story.is_some();
        if let Some(s) = story {
            self.briefing = Some(&s.briefing);
            self.line = s.line;
        }
        let step = frame.dt as f32 / FADE_SECONDS;
        self.shown = if self.show {
            (self.shown + step).min(1.0)
        } else {
            (self.shown - step).max(0.0)
        };
    }

    fn size(&self, _hud: &Hud2d, _fonts: &Fonts) -> [f32; 2] {
        [0.0, 0.0]
    }

    /// Placed by its own style: 15vh in from the left and the top.
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, _x: f32, _y: f32) {
        let Some(briefing) = self.briefing else { return };
        if self.shown <= 0.0 {
            return;
        }
        let s = UI_SCALE;
        let [vw, vh] = hud.size();
        // `ease`, roughly.
        let t = self.shown * self.shown * (3.0 - 2.0 * self.shown);
        let alpha = t;
        let fade = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * alpha];
        let goal_style = fonts.display(26.0, fade(ink())).spacing(26.0 * s * 0.04);
        let point_style = fonts.body(18.0, fade(ink()));
        // `.fps-campaign-briefing__line`: 17 px, semibold, italic.
        let line_style = fonts.body(17.0, fade(ink()));
        let (pad_t, pad_r, pad_b, pad_l) = (18.0 * s, 30.0 * s, 20.0 * s, 24.0 * s);
        let max_w = (vw * 0.3).min(520.0 * s);
        let inner = max_w - pad_l - pad_r;
        let bullet = 22.0 * s;
        let goal = wrap(hud, &briefing.goal.to_uppercase(), &goal_style, inner);
        let story = wrap(hud, self.line, &line_style, inner);
        let story_line = 17.0 * s * 1.3;
        let story_h = if story.is_empty() {
            0.0
        } else {
            8.0 * s + story.len() as f32 * story_line
        };
        let points: Vec<Vec<String>> = briefing
            .points
            .iter()
            .map(|p| wrap(hud, p, &point_style, inner - bullet))
            .collect();
        let goal_line = 26.0 * s * 1.1;
        let point_line = 18.0 * s * 1.45;
        let point_rows: usize = points.iter().map(Vec::len).sum();
        let width = goal
            .iter()
            .map(|l| hud.measure(l, &goal_style))
            .chain(story.iter().map(|l| hud.measure(l, &line_style)))
            .chain(points.iter().flatten().map(|l| bullet + hud.measure(l, &point_style)))
            .fold(0.0f32, f32::max)
            + pad_l
            + pad_r;
        let height =
            pad_t + goal.len() as f32 * goal_line + story_h + 12.0 * s + point_rows as f32 * point_line + pad_b;
        let x = 0.15 * vh - 24.0 * (1.0 - t);
        let y = 0.15 * vh;
        Slab {
            clip: &[[0.0, 0.03], [1.0, 0.0], [0.97, 1.0], [0.02, 0.97]],
            background: Gradient::new(
                100.0,
                &[
                    (fade(hsla(48.0, 100.0, 60.0, 1.0)), None),
                    (fade(hsla(42.0, 96.0, 50.0, 1.0)), Some(1.0)),
                ],
            ),
            skew_deg: -4.0,
            shadow: None,
        }
        .draw(hud, x, y, width, height);
        // The skew leans the text with the plate about its top.
        let lean = (4f32).to_radians().tan();
        let (ga, gd) = hud.line_metrics(&goal_style);
        let mut line_y = y + pad_t;
        for l in &goal {
            let baseline = line_y + (goal_line + ga - gd) / 2.0;
            hud.text(x + pad_l - lean * (baseline - y), baseline, l, &goal_style);
            line_y += goal_line;
        }
        if !story.is_empty() {
            line_y += 8.0 * s;
            let (la, ld) = hud.line_metrics(&line_style);
            for l in &story {
                let baseline = line_y + (story_line + la - ld) / 2.0;
                hud.text(x + pad_l - lean * (baseline - y), baseline, l, &line_style);
                line_y += story_line;
            }
        }
        line_y += 12.0 * s;
        let (pa, pd) = hud.line_metrics(&point_style);
        for point in &points {
            for (i, l) in point.iter().enumerate() {
                let baseline = line_y + (point_line + pa - pd) / 2.0;
                let left = x + pad_l - lean * (baseline - y);
                if i == 0 {
                    hud.text(left + bullet * 0.25, baseline, "•", &point_style);
                }
                hud.text(left + bullet, baseline, l, &point_style);
                line_y += point_line;
            }
        }
    }
}
