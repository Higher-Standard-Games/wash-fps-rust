//! wash-fps `client/PlayerModelVisualSettings.ts`: team tints and outlines
//! for third-person player models. The cvars are not ported yet, so the
//! style is the cvar defaults.

use shagger::animation::player_model_manager::PlayerModelVisualStyle;
use shagger::rendering::glam::Vec3;

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

/// `cl_player_outline` default.
pub const CL_PLAYER_OUTLINE: bool = true;
/// `cl_player_outline_size` default.
pub const CL_PLAYER_OUTLINE_SIZE: f32 = 25.0;
/// `cl_player_fullbright` default.
pub const CL_PLAYER_FULLBRIGHT: bool = false;
/// `cl_player_fullbright_intensity` default.
pub const CL_PLAYER_FULLBRIGHT_INTENSITY: f32 = 0.25;

/// `getPlayerModelVisualStyle`, with every cvar at its default.
#[must_use]
pub fn get_player_model_visual_style(slot: PlayerTeamSlot, model_scale: f32) -> PlayerModelVisualStyle {
    let outline_size = CL_PLAYER_OUTLINE_SIZE.max(0.0);
    PlayerModelVisualStyle {
        tint_color: Some(player_model_visual_default(slot)),
        fullbright_enabled: CL_PLAYER_FULLBRIGHT,
        fullbright_intensity: CL_PLAYER_FULLBRIGHT_INTENSITY.clamp(0.0, 1.0),
        outline_enabled: CL_PLAYER_OUTLINE && outline_size > 0.0,
        outline_color: player_model_outline_default(slot),
        outline_width: outline_size / (model_scale * 16.0).max(1.0),
    }
}
