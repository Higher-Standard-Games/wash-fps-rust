//! Port of wash-fps `client/FPSLiquidAudio.ts`: the sound of entering,
//! leaving and being under a liquid. Local sounds play 2D, at your ears;
//! everyone else's in the world.

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::audio::mixer::SfxChannel;
use shagger::rendering::glam::DVec3;

use crate::client::fps_player_audio::{PlayerVoiceSide, player_voice_set, player_voice_sound};

use crate::liquid::liquid_types::LiquidKind;

const WATER_IN: &str = "sounds/world/water_in.ogg";
const WATER_OUT: &str = "sounds/world/water_out.ogg";
const UNDERWATER: &str = "sounds/world/underwater.ogg";
const LAVA_IN: &str = "sounds/world/lava_in.ogg";
const LAVA_OUT: &str = "sounds/world/lava_out.ogg";
const UNDERLAVA: &str = "sounds/world/underlava.ogg";
/// Projectile and body impacts on a surface, rather than a body crossing it.
pub const HIT_WATER: &str = "sounds/misc/hit_water.wav";

/// `FPS_LIQUID_SOUND_PATHS`.
pub const FPS_LIQUID_SOUND_PATHS: [&str; 7] =
    [WATER_IN, WATER_OUT, UNDERWATER, LAVA_IN, LAVA_OUT, UNDERLAVA, HIT_WATER];

/// `enterSound`: slime borrows water's sounds.
#[must_use]
pub fn enter_sound(kind: LiquidKind) -> &'static str {
    if kind == LiquidKind::Lava { LAVA_IN } else { WATER_IN }
}

/// `exitSound`.
#[must_use]
pub fn exit_sound(kind: LiquidKind) -> &'static str {
    if kind == LiquidKind::Lava { LAVA_OUT } else { WATER_OUT }
}

/// `submergedSound`.
#[must_use]
pub fn submerged_sound(kind: LiquidKind) -> &'static str {
    if kind == LiquidKind::Lava {
        UNDERLAVA
    } else {
        UNDERWATER
    }
}

/// `splashVolume`: how loud a crossing is, from how fast the body went
/// through the surface. A drift across the waterline is silent.
#[must_use]
pub fn splash_volume(vertical_speed: f64) -> f64 {
    let speed = vertical_speed.abs();
    if speed < 40.0 {
        return 0.0;
    }
    1f64.min(0.25 + (speed / 700.0) * 0.75)
}

/// `playDrownGasp`: 2D and local, the gasp when the lungs empty.
pub fn play_drown_gasp(audio: &mut AudioV2Sfx) {
    play_drown(audio, 1.0);
}

/// `playDrownTick`: quieter, each drowning hit after.
pub fn play_drown_tick(audio: &mut AudioV2Sfx) {
    play_drown(audio, 0.55);
}

fn play_drown(audio: &mut AudioV2Sfx, volume: f32) {
    let path = player_voice_sound("drown", player_voice_set(PlayerVoiceSide::Ally));
    audio.play_2d(
        &path,
        &SfxPlayOptions {
            volume: Some(volume),
            max_instances: Some(1),
            channel: SfxChannel::Players,
            ..SfxPlayOptions::default()
        },
    );
}

/// A crossing to play (`playLiquidCrossing`'s arguments).
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidCrossingSound {
    pub path: &'static str,
    pub volume: f64,
    /// Babylon space.
    pub position: DVec3,
    pub local: bool,
}

/// `playLiquidCrossing`: local plays 2D, remote plays in the world.
pub fn play_liquid_crossing(audio: &mut AudioV2Sfx, sound: &LiquidCrossingSound) {
    if sound.volume <= 0.0 {
        return;
    }
    let volume = Some(sound.volume as f32);
    if sound.local {
        audio.play_2d(
            sound.path,
            &SfxPlayOptions {
                volume,
                max_instances: Some(3),
                ..SfxPlayOptions::default()
            },
        );
    } else {
        audio.play_3d(
            sound.path,
            sound.position,
            &SfxPlayOptions {
                volume,
                ..SfxPlayOptions::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splash_volume_matches_the_typescript() {
        assert_eq!(splash_volume(0.0), 0.0);
        assert_eq!(splash_volume(20.0), 0.0);
        assert!(splash_volume(600.0) > splash_volume(100.0));
        assert_eq!(splash_volume(-500.0), splash_volume(500.0));
        assert!(splash_volume(50000.0) <= 1.0);
    }
}
