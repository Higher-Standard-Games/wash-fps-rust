//! wash-fps `client/PlayerModelVisualSettings.ts`: team tints and outlines
//! for third-person player models, from the `cl_team_*` and `cl_player_*`
//! cvars, and `FfaColors`: each free-for-all opponent in a colour of their
//! own.

use std::collections::HashMap;

use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};
use shagger::rendering::glam::Vec3;
use shagger::shared::spread_utils::mulberry32;

use crate::client::settings::settings_model::{TEAM_COLORS, TeamColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerTeamSlot {
    Ally,
    Enemy,
    Ffa,
}

/// `PLAYER_MODEL_VISUAL_DEFAULTS`.
#[must_use]
pub fn player_model_visual_default(slot: PlayerTeamSlot) -> Vec3 {
    match slot {
        PlayerTeamSlot::Ally => Vec3::new(1.55, 0.2, 1.35),
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => Vec3::new(0.1, 1.15, 1.85),
    }
}

/// `PLAYER_MODEL_OUTLINE_DEFAULTS`.
#[must_use]
pub fn player_model_outline_default(slot: PlayerTeamSlot) -> Vec3 {
    match slot {
        PlayerTeamSlot::Ally => Vec3::new(1.85, 0.28, 1.65),
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => Vec3::new(0.16, 1.35, 2.2),
    }
}

/// `rgbToCvar`: `"r g b"` as JavaScript writes the numbers.
fn rgb_to_cvar(c: [f64; 3]) -> String {
    let n = shagger::shared::js::js_number_to_string;
    format!("{} {} {}", n(c[0]), n(c[1]), n(c[2]))
}

/// The defaults at JavaScript's precision, which the cvars are written in.
fn visual_default(slot: PlayerTeamSlot) -> [f64; 3] {
    match slot {
        PlayerTeamSlot::Ally => [1.55, 0.2, 1.35],
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => [0.1, 1.15, 1.85],
    }
}

fn outline_default(slot: PlayerTeamSlot) -> [f64; 3] {
    match slot {
        PlayerTeamSlot::Ally => [1.85, 0.28, 1.65],
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => [0.16, 1.35, 2.2],
    }
}

/// `TEAM_COLOR_CVARS`.
#[must_use]
pub const fn team_color_cvar(slot: PlayerTeamSlot) -> &'static str {
    match slot {
        PlayerTeamSlot::Ally => "cl_team_ally_color",
        PlayerTeamSlot::Enemy => "cl_team_enemy_color",
        PlayerTeamSlot::Ffa => "cl_team_ffa_color",
    }
}

/// `TEAM_OUTLINE_CVARS`.
#[must_use]
pub const fn team_outline_cvar(slot: PlayerTeamSlot) -> &'static str {
    match slot {
        PlayerTeamSlot::Ally => "cl_team_ally_outline_color",
        PlayerTeamSlot::Enemy => "cl_team_enemy_outline_color",
        PlayerTeamSlot::Ffa => "cl_team_ffa_outline_color",
    }
}

const SLOTS: [(PlayerTeamSlot, &str); 3] = [
    (PlayerTeamSlot::Ally, "Teammate"),
    (PlayerTeamSlot::Enemy, "Enemy"),
    (PlayerTeamSlot::Ffa, "FFA enemy player model"),
];

/// `registerPlayerModelVisualCvars`.
pub fn register_player_model_visual_cvars(cvars: &mut Cvars) {
    for (slot, who) in SLOTS {
        cvars.register(
            team_color_cvar(slot),
            rgb_to_cvar(visual_default(slot)),
            CvarOptions::describe(&format!("{who} tint color as RGB floats.")),
        );
    }
    for (slot, who) in SLOTS {
        cvars.register(
            team_outline_cvar(slot),
            rgb_to_cvar(outline_default(slot)),
            CvarOptions::describe(&format!("{who} outline color as RGB floats.")),
        );
    }
    let int = |d: &str, max: f64| CvarOptions::describe(d).range(0.0, max).kind(CvarType::Int);
    cvars.register(
        "cl_player_outline",
        1,
        int("Enable third-person player model outlines.", 1.0),
    );
    let mut proxy = int(
        "Experimental reduced player outline geometry (requires benchmark:outline-proxy:build).",
        1.0,
    );
    proxy.flags.persist = false;
    cvars.register("cl_player_outline_proxy", 0, proxy);
    cvars.register(
        "cl_player_outline_size",
        25,
        int("Third-person player model outline size, 0-100.", 100.0),
    );
    cvars.register(
        "cl_player_fullbright",
        0,
        int("Enable a gentle emissive boost for third-person player models.", 1.0),
    );
    cvars.register(
        "cl_player_fullbright_intensity",
        0.25,
        CvarOptions::describe("Third-person player model fullbright boost intensity, 0-1.")
            .range(0.0, 1.0)
            .kind(CvarType::Float),
    );
}

/// `parseRgbCvar`: three numbers, else `fallback`.
#[must_use]
pub fn parse_rgb_cvar(value: &str, fallback: Vec3) -> Vec3 {
    let parts: Vec<f64> = value
        .split_whitespace()
        .map(|p| p.parse::<f64>().unwrap_or(f64::NAN))
        .collect();
    if parts.len() < 3 || parts.iter().any(|p| !p.is_finite()) {
        return fallback;
    }
    Vec3::new(parts[0] as f32, parts[1] as f32, parts[2] as f32)
}

/// `getPlayerModelVisualStyle`.
#[must_use]
pub fn get_player_model_visual_style(slot: PlayerTeamSlot, model_scale: f32) -> PlayerModelVisualStyle {
    with_cvars(|c| {
        let text = |name: &str| {
            if c.exists(name) {
                c.get_string(name)
            } else {
                String::new()
            }
        };
        let number = |name: &str, d: f64| if c.exists(name) { c.get_number(name) } else { d };
        let outline_size = number("cl_player_outline_size", 25.0).max(0.0) as f32;
        PlayerModelVisualStyle {
            tint_color: Some(parse_rgb_cvar(
                &text(team_color_cvar(slot)),
                player_model_visual_default(slot),
            )),
            fullbright_enabled: number("cl_player_fullbright", 0.0) != 0.0,
            fullbright_intensity: number("cl_player_fullbright_intensity", 0.25).clamp(0.0, 1.0) as f32,
            outline_enabled: number("cl_player_outline", 1.0) != 0.0 && outline_size > 0.0,
            outline_color: parse_rgb_cvar(&text(team_outline_cvar(slot)), player_model_outline_default(slot)),
            outline_width: outline_size / (model_scale * 16.0).max(1.0),
        }
    })
}

/// `getPlayerTeamSlot`: a player's colour side seen from `local_team`.
#[must_use]
pub fn get_player_team_slot(team_id: Option<&str>, local_team_id: Option<&str>) -> PlayerTeamSlot {
    let them = team_id.and_then(canonical_team);
    let me = local_team_id.and_then(canonical_team);
    match (me, them) {
        (Some(me), Some(them)) if me != "spectator" && them != "spectator" => {
            if them == me {
                PlayerTeamSlot::Ally
            } else {
                PlayerTeamSlot::Enemy
            }
        }
        _ => PlayerTeamSlot::Ffa,
    }
}

/// `canonicalTeam`: the lobby's and the BSP's team names folded onto the
/// match's two.
fn canonical_team(team_id: &str) -> Option<&'static str> {
    match team_id.trim().to_lowercase().as_str() {
        "udon" | "bravo" => Some("udon"),
        "soba" | "alpha" | "beta" => Some("soba"),
        "spectator" => Some("spectator"),
        _ => None,
    }
}

/// `sameColor`.
fn same_color(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01 && (a.z - b.z).abs() < 0.01
}

/// `FfaColors`: in a free-for-all each opponent wears a colour of their own
/// from the team palette, so they can be told apart. A colour is kept for
/// the session, the least-worn ones are handed out first, and your own
/// side's colour is never used, so it still means you (the player whose
/// eyes a demo borrows).
pub struct FfaColors {
    assigned: HashMap<String, &'static TeamColor>,
    random: Box<dyn FnMut() -> f64>,
}

impl Default for FfaColors {
    fn default() -> Self {
        // `Math.random`: any seed will do, as long as each session differs.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        Self::with_random(Box::new(mulberry32(seed)))
    }
}

impl FfaColors {
    /// Colours dealt by `random` (`Math.random`'s stand-in, in `[0, 1)`).
    #[must_use]
    pub fn with_random(random: Box<dyn FnMut() -> f64>) -> Self {
        Self {
            assigned: HashMap::new(),
            random,
        }
    }

    /// `colorFor`.
    pub fn color_for(&mut self, entity_id: &str) -> &'static TeamColor {
        if let Some(held) = self.assigned.get(entity_id) {
            return held;
        }
        let ally = player_model_visual_default(PlayerTeamSlot::Ally);
        let own = with_cvars(|c| {
            let name = team_color_cvar(PlayerTeamSlot::Ally);
            parse_rgb_cvar(
                &if c.exists(name) {
                    c.get_string(name)
                } else {
                    String::new()
                },
                ally,
            )
        });
        let choices: Vec<&'static TeamColor> = TEAM_COLORS
            .iter()
            .filter(|color| !same_color(parse_rgb_cvar(color.fill, own), own))
            .collect();
        let worn = |color: &TeamColor| self.assigned.values().filter(|other| other.id == color.id).count();
        let fewest = choices.iter().map(|c| worn(c)).min().unwrap_or(0);
        let pool: Vec<&'static TeamColor> = choices.into_iter().filter(|c| worn(c) == fewest).collect();
        let pick = pool[((self.random)() * pool.len() as f64).floor() as usize];
        self.assigned.insert(entity_id.to_owned(), pick);
        pick
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.assigned.clear();
    }
}

/// `getFfaPlayerModelVisualStyle`: the FFA style with this opponent's own
/// colour in place of the shared one.
#[must_use]
pub fn get_ffa_player_model_visual_style(color: &TeamColor, model_scale: f32) -> PlayerModelVisualStyle {
    let base = get_player_model_visual_style(PlayerTeamSlot::Ffa, model_scale);
    PlayerModelVisualStyle {
        tint_color: Some(parse_rgb_cvar(
            color.fill,
            player_model_visual_default(PlayerTeamSlot::Ffa),
        )),
        outline_color: parse_rgb_cvar(color.outline, base.outline_color),
        ..base
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `vi.spyOn(Math, "random").mockReturnValue(0)`.
    fn first() -> FfaColors {
        FfaColors::with_random(Box::new(|| 0.0))
    }

    #[test]
    fn each_opponent_a_different_colour_until_the_palette_runs_out_never_your_own() {
        with_cvars(register_player_model_visual_cvars);
        let mut colors = FfaColors::default();
        let opponents = TEAM_COLORS.len() - 1;
        let worn: Vec<&str> = (0..opponents).map(|i| colors.color_for(&format!("p{i}")).id).collect();
        let distinct: std::collections::HashSet<_> = worn.iter().collect();
        assert_eq!(distinct.len(), opponents);
        assert!(!worn.contains(&"pink"));
        // The next one has to share, and takes a colour worn only once.
        assert!(worn.contains(&colors.color_for("one-more").id));
    }

    #[test]
    fn keeps_a_colour_for_the_session_and_deals_afresh_after_a_clear() {
        let mut colors = first();
        let a = colors.color_for("a").id;
        assert_ne!(colors.color_for("b").id, a);
        assert_eq!(colors.color_for("a").id, a);
        colors.clear();
        assert_eq!(colors.color_for("b").id, a);
    }

    #[test]
    fn follows_your_own_sides_colour_when_you_change_it() {
        with_cvars(|c| {
            register_player_model_visual_cvars(c);
            c.set("cl_team_ally_color", "0.55 1.7 0.2");
        });
        let mut colors = FfaColors::default();
        let worn: Vec<&str> = (0..TEAM_COLORS.len() - 1)
            .map(|i| colors.color_for(&format!("p{i}")).id)
            .collect();
        assert!(!worn.contains(&"lime"));
        assert!(worn.contains(&"pink"));
    }

    #[test]
    fn an_ffa_style_wears_the_colour_and_its_outline() {
        let lime = TEAM_COLORS.iter().find(|c| c.id == "lime").unwrap();
        let style = get_ffa_player_model_visual_style(lime, 36.0);
        assert_eq!(style.tint_color, Some(Vec3::new(0.55, 1.7, 0.2)));
        assert_eq!(style.outline_color, Vec3::new(0.7, 2.0, 0.3));
    }
}
