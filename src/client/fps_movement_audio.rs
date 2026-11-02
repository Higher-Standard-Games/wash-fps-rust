//! wash-fps `client/FPSMovementAudio.ts`: footsteps by stride, ladder rungs,
//! jump grunts, and the jump pad's launch, from each snapshot's players.

use std::collections::{HashMap, HashSet};

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::audio::mixer::SfxChannel;
use shagger::network::client::snapshot_buffer::EntitySnapshot;
use shagger::physics::content_flags::surface;
use shagger::rendering::glam::DVec3;

use crate::client::fps_player_audio::{player_voice_for_entity, player_voice_sound};

const NORMAL_FOOTSTEPS: [&str; 3] = [
    "sounds/world/ft_step1.ogg",
    "sounds/world/ft_step2.ogg",
    "sounds/world/ft_step3.ogg",
];
const METAL_FOOTSTEPS: [&str; 3] = [
    "sounds/world/ft_step_met1.ogg",
    "sounds/world/ft_step_met2.ogg",
    "sounds/world/ft_step_met3.ogg",
];

/// Vertical units between ladder rung sounds.
const LADDER_RUNG_SPACING: f64 = 44.0;

/// The jump pad's launch.
const JUMPPAD_LAUNCH: &str = "sounds/world/jumppad.ogg";

/// `FPS_MOVEMENT_SOUND_PATHS`.
pub const FPS_MOVEMENT_SOUND_PATHS: [&str; 7] = [
    NORMAL_FOOTSTEPS[0],
    NORMAL_FOOTSTEPS[1],
    NORMAL_FOOTSTEPS[2],
    METAL_FOOTSTEPS[0],
    METAL_FOOTSTEPS[1],
    METAL_FOOTSTEPS[2],
    JUMPPAD_LAUNCH,
];

#[derive(Debug, Clone, Copy)]
struct MovementAudioState {
    position: DVec3,
    grounded: bool,
    step_distance: f64,
    /// Vertical travel banked toward the next ladder rung.
    climb_distance: f64,
}

/// The module's state.
#[derive(Debug, Clone, Default)]
pub struct FpsMovementAudio {
    states: HashMap<String, MovementAudioState>,
    footstep_sequence: usize,
    jump_sequence: usize,
}

fn play_at_player(
    audio: &mut AudioV2Sfx,
    path: &str,
    position: DVec3,
    local: bool,
    volume: f32,
    channel: SfxChannel,
    slot: Option<String>,
) {
    if local {
        audio.play_2d(
            path,
            &SfxPlayOptions {
                volume: Some(volume),
                max_instances: Some(2),
                channel,
                ..SfxPlayOptions::default()
            },
        );
    } else {
        audio.play_3d(
            path,
            position,
            &SfxPlayOptions {
                volume: Some(volume),
                channel,
                slot,
                ..SfxPlayOptions::default()
            },
        );
    }
}

/// `playJumppadLaunch`, on the rising edge of a launch.
pub fn play_jumppad_launch(audio: &mut AudioV2Sfx, position: DVec3, local: bool) {
    play_at_player(audio, JUMPPAD_LAUNCH, position, local, 0.75, SfxChannel::Effects, None);
}

impl FpsMovementAudio {
    /// `resetFPSMovementAudio`.
    pub fn reset(&mut self) {
        self.states.clear();
    }

    /// `processFPSMovementAudio`: one snapshot's players. Distance, not a
    /// timer, sets the cadence. `ground_flags` gives the surface flags of
    /// the ground under a position (`getGroundSurfaceFlags`).
    pub fn process(
        &mut self,
        audio: &mut AudioV2Sfx,
        players: &HashMap<String, EntitySnapshot>,
        local_entity_id: Option<&str>,
        teams: &HashMap<String, String>,
        ground_flags: &mut dyn FnMut(DVec3) -> i32,
    ) {
        let mut alive = HashSet::new();
        // In id order, so the variant sequences do not depend on hashing.
        let mut ids: Vec<&String> = players.keys().collect();
        ids.sort();
        for entity_id in ids {
            let entity = &players[entity_id];
            alive.insert(entity_id.clone());
            let position = DVec3::new(entity.x, entity.y, entity.z);
            let Some(previous) = self.states.get_mut(entity_id) else {
                self.states.insert(
                    entity_id.clone(),
                    MovementAudioState {
                        position,
                        grounded: entity.grounded,
                        step_distance: 0.0,
                        climb_distance: 0.0,
                    },
                );
                continue;
            };
            let dx = position.x - previous.position.x;
            let dz = position.z - previous.position.z;
            let horizontal_travel = dx.hypot(dz);
            let horizontal_speed = entity.vx.hypot(entity.vz);
            let local = Some(entity_id.as_str()) == local_entity_id;

            // A grounded to rising edge is a jump; teleports and seeks are
            // rejected by the travel cap.
            if previous.grounded && !entity.grounded && entity.vy > 60.0 && horizontal_travel < 256.0 {
                let cue = if self.jump_sequence.is_multiple_of(2) {
                    "jump_1"
                } else {
                    "jump_2"
                };
                self.jump_sequence += 1;
                let path = player_voice_sound(cue, player_voice_for_entity(teams, entity_id, local_entity_id));
                play_at_player(
                    audio,
                    &path,
                    position,
                    local,
                    0.72,
                    SfxChannel::Players,
                    Some(format!("{entity_id}:body")),
                );
            }

            if entity.on_ladder == Some(true) {
                previous.climb_distance += (position.y - previous.position.y).abs();
                if previous.climb_distance >= LADDER_RUNG_SPACING {
                    previous.climb_distance %= LADDER_RUNG_SPACING;
                    // playRungStep: metal, quieter than a step.
                    let path = METAL_FOOTSTEPS[self.footstep_sequence % METAL_FOOTSTEPS.len()];
                    self.footstep_sequence += 1;
                    play_at_player(audio, path, position, local, 0.38, SfxChannel::Players, None);
                }
                previous.step_distance = 0.0;
            } else if horizontal_travel > 256.0 {
                previous.climb_distance = 0.0;
                previous.step_distance = 0.0;
            } else if entity.grounded && !entity.sliding && horizontal_speed > 70.0 {
                previous.step_distance += horizontal_travel;
                let stride = if entity.crouching {
                    96.0
                } else if horizontal_speed > 500.0 {
                    145.0
                } else {
                    122.0
                };
                if previous.step_distance >= stride {
                    previous.step_distance %= stride;
                    // playFootstep: the ground's flags pick the bank.
                    let flags = ground_flags(position);
                    if flags & surface::NOSTEPS == 0 {
                        let bank = if flags & surface::METALSTEPS != 0 {
                            &METAL_FOOTSTEPS
                        } else {
                            &NORMAL_FOOTSTEPS
                        };
                        let path = bank[self.footstep_sequence % bank.len()];
                        self.footstep_sequence += 1;
                        play_at_player(audio, path, position, local, 0.5, SfxChannel::Players, None);
                    }
                }
            } else if !entity.grounded {
                previous.step_distance = 0.0;
            }

            previous.position = position;
            previous.grounded = entity.grounded;
        }
        self.states.retain(|id, _| alive.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, grounded: bool, vy: f64) -> HashMap<String, EntitySnapshot> {
        [(
            "player_2".to_owned(),
            EntitySnapshot {
                x,
                grounded,
                vx: 320.0,
                vy,
                ..EntitySnapshot::default()
            },
        )]
        .into()
    }

    /// FPSMovementAudio.test.ts: a stride makes a step, a jump a grunt.
    #[test]
    fn strides_step_and_jumps_grunt() {
        let mut audio = AudioV2Sfx::offline(48_000);
        // Nothing loaded: count what would play through the counters.
        let mut movement = FpsMovementAudio::default();
        let teams = HashMap::new();
        let mut flags = |_| 0;
        movement.process(&mut audio, &at(0.0, true, 0.0), Some("player_1"), &teams, &mut flags);
        movement.process(&mut audio, &at(130.0, true, 0.0), Some("player_1"), &teams, &mut flags);
        assert_eq!(movement.footstep_sequence, 1);
        movement.process(
            &mut audio,
            &at(150.0, false, 240.0),
            Some("player_1"),
            &teams,
            &mut flags,
        );
        assert_eq!(movement.jump_sequence, 1);
        movement.process(&mut audio, &HashMap::new(), None, &teams, &mut flags);
        assert!(movement.states.is_empty());
    }
}
