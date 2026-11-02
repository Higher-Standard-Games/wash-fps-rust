//! The bomb's beep and its announcer lines (wash-fps `client/FPSBombAudio.ts`).
//!
//! The beep is the clock: 2D, heard by everyone, on the interval the server
//! sends (a twelfth of the remaining fuse). The announcer lines are 2D and
//! read for the listener's side; they go through the match's one voice.

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::shared::json::Value;

use crate::client::fps_announcer::FpsAnnouncer;
use crate::client::match_state_manager::MatchPhase;

const BOMB_TIMER: &str = "sounds/bomb/bombtimer.ogg";

/// `ANNOUNCE`: (offense, defense) per event.
const TAKEN: (Option<&str>, Option<&str>) = (Some("sounds/announcer/bomb/offense/taken.ogg"), None);
const PLANTED: (Option<&str>, Option<&str>) = (Some("sounds/announcer/bomb/offense/inplace.ogg"), None);
const ARMED: (Option<&str>, Option<&str>) = (
    Some("sounds/announcer/bomb/offense/planted.ogg"),
    Some("sounds/announcer/bomb/defense/planted.ogg"),
);
const DEFUSED: (Option<&str>, Option<&str>) = (
    Some("sounds/announcer/bomb/offense/defused.ogg"),
    Some("sounds/announcer/bomb/defense/defused.ogg"),
);

/// `ROUND`.
const ROUND_GO: &str = "sounds/announcer/countdown/fight01.ogg";
const ROUND_START: (&str, &str) = (
    "sounds/announcer/bomb/offense/start.ogg",
    "sounds/announcer/bomb/defense/start.ogg",
);
/// The spoken 3-2-1; index 0 is unused.
const ROUND_COUNT: [Option<&str>; 4] = [
    None,
    Some("sounds/announcer/countdown/1_01.ogg"),
    Some("sounds/announcer/countdown/2_01.ogg"),
    Some("sounds/announcer/countdown/3_01.ogg"),
];

/// `LAST_CALL`.
const LAST_CALL: (&str, &str) = (
    "sounds/announcer/bomb/offense/lastcall.ogg",
    "sounds/announcer/bomb/defense/defuse.ogg",
);
const LAST_CALL_LEAD_SECONDS: f64 = 5.0;

/// `DEFAULT_BOMB_RULES.armTicks` and `defuseTicks` over
/// `DEFAULT_PHYSICS_TICK_RATE`.
const ARM_SECONDS: f64 = (4 * 125) as f64 / 125.0;
const DEFUSE_SECONDS: f64 = (7 * 125) as f64 / 125.0;

/// The detonation, shared with the rocket blast.
pub const BOMB_BLAST_SOUND: &str = "sounds/weapons/rocket_strong_explosion.ogg";

/// `FPS_BOMB_SOUND_PATHS`.
pub const FPS_BOMB_SOUND_PATHS: [&str; 16] = [
    BOMB_TIMER,
    BOMB_BLAST_SOUND,
    "sounds/announcer/bomb/offense/taken.ogg",
    "sounds/announcer/bomb/offense/inplace.ogg",
    "sounds/announcer/bomb/offense/planted.ogg",
    "sounds/announcer/bomb/defense/planted.ogg",
    "sounds/announcer/bomb/offense/defused.ogg",
    "sounds/announcer/bomb/defense/defused.ogg",
    ROUND_GO,
    ROUND_START.0,
    ROUND_START.1,
    "sounds/announcer/countdown/1_01.ogg",
    "sounds/announcer/countdown/2_01.ogg",
    "sounds/announcer/countdown/3_01.ogg",
    LAST_CALL.0,
    LAST_CALL.1,
];

const BEEP_VOLUME: f32 = 0.75;

fn side<T>(pair: (T, T), attacking: bool) -> T {
    if attacking { pair.0 } else { pair.1 }
}

/// The parts of a `BombObjectiveSnapshot` the sounds read.
fn bomb_phase(bomb: &Value) -> &str {
    bomb.get("phase").and_then(Value::as_str).unwrap_or("")
}

fn number(bomb: &Value, key: &str) -> Option<f64> {
    bomb.get(key).and_then(Value::as_f64)
}

/// `FPSBombAudio`.
#[derive(Debug, Default)]
pub struct FpsBombAudio {
    next_beep: f64,
    last_phase: Option<String>,
    last_call_done: bool,
    last_count_second: Option<i64>,
}

impl FpsBombAudio {
    /// `update`: every frame with the objective when it is the bomb; `dt`
    /// in seconds, `now` in ms for the announcer.
    pub fn update(
        &mut self,
        audio: &mut AudioV2Sfx,
        announcer: &mut FpsAnnouncer,
        bomb: Option<&Value>,
        dt: f64,
        attacking: bool,
        now: f64,
    ) {
        let Some(bomb) = bomb else {
            self.reset();
            return;
        };
        let phase = bomb_phase(bomb);
        if self.last_phase.as_deref() != Some(phase) {
            let from = self.last_phase.take();
            self.announce(audio, announcer, from.as_deref(), phase, attacking, now);
            self.last_phase = Some(phase.to_owned());
            if phase == "armed" {
                self.next_beep = 0.0;
            }
        }
        let Some(interval) = number(bomb, "beepInterval") else {
            return;
        };
        if phase != "armed" {
            return;
        }
        self.next_beep -= dt;
        if self.next_beep > 0.0 {
            return;
        }
        self.next_beep = interval;
        audio.play_2d(
            BOMB_TIMER,
            &SfxPlayOptions {
                volume: Some(BEEP_VOLUME),
                max_instances: Some(2),
                ..SfxPlayOptions::default()
            },
        );
    }

    fn announce(
        &mut self,
        audio: &mut AudioV2Sfx,
        announcer: &mut FpsAnnouncer,
        from: Option<&str>,
        to: &str,
        attacking: bool,
        now: f64,
    ) {
        let Some(from) = from else {
            return;
        };
        let path = match to {
            "carried" if from == "dropped" => side(TAKEN, attacking),
            "planted" => side(PLANTED, attacking),
            "armed" => side(ARMED, attacking),
            "defused" => side(DEFUSED, attacking),
            _ => None,
        };
        if let Some(path) = path {
            announcer.queue(audio, path.to_owned(), now);
        }
    }

    fn last_call(
        &mut self,
        audio: &mut AudioV2Sfx,
        announcer: &mut FpsAnnouncer,
        bomb: &Value,
        time_remaining: f64,
        attacking: bool,
        now: f64,
    ) {
        if self.last_call_done {
            return;
        }
        let armed = bomb_phase(bomb) == "armed";
        let due = if attacking {
            !armed && time_remaining > 0.0 && time_remaining <= ARM_SECONDS + LAST_CALL_LEAD_SECONDS
        } else if armed && let Some(fuse) = number(bomb, "fuse") {
            fuse <= DEFUSE_SECONDS + LAST_CALL_LEAD_SECONDS
        } else {
            false
        };
        if !due {
            return;
        }
        self.last_call_done = true;
        announcer.queue(audio, side(LAST_CALL, attacking).to_owned(), now);
    }

    fn countdown(&mut self, audio: &mut AudioV2Sfx, announcer: &mut FpsAnnouncer, time_remaining: f64, now: f64) {
        let second = time_remaining.ceil() as i64;
        if Some(second) == self.last_count_second {
            return;
        }
        self.last_count_second = Some(second);
        if let Some(Some(path)) = usize::try_from(second).ok().and_then(|s| ROUND_COUNT.get(s)) {
            announcer.say(audio, path, now);
        }
    }

    /// `onMatchPhase`: a round announces its own start.
    pub fn on_match_phase(
        &mut self,
        audio: &mut AudioV2Sfx,
        announcer: &mut FpsAnnouncer,
        phase: MatchPhase,
        previous: Option<MatchPhase>,
        attacking: bool,
        now: f64,
    ) {
        let Some(previous) = previous else {
            return;
        };
        if phase == previous {
            return;
        }
        self.last_call_done = false;
        self.last_count_second = None;
        match phase {
            MatchPhase::Countdown => announcer.say(audio, side(ROUND_START, attacking), now),
            MatchPhase::Active => announcer.say(audio, ROUND_GO, now),
            _ => {}
        }
    }

    /// `updateRound`: the spoken 3-2-1 and the last call.
    #[allow(clippy::too_many_arguments)]
    pub fn update_round(
        &mut self,
        audio: &mut AudioV2Sfx,
        announcer: &mut FpsAnnouncer,
        phase: MatchPhase,
        time_remaining: f64,
        bomb: Option<&Value>,
        attacking: bool,
        now: f64,
    ) {
        if phase == MatchPhase::Countdown {
            self.countdown(audio, announcer, time_remaining, now);
            return;
        }
        if phase == MatchPhase::Active
            && let Some(bomb) = bomb
        {
            self.last_call(audio, announcer, bomb, time_remaining, attacking, now);
        }
    }

    /// `reset`: no bomb this frame.
    pub fn reset(&mut self) {
        self.next_beep = 0.0;
        self.last_phase = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shagger::shared::json::parse;

    fn bomb(phase: &str, beep: Option<f64>, fuse: Option<f64>) -> Value {
        let n = |v: Option<f64>| v.map_or("null".to_owned(), |v| v.to_string());
        parse(&format!(
            r#"{{"kind":"bomb","phase":"{phase}","beepInterval":{},"fuse":{}}}"#,
            n(beep),
            n(fuse)
        ))
        .unwrap()
    }

    #[test]
    fn the_beep_speeds_up_with_the_interval_and_lines_wait_for_a_change() {
        let mut audio = AudioV2Sfx::offline(48000);
        let mut announcer = FpsAnnouncer::default();
        let mut sounds = FpsBombAudio::default();
        // The first frame announces nothing.
        sounds.update(
            &mut audio,
            &mut announcer,
            Some(&bomb("armed", Some(1.0), Some(30.0))),
            0.1,
            false,
            0.0,
        );
        assert_eq!(sounds.next_beep, 1.0);
        sounds.update(
            &mut audio,
            &mut announcer,
            Some(&bomb("armed", Some(0.5), Some(20.0))),
            0.6,
            false,
            0.0,
        );
        assert!((sounds.next_beep - 0.4).abs() < 1e-9);
        sounds.update(
            &mut audio,
            &mut announcer,
            Some(&bomb("armed", Some(0.5), Some(20.0))),
            0.5,
            false,
            0.0,
        );
        assert_eq!(sounds.next_beep, 0.5);
        sounds.update(&mut audio, &mut announcer, None, 0.1, false, 0.0);
        assert_eq!(sounds.last_phase, None);
    }

    #[test]
    fn the_last_call_comes_once_a_round_for_the_side_running_out() {
        let mut audio = AudioV2Sfx::offline(48000);
        let mut announcer = FpsAnnouncer::default();
        let mut sounds = FpsBombAudio::default();
        let carried = bomb("carried", None, None);
        let (a, n) = (&mut audio, &mut announcer);
        sounds.update_round(a, n, MatchPhase::Active, 20.0, Some(&carried), true, 0.0);
        assert!(!sounds.last_call_done);
        sounds.update_round(a, n, MatchPhase::Active, 9.0, Some(&carried), true, 0.0);
        assert!(sounds.last_call_done);
        sounds.on_match_phase(a, n, MatchPhase::Countdown, Some(MatchPhase::RoundEnd), true, 0.0);
        assert!(!sounds.last_call_done);
        // A defender hears it when the fuse leaves just a defuse and the lead.
        let armed = bomb("armed", Some(1.0), Some(12.5));
        sounds.update_round(a, n, MatchPhase::Active, 60.0, Some(&armed), false, 0.0);
        assert!(!sounds.last_call_done);
        let armed = bomb("armed", Some(1.0), Some(12.0));
        sounds.update_round(a, n, MatchPhase::Active, 60.0, Some(&armed), false, 0.0);
        assert!(sounds.last_call_done);
    }
}
