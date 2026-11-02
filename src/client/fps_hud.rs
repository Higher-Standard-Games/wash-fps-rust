//! The HUD's frame (wash-fps `client/FPSHud.ts`): the widgets in their
//! areas. The top-centre row holds the frag wing, the clock and the
//! gametype tab (flex, `order` 1 to 3, the frag wing pulled 18 px under the
//! clock and the tab 16 px); the phase message hangs below it; the
//! scoreboard covers everything when open. Vitals, ammo and the rest of
//! the bottom bar need a player in play.

use shagger::rendering::gpu::Gpu;
use shagger::rendering::hud_2d_renderer::Hud2dRenderer;
use shagger::rendering::wgpu;
use shagger::ui::hud_2d::Hud2d;

use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, UI_SCALE};
use crate::client::hud::widgets::frag_counter::FragCounter;
use crate::client::hud::widgets::gametype_badge::GametypeBadge;
use crate::client::hud::widgets::phase_message::PhaseMessage;
use crate::client::hud::widgets::scoreboard::Scoreboard;
use crate::client::hud::widgets::timer::Timer;

/// `FPSHud`.
pub struct FpsHud {
    pub hud: Hud2d,
    fonts: Fonts,
    renderer: Hud2dRenderer,
    timer: Timer,
    frags: FragCounter,
    gametype: GametypeBadge,
    phase: PhaseMessage,
    pub scoreboard: Scoreboard,
}

impl FpsHud {
    pub fn new(gpu: &Gpu, color_format: wgpu::TextureFormat) -> Result<Self, String> {
        let mut hud = Hud2d::new();
        let fonts = Fonts::load(&mut hud)?;
        let renderer = Hud2dRenderer::new(gpu, color_format, hud.atlas.size);
        Ok(Self {
            hud,
            fonts,
            renderer,
            timer: Timer::default(),
            frags: FragCounter::default(),
            gametype: GametypeBadge::default(),
            phase: PhaseMessage::default(),
            scoreboard: Scoreboard::default(),
        })
    }

    /// Updates every widget and lays the frame out on a `width` by
    /// `height` target.
    pub fn update(&mut self, frame: &HudFrame<'_>, width: f32, height: f32) {
        for widget in self.widgets_mut() {
            widget.update(frame);
        }
        self.hud.begin(width, height, 1.0);
        let fonts = self.fonts;
        let s = UI_SCALE;
        let [vw, _] = self.hud.size();
        // The top-centre row.
        let frags = self.frags.size(&self.hud, &fonts);
        let timer = self.timer.size(&self.hud, &fonts);
        let badge = self.gametype.size(&self.hud, &fonts);
        let frag_overlap = if frags[0] > 0.0 { 18.0 * s } else { 0.0 };
        let row = frags[0] - frag_overlap + timer[0] + badge[0] - 16.0 * s;
        let x = (vw - row) / 2.0;
        let top = 20.0 * s;
        self.frags.draw(&mut self.hud, &fonts, x, top);
        let timer_x = x + frags[0] - frag_overlap;
        self.gametype
            .draw(&mut self.hud, &fonts, timer_x + timer[0] - 16.0 * s, top);
        self.timer.draw(&mut self.hud, &fonts, timer_x, top);
        // `.fps-phase-message`: top 88 px, centred.
        self.phase.draw(&mut self.hud, &fonts, vw / 2.0, 88.0 * s);
        self.scoreboard.draw(&mut self.hud, &fonts, 0.0, 0.0);
    }

    fn widgets_mut(&mut self) -> [&mut dyn FpsHudWidget; 5] {
        [
            &mut self.timer,
            &mut self.frags,
            &mut self.gametype,
            &mut self.phase,
            &mut self.scoreboard,
        ]
    }

    pub fn prepare(&mut self, gpu: &Gpu) {
        self.renderer.prepare(gpu, &mut self.hud);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.renderer.draw(pass);
    }
}
