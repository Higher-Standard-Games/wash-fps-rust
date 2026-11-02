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
use crate::client::hud::widgets::event_feed::EventFeed;
use crate::client::hud::widgets::frag_counter::FragCounter;
use crate::client::hud::widgets::frag_message::FragMessage;
use crate::client::hud::widgets::gametype_badge::GametypeBadge;
use crate::client::hud::widgets::phase_message::PhaseMessage;
use crate::client::hud::widgets::scoreboard::Scoreboard;
use crate::client::hud::widgets::timer::Timer;
use crate::client::hud_state::{DeathParties, HudState};
use crate::client::match_state_manager::MatchStateManager;
use crate::shared::net::snapshot_events::SnapshotEvent;
use shagger::shared::net::game_state::PlayerState;

/// `FPSHud`.
pub struct FpsHud {
    pub hud: Hud2d,
    fonts: Fonts,
    renderer: Hud2dRenderer,
    timer: Timer,
    frags: FragCounter,
    gametype: GametypeBadge,
    phase: PhaseMessage,
    frag_message: FragMessage,
    event_feed: EventFeed,
    pub scoreboard: Scoreboard,
    /// What the widgets read, kept across frames (the kill feed).
    pub state: HudState,
}

/// A frame of a demo or a match being watched.
pub struct WatchFrame<'a> {
    /// The newest snapshot's players.
    pub roster: &'a [(String, PlayerState)],
    pub match_state: &'a MatchStateManager,
    /// The events that arrived since the last frame.
    pub events: &'a [SnapshotEvent],
    /// The entity the camera follows, if any.
    pub view_entity: Option<&'a str>,
    /// A recording rather than a live match.
    pub playback: bool,
    /// We are in the match, and `view_entity` is our own.
    pub playing: bool,
    pub dt: f64,
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
            frag_message: FragMessage::default(),
            event_feed: EventFeed::default(),
            scoreboard: Scoreboard::default(),
            state: HudState::default(),
        })
    }

    /// A spectator's frame (`GameEvents.ts`'s state and death handling,
    /// then [`Self::update`]).
    pub fn update_watching(&mut self, frame: &WatchFrame<'_>, width: f32, height: f32) {
        let state = &mut self.state;
        state.watcher = !frame.playing;
        state.playback = frame.playback;
        if frame.playing {
            state.spectating.clear();
            state.local_player_id = frame.view_entity.unwrap_or_default().to_owned();
        } else {
            state.spectating = frame.view_entity.unwrap_or_default().to_owned();
        }
        state.update_from_game_state(frame.roster, frame.match_state);
        for event in frame.events {
            if let SnapshotEvent::Death {
                entity_id,
                killer_id,
                damage_type,
                ..
            } = event
            {
                let victim_name = HudState::resolve_player_name(frame.roster, entity_id);
                let killer_name = if killer_id.is_empty() {
                    String::new()
                } else {
                    HudState::resolve_player_name(frame.roster, killer_id)
                };
                state.process_death(
                    entity_id,
                    killer_id,
                    damage_type,
                    &DeathParties {
                        killer_name: &killer_name,
                        victim_name: &victim_name,
                        local_entity_id: frame.playing.then_some(frame.view_entity).flatten(),
                        view_entity_id: frame.view_entity,
                    },
                );
            }
        }
        state.expire_transients(frame.dt);
        let state = std::mem::take(&mut self.state);
        self.update(
            &HudFrame {
                state: &state,
                match_state: frame.match_state,
                dt: frame.dt,
            },
            width,
            height,
        );
        self.state = state;
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
        // `.fps-frag-container`: 31 % down, centred.
        let [_, vh] = self.hud.size();
        self.frag_message.draw(&mut self.hud, &fonts, vw / 2.0, vh * 0.31);
        self.scoreboard.draw(&mut self.hud, &fonts, 0.0, 0.0);
        // `.fps-event-feed` sits over the scoreboard's veil.
        self.event_feed.draw(&mut self.hud, &fonts, 24.0 * s, 64.0 * s);
    }

    fn widgets_mut(&mut self) -> [&mut dyn FpsHudWidget; 7] {
        [
            &mut self.frag_message,
            &mut self.event_feed,
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
