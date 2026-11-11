//! wash-fps `client/PlayerModelVisualSettings.ts`: team tints and outlines
//! for third-person player models, from the `cl_team_*` and `cl_player_*`
//! cvars, and `FfaColors`, each free-for-all opponent in a colour of their
//! own. Your side is pink and everyone you may shoot white.

use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::core::cvar::{CvarOptions, CvarType, Cvars, with_cvars};
use shagger::rendering::glam::Vec3;
use std::collections::HashMap;

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
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => Vec3::new(1.6, 1.6, 1.6),
    }
}

/// `PLAYER_MODEL_OUTLINE_DEFAULTS`.
#[must_use]
pub fn player_model_outline_default(slot: PlayerTeamSlot) -> Vec3 {
    match slot {
        PlayerTeamSlot::Ally => Vec3::new(1.85, 0.28, 1.65),
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => Vec3::new(1.9, 1.9, 1.9),
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
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => [1.6, 1.6, 1.6],
    }
}

fn outline_default(slot: PlayerTeamSlot) -> [f64; 3] {
    match slot {
        PlayerTeamSlot::Ally => [1.85, 0.28, 1.65],
        PlayerTeamSlot::Enemy | PlayerTeamSlot::Ffa => [1.9, 1.9, 1.9],
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
        "cl_player_tint",
        0,
        int(
            "Apply the selected team tint to player textures; outlines keep their own colour.",
            1.0,
        ),
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
            tint_color: (number("cl_player_tint", 0.0) != 0.0)
                .then(|| parse_rgb_cvar(&text(team_color_cvar(slot)), player_model_visual_default(slot))),
            fullbright_enabled: number("cl_player_fullbright", 0.0) != 0.0,
            fullbright_intensity: number("cl_player_fullbright_intensity", 0.25).clamp(0.0, 1.0) as f32,
            outline_enabled: number("cl_player_outline", 1.0) != 0.0 && outline_size > 0.0,
            outline_color: parse_rgb_cvar(&text(team_outline_cvar(slot)), player_model_outline_default(slot)),
            outline_width: outline_size / (model_scale * 16.0).max(1.0),
        }
    })
}

/// `getFfaPlayerModelVisualStyle`: the FFA style with this opponent's own
/// colour in place of the shared one.
#[must_use]
pub fn get_ffa_player_model_visual_style(color: &TeamColor, model_scale: f32) -> PlayerModelVisualStyle {
    let base = get_player_model_visual_style(PlayerTeamSlot::Ffa, model_scale);
    PlayerModelVisualStyle {
        tint_color: base
            .tint_color
            .map(|_| parse_rgb_cvar(color.fill, player_model_visual_default(PlayerTeamSlot::Ffa))),
        outline_color: parse_rgb_cvar(color.outline, base.outline_color),
        ..base
    }
}

/// `sameColor`.
fn same_color(a: Vec3, b: Vec3) -> bool {
    (a - b).abs().max_element() < 0.01
}

/// `FfaColors`: in a free-for-all each opponent wears a colour of their own
/// from the team palette, so they can be told apart. The server hands the
/// colours out, so every viewer sees a player the same way. Your own side's
/// colour is never worn by an opponent, so it still means you (the player
/// whose eyes a demo borrows): an opponent given it, or given none by an
/// older server or demo, wears the least-worn other colour instead, kept for
/// the session.
#[derive(Debug, Default)]
pub struct FfaColors {
    assigned: HashMap<String, &'static TeamColor>,
    own_setting: Option<String>,
    own_color: Option<&'static TeamColor>,
    seed: u32,
}

impl FfaColors {
    /// `colorFor`: `given` is the colour id the server gave this player,
    /// `handed_out` the ones it gave everyone, counted when a pick has to
    /// step around them.
    pub fn color_for(
        &mut self,
        entity_id: &str,
        given: Option<&str>,
        handed_out: impl IntoIterator<Item = &'static str>,
    ) -> &'static TeamColor {
        let setting = with_cvars(|c| {
            if c.exists(team_color_cvar(PlayerTeamSlot::Ally)) {
                c.get_string(team_color_cvar(PlayerTeamSlot::Ally))
            } else {
                String::new()
            }
        });
        self.color_for_own(&setting, entity_id, given, handed_out)
    }

    /// [`Self::color_for`] with your side's colour setting given.
    pub fn color_for_own(
        &mut self,
        own_setting: &str,
        entity_id: &str,
        given: Option<&str>,
        handed_out: impl IntoIterator<Item = &'static str>,
    ) -> &'static TeamColor {
        let own = self.own(own_setting);
        let server = given.and_then(|g| TEAM_COLORS.iter().find(|c| c.id == g));
        if let Some(server) = server
            && own.is_none_or(|o| o.id != server.id)
        {
            return server;
        }
        if let Some(held) = self.assigned.get(entity_id) {
            return held;
        }
        let choices: Vec<&'static TeamColor> = TEAM_COLORS
            .iter()
            .filter(|c| own.is_none_or(|o| o.id != c.id))
            .collect();
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for id in handed_out {
            if TEAM_COLORS.iter().any(|c| c.id == id) {
                *counts.entry(id).or_default() += 1;
            }
        }
        for color in self.assigned.values() {
            *counts.entry(color.id).or_default() += 1;
        }
        let worn = |c: &TeamColor| counts.get(c.id).copied().unwrap_or(0);
        let fewest = choices.iter().map(|c| worn(c)).min().unwrap_or(0);
        let pool: Vec<&'static TeamColor> = choices.into_iter().filter(|c| worn(c) == fewest).collect();
        // `Math.random()`: any of the least worn.
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let pick = pool[(self.seed >> 16) as usize % pool.len()];
        self.assigned.insert(entity_id.to_owned(), pick);
        pick
    }

    /// `clear`.
    pub fn clear(&mut self) {
        self.assigned.clear();
    }

    /// The palette colour your own side wears, if it is one of them; read
    /// again only when the setting changes.
    fn own(&mut self, setting: &str) -> Option<&'static TeamColor> {
        if self.own_setting.as_deref() != Some(setting) {
            let own = parse_rgb_cvar(setting, player_model_visual_default(PlayerTeamSlot::Ally));
            self.own_color = TEAM_COLORS
                .iter()
                .find(|c| same_color(parse_rgb_cvar(c.fill, own), own));
            self.own_setting = Some(setting.to_owned());
        }
        self.own_color
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const PINK: &str = "1.55 0.2 1.35";

    #[test]
    fn each_ffa_opponent_wears_a_colour_of_their_own_never_yours() {
        // FfaColors.test.ts: pink is your side's by default.
        let mut colors = FfaColors::default();
        let opponents = TEAM_COLORS.len() - 1;
        let worn: Vec<&str> = (0..opponents)
            .map(|i| colors.color_for_own(PINK, &format!("p{i}"), None, []).id)
            .collect();
        let distinct: std::collections::HashSet<_> = worn.iter().collect();
        assert_eq!(distinct.len(), opponents);
        assert!(!worn.contains(&"pink"));
        let more = colors.color_for_own(PINK, "one-more", None, []).id;
        assert!(worn.contains(&more));
        // Kept for the session.
        assert_eq!(colors.color_for_own(PINK, "p0", None, []).id, worn[0]);
    }

    #[test]
    fn the_server_given_colour_is_worn_unless_it_is_yours() {
        let mut colors = FfaColors::default();
        assert_eq!(colors.color_for_own(PINK, "a", Some("red"), []).id, "red");
        // Pink means you: the opponent given it steps around every colour handed out.
        let handed_out: Vec<&'static str> = TEAM_COLORS.iter().map(|c| c.id).filter(|id| *id != "violet").collect();
        assert_eq!(colors.color_for_own(PINK, "b", Some("pink"), handed_out).id, "violet");
    }

    #[test]
    fn enemies_start_white_and_untinted() {
        assert_eq!(player_model_visual_default(PlayerTeamSlot::Enemy), Vec3::splat(1.6));
        assert_eq!(player_model_outline_default(PlayerTeamSlot::Ffa), Vec3::splat(1.9));
        let mut cvars = Cvars::default();
        register_player_model_visual_cvars(&mut cvars);
        assert_eq!(cvars.get_number("cl_player_tint"), 0.0);
    }
}
