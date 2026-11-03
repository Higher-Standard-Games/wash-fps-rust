//! Port of wash-fps `client/TeleportEffect.ts`: Washoi's manga teleporter
//! effect. A ghost (a flat, unlit, team-coloured player figure fading out
//! in discrete alpha steps) from a pool of four, and a burst of speed lines
//! (two interleaved fans of spikes facing the camera, the inner one
//! spinning) that pops outward at an exit or collapses inward at an entry.
//!
//! The TypeScript builds a mesh and material per burst; here the bursts are
//! instances of one star template on shagger's effect quad batch. The
//! ghosts are the player model uploaded once more with the silhouette
//! material, each slot an instance of its own holding the idle clip.

use std::collections::HashMap;
use std::f64::consts::PI;

use shagger::animation::player_model_manager::{PlayerModel, PlayerModelInstance};
use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::babylon::materials::standard_material::StandardMaterialParams;
use shagger::rendering::effect_quad_batch::{EffectFrame, EffectInstance, EffectQuadBatch, EffectTemplate};
use shagger::rendering::glam::{DVec3, Mat4, Quat, Vec3};
use shagger::rendering::gpu::Gpu;
use shagger::rendering::model_renderer::{ModelDraw, ModelHandle, ModelMaterialMode, ModelRenderer};
use shagger::rendering::wgpu;

use super::remote_player_renderer::MODEL_SCALE;

/// Ghost lifetime (ms) and its discrete alpha steps.
pub const GHOST_LIFETIME_MS: f64 = 450.0;
pub const GHOST_ALPHA_STEPS: [f64; 4] = [0.85, 0.55, 0.3, 0.12];
pub const GHOST_POOL_SIZE: usize = 4;
/// The ghost's anchor below the player origin.
pub const GHOST_ANCHOR_Y_OFFSET: f64 = -12.0;
pub const GHOST_MODEL_SCALE: f32 = 36.0;
pub const BURST_LIFETIME_MS: f64 = 420.0;
pub const BURST_ALPHA_STEPS: [f64; 4] = [0.95, 0.7, 0.45, 0.2];
pub const BURST_START_SCALE: f64 = 0.5;
pub const BURST_END_SCALE: f64 = 1.6;
pub const IMPLOSION_START_SCALE: f64 = 1.9;
pub const IMPLOSION_END_SCALE: f64 = 0.2;
pub const TELE_IN_SOUND: &str = "sounds/world/tele_in.ogg";
pub const BURST_SPIN_RAD: f64 = 0.9;
pub const BURST_SPIKES: usize = 16;
pub const BURST_INNER_RADIUS: f32 = 20.0;
pub const BURST_OUTER_RADIUS: f32 = 84.0;
pub const BURST_BASE_HALF_WIDTH: f32 = 2.5;
pub const BURST_Y_OFFSET: f64 = 8.0;
pub const MAX_EFFECTS: usize = 12;
/// The update's clamp on a frame (`Math.min(100, …)`).
const MAX_STEP_MS: f64 = 100.0;

/// The udon and soba tints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TeamColor {
    Alpha,
    Bravo,
}

impl TeamColor {
    /// `TEAM_COLORS`.
    #[must_use]
    pub fn color(self) -> Vec3 {
        match self {
            Self::Alpha => Vec3::new(0.15, 0.95, 1.0),
            Self::Bravo => Vec3::new(1.0, 0.2, 0.75),
        }
    }
}

/// `BURST_COLOR`.
pub const BURST_COLOR: [f32; 3] = [0.85, 1.0, 1.0];

/// What an effect is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TeleportPiece {
    /// A ghost in pool slot `slot`, its player origin and yaw.
    Ghost {
        slot: usize,
        at: DVec3,
        yaw: f64,
        team: TeamColor,
    },
    /// A burst at its centre, scaling from one size to another.
    Burst { at: DVec3, from: f64, to: f64 },
}

/// `ActiveEffect`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeleportPieceState {
    pub piece: TeleportPiece,
    pub elapsed_ms: f64,
}

impl TeleportPieceState {
    fn lifetime(&self) -> f64 {
        match self.piece {
            TeleportPiece::Ghost { .. } => GHOST_LIFETIME_MS,
            TeleportPiece::Burst { .. } => BURST_LIFETIME_MS,
        }
    }

    /// Life spent, 0 to 1.
    #[must_use]
    pub fn t(&self) -> f64 {
        self.elapsed_ms / self.lifetime()
    }

    /// The posterized fade: the alpha step this far in.
    #[must_use]
    pub fn alpha(&self) -> f64 {
        let steps: &[f64] = match self.piece {
            TeleportPiece::Ghost { .. } => &GHOST_ALPHA_STEPS,
            TeleportPiece::Burst { .. } => &BURST_ALPHA_STEPS,
        };
        let step = ((self.t() * steps.len() as f64).floor() as usize).min(steps.len() - 1);
        steps[step]
    }

    /// A burst's size now, smoothly from start to end.
    #[must_use]
    pub fn scale(&self) -> f64 {
        match self.piece {
            TeleportPiece::Burst { from, to, .. } => from + (to - from) * self.t(),
            TeleportPiece::Ghost { .. } => 1.0,
        }
    }

    /// The inner fan's turn: interleaved with the outer, then spinning.
    #[must_use]
    pub fn spin(&self) -> f64 {
        PI / BURST_SPIKES as f64 + BURST_SPIN_RAD * self.t()
    }
}

/// A sound the effect asks for: `at` none is heard by ourselves (2D).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeleportSound {
    pub at: Option<DVec3>,
}

/// `TeleportEffect`.
#[derive(Debug, Clone, Default)]
pub struct TeleportEffect {
    effects: Vec<TeleportPieceState>,
    /// Which pool slots a ghost stands in (`GhostSlot.busy`).
    busy: [bool; GHOST_POOL_SIZE],
    /// Without a pool (no player model) only the bursts play.
    pub has_ghosts: bool,
    sounds: Vec<TeleportSound>,
}

impl TeleportEffect {
    #[must_use]
    pub fn new(has_ghosts: bool) -> Self {
        Self {
            has_ghosts,
            ..Self::default()
        }
    }

    /// `playerTeleported`: another player's teleport, with a ghost and a
    /// burst at both ends (the entry's collapsing inward) and the entry's
    /// sound where it happened.
    pub fn player_teleported(&mut self, from: DVec3, to: DVec3, from_yaw: f64, to_yaw: f64, team: TeamColor) {
        self.sounds.push(TeleportSound { at: Some(from) });
        self.spawn_ghost(from, from_yaw, team);
        self.spawn_burst(from, true);
        self.spawn_ghost(to, to_yaw, team);
        self.spawn_burst(to, false);
    }

    /// `localTeleported`: our own ghost and implosion at the entry, the
    /// sound in our ears, and the exit burst thrown 90 units ahead along
    /// the exit yaw at eye height, to fly through. No exit ghost; it would
    /// stand in the camera.
    pub fn local_teleported(&mut self, from: DVec3, to: DVec3, entry_yaw: f64, exit_yaw: f64, team: TeamColor) {
        self.sounds.push(TeleportSound { at: None });
        self.spawn_ghost(from, entry_yaw, team);
        self.spawn_burst(from, true);
        self.spawn_burst(
            DVec3::new(to.x + exit_yaw.sin() * 90.0, to.y + 26.0, to.z + exit_yaw.cos() * 90.0),
            false,
        );
    }

    /// `spawnGhost`: a free slot, or none (never stolen mid-fade).
    fn spawn_ghost(&mut self, at: DVec3, yaw: f64, team: TeamColor) {
        if !self.has_ghosts {
            return;
        }
        let Some(slot) = self.busy.iter().position(|b| !b) else {
            return;
        };
        self.busy[slot] = true;
        self.effects.push(TeleportPieceState {
            piece: TeleportPiece::Ghost { slot, at, yaw, team },
            elapsed_ms: 0.0,
        });
    }

    /// `spawnBurst`, or `spawnImplosion` when `implode`.
    fn spawn_burst(&mut self, at: DVec3, implode: bool) {
        if self.effects.len() >= MAX_EFFECTS {
            return;
        }
        let (from, to) = if implode {
            (IMPLOSION_START_SCALE, IMPLOSION_END_SCALE)
        } else {
            (BURST_START_SCALE, BURST_END_SCALE)
        };
        self.effects.push(TeleportPieceState {
            piece: TeleportPiece::Burst {
                at: DVec3::new(at.x, at.y + BURST_Y_OFFSET, at.z),
                from,
                to,
            },
            elapsed_ms: 0.0,
        });
    }

    /// `update`: `dt` seconds on, at most 100 ms a frame; finished ghosts
    /// give their slots back.
    pub fn update(&mut self, dt: f64) {
        let step = (dt * 1000.0).min(MAX_STEP_MS);
        for effect in &mut self.effects {
            effect.elapsed_ms += step;
        }
        let busy = &mut self.busy;
        self.effects.retain(|e| {
            let alive = e.elapsed_ms < e.lifetime();
            if !alive && let TeleportPiece::Ghost { slot, .. } = e.piece {
                busy[slot] = false;
            }
            alive
        });
    }

    #[must_use]
    pub fn effects(&self) -> &[TeleportPieceState] {
        &self.effects
    }

    /// The sounds asked for since the last call.
    pub fn take_sounds(&mut self) -> Vec<TeleportSound> {
        std::mem::take(&mut self.sounds)
    }
}

/// Plays the entry sound: 2D for ourselves, 3D for others.
pub fn play_tele_in(audio: &mut AudioV2Sfx, sound: &TeleportSound) {
    match sound.at {
        None => {
            audio.play_2d(
                TELE_IN_SOUND,
                &SfxPlayOptions {
                    volume: Some(0.8),
                    max_instances: Some(2),
                    ..SfxPlayOptions::default()
                },
            );
        }
        Some(at) => {
            audio.play_3d(
                TELE_IN_SOUND,
                at,
                &SfxPlayOptions {
                    volume: Some(0.85),
                    ..SfxPlayOptions::default()
                },
            );
        }
    }
}

/// `legacyTeamColor`: an ally is udon, an enemy soba, anyone else by the
/// sum of their id's character codes.
#[must_use]
pub fn legacy_team_color(ally: Option<bool>, entity_id: &str) -> TeamColor {
    match ally {
        Some(true) => TeamColor::Alpha,
        Some(false) => TeamColor::Bravo,
        None => {
            let hash: u32 = entity_id.encode_utf16().map(u32::from).sum();
            if hash.is_multiple_of(2) {
                TeamColor::Alpha
            } else {
                TeamColor::Bravo
            }
        }
    }
}

/// `buildStarburstMesh`: tapered spikes between the two radii, in the
/// local XY plane.
#[must_use]
pub fn starburst_template() -> EffectTemplate {
    let mut template = EffectTemplate::default();
    for i in 0..BURST_SPIKES {
        let angle = i as f32 / BURST_SPIKES as f32 * std::f32::consts::TAU;
        let (sin, cos) = angle.sin_cos();
        let (px, py) = (-sin, cos);
        let base = template.positions.len() as u32;
        template.positions.extend([
            [
                cos * BURST_INNER_RADIUS + px * BURST_BASE_HALF_WIDTH,
                sin * BURST_INNER_RADIUS + py * BURST_BASE_HALF_WIDTH,
                0.0,
            ],
            [
                cos * BURST_INNER_RADIUS - px * BURST_BASE_HALF_WIDTH,
                sin * BURST_INNER_RADIUS - py * BURST_BASE_HALF_WIDTH,
                0.0,
            ],
            [cos * BURST_OUTER_RADIUS, sin * BURST_OUTER_RADIUS, 0.0],
        ]);
        template.uvs.extend([[0.0, 0.0]; 3]);
        template.indices.extend([base, base + 1, base + 2]);
    }
    template
}

const BURST_WGSL: &str = include_str!("teleport_effect.wgsl");

/// The bursts (two fans each) and the ghost pool's figures.
pub struct TeleportRenderer {
    bursts: EffectQuadBatch,
    instances: Vec<EffectInstance>,
    ghost_model: Option<ModelHandle>,
    /// The pool's own posed figures, holding the idle clip.
    ghosts: Vec<PlayerModelInstance>,
}

impl TeleportRenderer {
    /// The bursts, and the ghosts when the player model is loaded
    /// (`ensureGhostPool`): it is uploaded again with the silhouette
    /// material (flat, unlit, blended, both sides).
    pub fn new(
        gpu: &Gpu,
        color_format: wgpu::TextureFormat,
        models: &mut ModelRenderer,
        player: Option<&PlayerModel>,
    ) -> Self {
        let ghost_model = player.map(|p| {
            models.upload(gpu, &p.model, |_| {
                ModelMaterialMode::Standard(StandardMaterialParams {
                    emissive_color: TeamColor::Alpha.color(),
                    diffuse_color: Vec3::ZERO,
                    specular_color: Vec3::ZERO,
                    disable_lighting: true,
                    alpha: GHOST_ALPHA_STEPS[0] as f32,
                    alpha_blend: true,
                    back_face_culling: false,
                    ..StandardMaterialParams::default()
                })
            })
        });
        let ghosts = player.map_or_else(Vec::new, |p| {
            (0..GHOST_POOL_SIZE)
                .map(|_| {
                    let mut instance = p.create_instance();
                    p.play_animation(&mut instance, "idle");
                    instance
                })
                .collect()
        });
        Self {
            bursts: EffectQuadBatch::with_template(
                gpu,
                color_format,
                "teleport-bursts",
                BURST_WGSL,
                MAX_EFFECTS * 2,
                &starburst_template(),
            ),
            instances: Vec::with_capacity(MAX_EFFECTS * 2),
            ghost_model,
            ghosts,
        }
    }

    /// Whether the ghost pool is loaded.
    #[must_use]
    pub fn has_ghosts(&self) -> bool {
        self.ghost_model.is_some()
    }

    /// The ghosts' idle clips run on.
    pub fn advance(&mut self, player: Option<&PlayerModel>, dt: f64) {
        if let Some(p) = player {
            for ghost in &mut self.ghosts {
                p.advance(ghost, dt);
            }
        }
    }

    /// The standing ghosts' draws, their joint palettes appended to
    /// `joints`.
    pub fn ghost_draws(
        &self,
        player: Option<&PlayerModel>,
        effect: &TeleportEffect,
        joints: &mut Vec<Mat4>,
        draws: &mut Vec<ModelDraw>,
    ) {
        let (Some(handle), Some(p)) = (self.ghost_model, player) else {
            return;
        };
        for state in effect.effects() {
            let TeleportPiece::Ghost { slot, at, yaw, team } = state.piece else {
                continue;
            };
            let Some(instance) = self.ghosts.get(slot) else {
                continue;
            };
            let world = Mat4::from_translation(Vec3::new(
                at.x as f32,
                (at.y + GHOST_ANCHOR_Y_OFFSET) as f32,
                at.z as f32,
            )) * Mat4::from_quat(Quat::from_rotation_y(yaw as f32))
                * Mat4::from_scale(Vec3::splat(GHOST_MODEL_SCALE));
            let mut palettes: HashMap<usize, u32> = HashMap::new();
            for (index, primitive) in p.model.primitives.iter().enumerate() {
                let Some(skin) = &primitive.skin else { continue };
                let offset = *palettes.entry(skin.skin).or_insert_with(|| {
                    let at = joints.len() as u32;
                    joints.extend(p.joint_matrices(instance, skin.skin));
                    at
                });
                draws.push(ModelDraw {
                    skin: Some(offset),
                    emissive: Some(team.color()),
                    alpha: Some(state.alpha() as f32),
                    ..ModelDraw::new(handle, index, world)
                });
            }
        }
    }

    /// The bursts' fans this frame.
    pub fn prepare(&mut self, gpu: &Gpu, frame: &EffectFrame, effect: &TeleportEffect) {
        self.instances.clear();
        for state in effect.effects() {
            let TeleportPiece::Burst { at, .. } = state.piece else {
                continue;
            };
            let scale = state.scale() as f32;
            let alpha = state.alpha() as f32;
            let [r, g, b] = BURST_COLOR;
            // The billboarded fan, then its child: interleaved, spinning,
            // at 0.8 of its size.
            for (turn, size) in [(0.0, scale), (state.spin() as f32, scale * 0.8)] {
                let mut instance = [0.0f32; 20];
                instance[..4].copy_from_slice(&[at.x as f32, at.y as f32, at.z as f32, size]);
                instance[4..8].copy_from_slice(&[turn, alpha, 0.0, 0.0]);
                instance[8..12].copy_from_slice(&[r, g, b, 0.0]);
                self.instances.push(instance);
            }
        }
        self.bursts.prepare(gpu, frame, &self.instances, true);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.bursts.draw(pass);
    }
}

/// `MODEL_SCALE` is the remote players' size; the ghosts match it.
const _: () = assert!(MODEL_SCALE == GHOST_MODEL_SCALE);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remote_teleport_marks_both_ends() {
        let mut fx = TeleportEffect::new(true);
        fx.player_teleported(DVec3::ZERO, DVec3::new(500.0, 0.0, 0.0), 0.0, 1.0, TeamColor::Bravo);
        let kinds: Vec<_> = fx.effects().iter().map(|e| e.piece).collect();
        assert_eq!(kinds.len(), 4);
        assert!(matches!(kinds[0], TeleportPiece::Ghost { slot: 0, .. }));
        assert_eq!(
            kinds[1],
            TeleportPiece::Burst {
                at: DVec3::new(0.0, BURST_Y_OFFSET, 0.0),
                from: IMPLOSION_START_SCALE,
                to: IMPLOSION_END_SCALE
            }
        );
        assert!(matches!(kinds[2], TeleportPiece::Ghost { slot: 1, .. }));
        assert_eq!(fx.take_sounds(), vec![TeleportSound { at: Some(DVec3::ZERO) }]);
    }

    #[test]
    fn our_own_exit_burst_is_thrown_ahead() {
        let mut fx = TeleportEffect::new(true);
        fx.local_teleported(DVec3::ZERO, DVec3::new(100.0, 0.0, 0.0), 0.0, 0.0, TeamColor::Alpha);
        assert_eq!(fx.effects().len(), 3);
        let TeleportPiece::Burst { at, from, .. } = fx.effects()[2].piece else {
            panic!("a burst")
        };
        assert_eq!(at, DVec3::new(100.0, 26.0 + BURST_Y_OFFSET, 90.0));
        assert_eq!(from, BURST_START_SCALE);
        assert_eq!(fx.take_sounds(), vec![TeleportSound { at: None }]);
    }

    #[test]
    fn fades_step_and_slots_come_back() {
        let mut fx = TeleportEffect::new(true);
        for _ in 0..3 {
            fx.player_teleported(DVec3::ZERO, DVec3::X * 300.0, 0.0, 0.0, TeamColor::Alpha);
        }
        // Four slots: the third teleport gets no ghosts.
        let ghosts = fx
            .effects()
            .iter()
            .filter(|e| matches!(e.piece, TeleportPiece::Ghost { .. }))
            .count();
        assert_eq!(ghosts, 4);
        assert_eq!(fx.effects()[0].alpha(), 0.85);
        fx.update(0.1);
        fx.update(0.02);
        // 120 of 450 ms: the second step; the burst at 120 of 420 too.
        assert_eq!(fx.effects()[0].alpha(), 0.55);
        assert_eq!(fx.effects()[1].alpha(), 0.7);
        // A long frame counts as 100 ms.
        fx.update(5.0);
        fx.update(0.1);
        fx.update(0.1);
        fx.update(0.04);
        assert!(fx.effects().is_empty());
        fx.player_teleported(DVec3::ZERO, DVec3::X * 300.0, 0.0, 0.0, TeamColor::Alpha);
        assert!(matches!(fx.effects()[0].piece, TeleportPiece::Ghost { slot: 0, .. }));
    }

    #[test]
    fn bursts_grow_and_spin() {
        let mut fx = TeleportEffect::new(false);
        fx.player_teleported(DVec3::ZERO, DVec3::X * 300.0, 0.0, 0.0, TeamColor::Alpha);
        assert_eq!(fx.effects().len(), 2);
        fx.update(0.042);
        let exit = fx.effects()[1];
        assert!((exit.scale() - (0.5 + 1.1 * 0.1)).abs() < 1e-12);
        assert!((exit.spin() - (PI / 16.0 + 0.09)).abs() < 1e-12);
    }

    #[test]
    fn colours_by_side_or_id() {
        assert_eq!(legacy_team_color(Some(true), "x"), TeamColor::Alpha);
        assert_eq!(legacy_team_color(Some(false), "x"), TeamColor::Bravo);
        // "ab": 97 + 98 is odd.
        assert_eq!(legacy_team_color(None, "ab"), TeamColor::Bravo);
        assert_eq!(legacy_team_color(None, "aa"), TeamColor::Alpha);
    }

    #[test]
    fn the_star_has_its_spikes() {
        let star = starburst_template();
        assert_eq!(star.positions.len(), 48);
        assert_eq!(star.indices.len(), 48);
        assert!((star.positions[2][0] - 84.0).abs() < 1e-4);
    }
}
