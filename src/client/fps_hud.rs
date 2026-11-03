//! The HUD's frame (wash-fps `client/FPSHud.ts`): the widgets in their
//! areas. The top-centre row holds the frag wing, the clock and the
//! gametype tab (flex, `order` 1 to 3, the frag wing pulled 18 px under the
//! clock and the tab 16 px); the phase message hangs below it; the
//! scoreboard covers everything when open. The bottom bar holds the
//! vitals at the left and the ammo at the right, for the player on screen;
//! the crosshair sits over it all.

use shagger::rendering::gpu::Gpu;
use shagger::rendering::hud_2d_renderer::Hud2dRenderer;
use shagger::rendering::wgpu;
use shagger::ui::hud::damage_overlay::DamageOverlay;
use shagger::ui::hud_2d::Hud2d;

use crate::client::ammo_status::get_ammo_status;
use crate::client::crosshair::{Crosshair, CrosshairFrame};
use crate::client::hud::frame_rate_display::FrameRateDisplay;
use crate::client::hud::hud_widget::{FpsHudWidget, HudFrame};
use crate::client::hud::washiki::{Fonts, UI_SCALE};
use crate::client::hud::widgets::ammo_counter::AmmoCounter;
use crate::client::hud::widgets::ammo_warning::AmmoWarning;
use crate::client::hud::widgets::chat::Chat;
use crate::client::hud::widgets::death_screen::DeathScreen;
use crate::client::hud::widgets::event_feed::EventFeed;
use crate::client::hud::widgets::frag_counter::FragCounter;
use crate::client::hud::widgets::frag_message::FragMessage;
use crate::client::hud::widgets::gametype_badge::GametypeBadge;
use crate::client::hud::widgets::phase_message::PhaseMessage;
use crate::client::hud::widgets::pickup_toast::PickupToast;
use crate::client::hud::widgets::powerup_timer::PowerupTimer;
use crate::client::hud::widgets::scoreboard::Scoreboard;
use crate::client::hud::widgets::speedometer::Speedometer;
use crate::client::hud::widgets::timer::Timer;
use crate::client::hud::widgets::vitals_block::VitalsBlock;
use crate::client::hud::widgets::weapon_slots::WeaponSlots;
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
    vitals: VitalsBlock,
    ammo: AmmoCounter,
    crosshair: Crosshair,
    weapon_slots: WeaponSlots,
    speedometer: Speedometer,
    /// `show_fps`, app-wide in the browser; here over the match and demos.
    pub frame_rate: FrameRateDisplay,
    ammo_warning: AmmoWarning,
    pickup: PickupToast,
    powerup_timer: PowerupTimer,
    death: DeathScreen,
    damage: DamageOverlay,
    pub chat: Chat,
    /// The reticle this frame, when someone's eyes are on screen.
    reticle: Option<(String, f64, bool)>,
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
    /// The view entity's horizontal speed, units a second.
    pub speed: f64,
    /// Where the players are (entity id, position), for which way damage
    /// came from.
    pub positions: &'a [(String, [f64; 3])],
    /// The camera's yaw, radians.
    pub yaw: f64,
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
            vitals: VitalsBlock::default(),
            ammo: AmmoCounter::default(),
            crosshair: Crosshair::default(),
            weapon_slots: WeaponSlots::default(),
            speedometer: Speedometer::default(),
            frame_rate: FrameRateDisplay::default(),
            ammo_warning: AmmoWarning::default(),
            pickup: PickupToast::default(),
            powerup_timer: PowerupTimer::default(),
            death: DeathScreen::default(),
            damage: DamageOverlay::default(),
            chat: Chat::default(),
            reticle: None,
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
        let view = frame.view_entity.unwrap_or_default();
        // `keepOnlyLocalPowerups` runs per snapshot; the subject's
        // `combat_state`, sent in every one, says a snapshot arrived.
        let mut snapshot_arrived = false;
        let mut running_powerups = std::collections::BTreeSet::new();
        for event in frame.events {
            match event {
                // The HUD describes whoever the camera is behind.
                SnapshotEvent::CombatState {
                    entity_id,
                    health,
                    armor,
                    dead,
                } if entity_id == view => {
                    snapshot_arrived = true;
                    state.process_combat_state(*health, *armor, *dead);
                    if frame.playing {
                        state.local_dead = *dead;
                    }
                }
                SnapshotEvent::WeaponState {
                    entity_id,
                    current_weapon,
                    weapons,
                    ammo,
                    ..
                } if entity_id == view => {
                    state.process_weapon_state(current_weapon, weapons, ammo);
                }
                SnapshotEvent::Chat {
                    sender_name,
                    team,
                    text,
                    ..
                } => self.chat.add_message(sender_name, text, team),
                SnapshotEvent::Damage {
                    entity_id,
                    attacker_id,
                    damage,
                    hit_location,
                    ..
                } if !view.is_empty() => {
                    if entity_id == view && *damage > 0 {
                        // Incoming: the overlay, from the attacker's side.
                        let at = |id: &str| frame.positions.iter().find(|(e, _)| e == id).map(|(_, p)| *p);
                        let angle = match (at(attacker_id), at(view)) {
                            (Some(a), Some(v)) if attacker_id != view => (a[2] - v[2]).atan2(a[0] - v[0]) - frame.yaw,
                            // `Math.random() * Math.PI * 2`: any side.
                            _ => (state.now * 7.31).fract() * std::f64::consts::TAU,
                        };
                        self.damage.show(f64::from(*damage), angle as f32);
                    }
                    if attacker_id == view && entity_id != view {
                        self.crosshair.notify_hit(hit_location.is_some_and(|l| l == "head"));
                    }
                }
                SnapshotEvent::PickupNotify { entity_id, label } if entity_id == view => {
                    self.pickup.show_pickup(label);
                }
                SnapshotEvent::PowerupState {
                    entity_id,
                    kind,
                    seconds,
                } if entity_id == view => {
                    state.powerups.set(kind, f64::from(*seconds), state.now);
                    if *seconds > 0 {
                        running_powerups.insert(kind.clone());
                    }
                }
                _ => {}
            }
            if let SnapshotEvent::Death {
                entity_id,
                killer_id,
                damage_type,
                ..
            } = event
            {
                // Powerups do not survive death.
                if entity_id == view {
                    state.powerups.clear();
                }
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
        if snapshot_arrived {
            state.powerups.keep_only(&running_powerups);
        }
        state.expire_transients(frame.dt);
        self.chat.tick(frame.dt);
        if frame.playing {
            let team = state
                .entry_for_entity(view)
                .and_then(|e| e.team_id.clone())
                .unwrap_or_default();
            self.chat.set_local_team(&team);
        }
        let (show_speed, show_fps) =
            shagger::core::cvar::with_cvars(|c| (c.get_bool("cl_showspeed"), c.get_bool("show_fps")));
        // `FPSController` runs only for the player's own eyes.
        self.speedometer.update(frame.playing && show_speed, frame.speed);
        self.frame_rate.tick(frame.dt, show_fps);
        self.reticle = (!view.is_empty() && !state.dead).then(|| {
            let weapon = state.current_weapon.clone();
            let out = get_ammo_status(&weapon, state.ammo.get(&weapon).copied()).out_of_ammo;
            (weapon, frame.speed, out)
        });
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
        let [vw, vh] = self.hud.size();
        // `FPSHud`'s post-game: the result owns the screen; the gameplay
        // HUD is hidden, the board and the chat stay.
        if self.scoreboard.post_game() {
            self.scoreboard.draw(&mut self.hud, &fonts, 0.0, 0.0);
            self.chat.draw(
                &mut self.hud,
                &fonts,
                20.0 * s,
                vh - 236.0 * s,
                (460.0 * s).min(vw / 2.0),
            );
            self.frame_rate.draw(&mut self.hud, &fonts, vw);
            return;
        }
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
        self.frag_message.draw(&mut self.hud, &fonts, vw / 2.0, vh * 0.31);
        // `.fps-hud-bottom-bar`: 16 px up, 28 px in; nobody's to show for
        // a spectator flying free.
        let subject = !frame.state.subjectless() && !frame.state.subject().is_empty();
        if subject {
            let bottom = vh - 16.0 * s;
            let [_, vitals_h] = self.vitals.size(&self.hud, &fonts);
            self.vitals.draw(&mut self.hud, &fonts, 28.0 * s, bottom - vitals_h);
            // `.fps-powerup-timer`, stacked above the vitals.
            let [_, timer_h] = self.powerup_timer.size(&self.hud, &fonts);
            self.powerup_timer
                .draw(&mut self.hud, &fonts, 28.0 * s, bottom - vitals_h - timer_h);
            let [ammo_w, ammo_h] = self.ammo.size(&self.hud, &fonts);
            let ammo_x = vw - 28.0 * s - ammo_w;
            self.ammo.draw(&mut self.hud, &fonts, ammo_x, bottom - ammo_h);
            // The slots before the ammo, which tucks 8 px under them.
            let [slots_w, slots_h] = self.weapon_slots.size(&self.hud, &fonts);
            self.weapon_slots
                .draw(&mut self.hud, &fonts, ammo_x + 8.0 * s - slots_w, bottom - slots_h);
            // `.fps-hud-bottom-center`: the speedometer, and the toasts
            // floating over it.
            let [speed_w, speed_h] = self.speedometer.size(&self.hud, &fonts);
            self.speedometer
                .draw(&mut self.hud, &fonts, (vw - speed_w) / 2.0, bottom - speed_h);
            self.pickup.draw(&mut self.hud, &fonts, vw / 2.0, bottom - speed_h);
        }
        self.scoreboard.draw(&mut self.hud, &fonts, 0.0, 0.0);
        // `.fps-chat`: clear of the bottom bar, over the scoreboard's veil.
        self.chat.draw(
            &mut self.hud,
            &fonts,
            20.0 * s,
            vh - 236.0 * s,
            (460.0 * s).min(vw / 2.0),
        );
        // `.fps-event-feed` sits over the scoreboard's veil.
        self.event_feed.draw(&mut self.hud, &fonts, 24.0 * s, 64.0 * s);
        // Then the death screen (z 800), the damage overlay (900), the
        // ammo warning (950) and the crosshair (1000).
        self.death.draw(&mut self.hud, &fonts, 0.0, 0.0);
        self.damage.draw(&mut self.hud, frame.dt, vw, vh);
        if subject {
            self.ammo_warning.draw(&mut self.hud, &fonts, vw / 2.0, vh * 0.83);
        }
        // `.fps-frame-rate` (z 900).
        self.frame_rate.draw(&mut self.hud, &fonts, vw);
        if let Some((weapon, speed, out_of_ammo)) = &self.reticle
            && !self.scoreboard.held
        {
            self.crosshair.draw(
                &mut self.hud,
                &CrosshairFrame {
                    weapon,
                    speed: *speed,
                    out_of_ammo: *out_of_ammo,
                    dt: frame.dt,
                },
                vw,
                vh,
            );
        }
    }

    fn widgets_mut(&mut self) -> [&mut dyn FpsHudWidget; 14] {
        [
            &mut self.powerup_timer,
            &mut self.weapon_slots,
            &mut self.ammo_warning,
            &mut self.pickup,
            &mut self.death,
            &mut self.vitals,
            &mut self.ammo,
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
