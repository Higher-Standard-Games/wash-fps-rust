//! The widget interface (wash-fps `client/hud/HudWidget.ts`'s
//! `FPSHudWidget`): `update` from the state each frame, then `draw`.
//! A widget in a flex area also says its size, so the area can lay it out.

use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::washiki::Fonts;
use crate::client::hud_state::HudState;
use crate::client::match_state_manager::MatchStateManager;

/// What a frame gives the widgets.
pub struct HudFrame<'a> {
    pub state: &'a HudState,
    pub match_state: &'a MatchStateManager,
    /// Seconds since the last frame.
    pub dt: f64,
}

pub trait FpsHudWidget {
    fn update(&mut self, frame: &HudFrame<'_>);

    /// The box in CSS pixels; zero when hidden.
    fn size(&self, hud: &Hud2d, fonts: &Fonts) -> [f32; 2];

    /// Draws with the box's top left at (`x`, `y`).
    fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, x: f32, y: f32);
}
