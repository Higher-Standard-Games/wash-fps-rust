//! wash-fps `client/settings/SettingsModel.ts`, the pages and their cvars.
//! Every choice writes the game's own cvars; nothing here is a setting of
//! its own. The key bindings half comes with the input system's bindings.
//!
//! [`register_client_cvars`] stands in for the browser's scattered
//! `Cvar.register` calls at module load, naming each owner.

use shagger::audio::audio_cvars::register_audio_cvars;
use shagger::core::cvar::{CvarOptions, CvarType, Cvars};
use shagger::movement::movement_vars::register_movement_vars;

use crate::client::crosshair::register_crosshair_cvars;

/// `SettingsPageId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsPageId {
    Controls,
    Gameplay,
    Video,
    Audio,
}

/// `SETTINGS_PAGES`: id, title, hint.
pub const SETTINGS_PAGES: [(SettingsPageId, &str, &str); 4] = [
    (SettingsPageId::Controls, "Controls", "Keys · mouse"),
    (SettingsPageId::Gameplay, "Gameplay", "Aim · colours"),
    (SettingsPageId::Video, "Video", "Quality"),
    (SettingsPageId::Audio, "Audio", "Volume"),
];

/// `PAGE_CVARS`: the cvars each page's reset puts back.
#[must_use]
pub fn page_cvars(page: SettingsPageId) -> &'static [&'static str] {
    match page {
        SettingsPageId::Controls => &["m_sensitivity", "cl_ads_sensitivity", "cl_autorun"],
        SettingsPageId::Gameplay => &[
            "cl_team_ally_sounds",
            "cl_team_enemy_sounds",
            "cl_team_ally_color",
            "cl_team_ally_outline_color",
            "cl_team_enemy_color",
            "cl_team_enemy_outline_color",
            "cl_team_ffa_color",
            "cl_team_ffa_outline_color",
            "cl_player_outline",
            "cl_player_outline_size",
            "cl_player_fullbright",
            "cl_player_fullbright_intensity",
            "cg_crosshair",
            "cg_crosshairSize",
            "cg_crosshairAlpha",
            "cg_crosshairColor",
            "cg_crosshairSpread",
            "cg_crosshairDmgColor",
            "cl_fov",
            "show_fps",
            "cl_showspeed",
            "cl_hints",
            "cl_invertClock",
            "cl_autoswitch",
            "cl_netgraph",
        ],
        SettingsPageId::Video => &[
            "r_picmip",
            "r_bloom",
            "r_glow",
            "r_ssr",
            "r_ssr_strength",
            "r_anisotropy",
            "r_godrays",
            "r_godrays_samples",
            "r_softhalos",
            "r_water_quality",
            "r_water_actors",
            "r_arealights",
            "r_bsp_pbr",
            "r_cel_outlines",
            "r_exposure",
            "r_contrast",
            "r_underwater_distortion",
        ],
        SettingsPageId::Audio => &[
            "s_musicenabled",
            "s_volume",
            "s_sfxenabled",
            "s_sfxvolume",
            "cg_volume_effects",
            "cg_volume_players",
            "cg_volume_hitsound",
            "cg_volume_announcer",
        ],
    }
}

/// `resetPage`.
pub fn reset_page(cvars: &mut Cvars, page: SettingsPageId) {
    for name in page_cvars(page) {
        if cvars.exists(name) {
            cvars.reset(name);
        }
    }
}

fn int_flag(cvars: &mut Cvars, name: &str, default: i32, description: &str) {
    cvars.register(
        name,
        default,
        CvarOptions::describe(description).range(0.0, 1.0).kind(CvarType::Int),
    );
}

/// The cvars this client reads, as their owners register them.
pub fn register_client_cvars(cvars: &mut Cvars) {
    // shagger: MovementVars.ts, AudioManager.ts.
    register_movement_vars(cvars);
    register_audio_cvars(cvars);
    // Crosshair.ts.
    register_crosshair_cvars(cvars);
    // Game.ts.
    int_flag(
        cvars,
        "cl_autoswitch",
        1,
        "Switch to a weapon when you pick it up. 0 keeps what you hold, blade included",
    );
    // hud/widgets/HintPanel.ts.
    int_flag(
        cvars,
        "cl_hints",
        1,
        "Show hints for new players (movement, weapons, items), above your weapons",
    );
    // MatchStateManager.ts.
    int_flag(
        cvars,
        "cl_invertClock",
        0,
        "Match clock counts up from 0:00 (time played) instead of down (time left)",
    );
    // SHAG core/FpsDisplaySettings.ts.
    int_flag(cvars, "show_fps", 1, "Show the FPS counter");
    // SHAG network/client/NetGraph.ts.
    cvars.register(
        "cl_netgraph",
        0,
        CvarOptions::describe("Network graph overlay (0=off, 1=text, 2=text+graph)")
            .range(0.0, 2.0)
            .kind(CvarType::Int),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_reset_puts_its_cvars_back() {
        let mut cvars = Cvars::new(None, false);
        register_client_cvars(&mut cvars);
        cvars.set("cl_fov", 105);
        cvars.set("s_volume", 0.1);
        reset_page(&mut cvars, SettingsPageId::Gameplay);
        assert_eq!(cvars.get_number("cl_fov"), 90.0);
        assert_eq!(cvars.get_number("s_volume"), 0.1);
    }
}
