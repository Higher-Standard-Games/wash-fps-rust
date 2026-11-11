//! The Voltaic's sustained hum (wash-fps `client/FPSVoltaicAudio.ts`):
//! one looping voice per shooter while the beam is held, an attack on the
//! first shot and a release when it stops, a semitone and a half up under
//! quad. The beam's single shots are not played (`playWeaponFireSound`
//! skips the Voltaic).
//!
//! The browser keeps a voice while the server's tick is within 0.12 s of
//! the shooter's last fire tick (from `weapon_fire`, or `weapon_state`'s
//! `lastFireTick`). The native frame has no server tick, so the same
//! window is measured on the effect clock from the last `weapon_fire`
//! seen; `weapon_state` still ends a voice on a switch or a reload.

use std::collections::{BTreeMap, BTreeSet};

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxLoop, SfxPlayOptions, attenuation};
use shagger::audio::loop_overlap::{LoopIntro, LoopRegion, LoopShape};
use shagger::rendering::glam::DVec3;

use crate::client::game_events::canonical_weapon_id;
use crate::shared::net::snapshot_events::SnapshotEvent;

/// `VOLTAIC_SOUNDS`.
pub const VOLTAIC_START: &str = "sounds/weapons/laser_strong_fire.ogg";
pub const VOLTAIC_STOP: &str = "sounds/weapons/laser_strong_stop.ogg";
/// `Math.ceil(tickRate * 0.12)` ticks, in seconds.
const HOLD_SECONDS: f64 = 0.12;

/// `voltaicLoopOptions`' buffer half.
#[must_use]
pub fn voltaic_loop_shape(quad: bool, attack: bool) -> LoopShape {
    LoopShape {
        loop_region: Some(LoopRegion {
            start_seconds: 0.1,
            end_seconds: 0.25,
        }),
        overlap_seconds: 0.03,
        intro: attack.then(|| LoopIntro {
            path: VOLTAIC_START.to_owned(),
            hold_seconds: 0.12,
            fade_seconds: 0.06,
        }),
        pitch: if quad { 150.0 } else { 0.0 },
    }
}

fn loop_options() -> SfxPlayOptions {
    SfxPlayOptions {
        volume: Some(0.9),
        attenuation: Some(attenuation::NORMAL),
        ..SfxPlayOptions::default()
    }
}

#[derive(Debug, Clone)]
struct Voice {
    position: DVec3,
    viewed: bool,
    quad: bool,
    last_fire: f64,
    sound: Option<SfxLoop>,
}

/// `FPSVoltaicAudio`.
#[derive(Debug, Default)]
pub struct FpsVoltaicAudio {
    voices: BTreeMap<String, Voice>,
}

impl FpsVoltaicAudio {
    /// `present`: this frame's events at `now` seconds on the effect clock.
    /// Nothing plays while `present` is false (a paused or seeking demo).
    pub fn present(
        &mut self,
        audio: &mut AudioV2Sfx,
        events: &[SnapshotEvent],
        viewed_entity: Option<&str>,
        position: &dyn Fn(&str) -> Option<DVec3>,
        now: f64,
        present: bool,
    ) {
        if !present {
            self.reset(audio);
            return;
        }
        let mut weapons: BTreeMap<&str, (&str, bool)> = BTreeMap::new();
        let mut shots: BTreeSet<&str> = BTreeSet::new();
        let mut dead: BTreeSet<&str> = BTreeSet::new();
        let mut quad: BTreeSet<&str> = BTreeSet::new();
        for event in events {
            match event {
                SnapshotEvent::WeaponState {
                    entity_id,
                    current_weapon,
                    reloading,
                    ..
                } => {
                    weapons.insert(entity_id, (current_weapon, *reloading));
                }
                SnapshotEvent::WeaponFire {
                    entity_id, weapon_id, ..
                } if canonical_weapon_id(weapon_id) == "voltaic" => {
                    shots.insert(entity_id);
                }
                SnapshotEvent::Death { entity_id, .. } => {
                    dead.insert(entity_id);
                }
                SnapshotEvent::CombatState {
                    entity_id, dead: true, ..
                } => {
                    dead.insert(entity_id);
                }
                SnapshotEvent::PowerupState {
                    entity_id,
                    kind,
                    seconds,
                } if kind == "quad" && *seconds > 0 => {
                    quad.insert(entity_id);
                }
                _ => {}
            }
        }
        let ids: BTreeSet<String> = self
            .voices
            .keys()
            .cloned()
            .chain(shots.iter().map(|s| (*s).to_owned()))
            .collect();
        for id in ids {
            let viewed = Some(id.as_str()) == viewed_entity;
            let previous = self.voices.get(&id).cloned();
            let shot = shots.contains(id.as_str());
            // The one whose eyes we look through is heard in 2D: no place
            // needed.
            let at = position(&id)
                .or(previous.as_ref().map(|v| v.position))
                .or(viewed.then_some(DVec3::ZERO));
            let Some(at) = at.filter(|_| !dead.contains(id.as_str())) else {
                self.stop(audio, &id, false);
                continue;
            };
            let last_fire = if shot {
                now
            } else {
                previous.as_ref().map_or(f64::NEG_INFINITY, |v| v.last_fire)
            };
            let switched = weapons
                .get(id.as_str())
                .is_some_and(|(weapon, reloading)| canonical_weapon_id(weapon) != "voltaic" || *reloading);
            if switched || now < last_fire || now - last_fire > HOLD_SECONDS {
                // A final shot that reloads at once still needs its attack.
                if shot && previous.is_none() {
                    cue(audio, VOLTAIC_START, viewed, at);
                }
                self.stop(audio, &id, true);
                continue;
            }
            let powered = quad.contains(id.as_str());
            match previous {
                Some(v) if v.viewed == viewed && v.quad == powered => {
                    let voice = self.voices.get_mut(&id).expect("voice");
                    voice.last_fire = last_fire;
                    voice.position = at;
                    if let (Some(sound), false) = (voice.sound, viewed) {
                        audio.set_loop_position(sound, at);
                    }
                }
                _ => {
                    let attack = previous.is_none();
                    self.stop(audio, &id, false);
                    let sound = audio.play_loop_shaped(
                        VOLTAIC_START,
                        (!viewed).then_some(at),
                        &loop_options(),
                        &voltaic_loop_shape(powered, attack),
                    );
                    self.voices.insert(
                        id,
                        Voice {
                            position: at,
                            viewed,
                            quad: powered,
                            last_fire,
                            sound,
                        },
                    );
                }
            }
        }
    }

    /// `reset`: every voice stops, without a release.
    pub fn reset(&mut self, audio: &mut AudioV2Sfx) {
        let ids: Vec<String> = self.voices.keys().cloned().collect();
        for id in ids {
            self.stop(audio, &id, false);
        }
    }

    fn stop(&mut self, audio: &mut AudioV2Sfx, id: &str, release: bool) {
        let Some(voice) = self.voices.remove(id) else { return };
        if let Some(sound) = voice.sound {
            audio.stop_loop(sound);
            if release {
                cue(audio, VOLTAIC_STOP, voice.viewed, voice.position);
            }
        }
    }

    /// Voices sounding, for tests.
    #[must_use]
    pub fn voices(&self) -> usize {
        self.voices.len()
    }
}

fn cue(audio: &mut AudioV2Sfx, path: &str, viewed: bool, at: DVec3) {
    if viewed {
        audio.play_2d(
            path,
            &SfxPlayOptions {
                volume: Some(0.9),
                max_instances: Some(2),
                ..SfxPlayOptions::default()
            },
        );
    } else {
        audio.play_3d(
            path,
            at,
            &SfxPlayOptions {
                volume: Some(0.9),
                ..SfxPlayOptions::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::net::snapshot_events::Point;

    fn fire(id: &str) -> SnapshotEvent {
        let p = Point { x: 0.0, y: 0.0, z: 0.0 };
        SnapshotEvent::WeaponFire {
            entity_id: id.into(),
            weapon_id: "voltaic".into(),
            origin: p,
            dir: p,
            pellet: None,
        }
    }

    #[test]
    fn a_voice_holds_while_the_beam_fires_then_releases() {
        let mut audio = AudioV2Sfx::offline(48000);
        let mut voltaic = FpsVoltaicAudio::default();
        let at = |_: &str| Some(DVec3::ZERO);
        voltaic.present(&mut audio, &[fire("p")], None, &at, 1.0, true);
        assert_eq!(voltaic.voices(), 1);
        // Between shots, inside the window.
        voltaic.present(&mut audio, &[], None, &at, 1.1, true);
        assert_eq!(voltaic.voices(), 1);
        voltaic.present(&mut audio, &[fire("p")], None, &at, 1.15, true);
        voltaic.present(&mut audio, &[], None, &at, 1.3, true);
        assert_eq!(voltaic.voices(), 0);
        // A switch ends it at once; a pause silences it.
        voltaic.present(&mut audio, &[fire("p")], None, &at, 2.0, true);
        voltaic.present(&mut audio, &[], None, &at, 2.0, false);
        assert_eq!(voltaic.voices(), 0);
        let other = |_: &str| None;
        voltaic.present(&mut audio, &[fire("p")], None, &other, 3.0, true);
        assert_eq!(voltaic.voices(), 0, "nowhere to place it");
        assert_eq!(voltaic_loop_shape(true, false).pitch, 150.0);
    }
}
