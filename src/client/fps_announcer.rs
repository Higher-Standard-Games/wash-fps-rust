//! wash-fps `client/FPSAnnouncer.ts`: the voice that calls the match, one
//! line at a time: the countdown, "fight", the lead, overtime, "game over",
//! and the warmup's ready-up reminder; a timeout: "timeout" when it is
//! called, "time in" as the count back in starts, 1 on its last second, and
//! "fight" when play resumes. A pause holds every other call, in every mode.

use std::collections::VecDeque;

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::audio::mixer::SfxChannel;
use shagger::shared::net::message_codec::MatchPauseState;
use shagger::shared::spread_utils::mulberry32;

use crate::client::match_state_manager::MatchPhase;

/// How long a line holds the voice before the next may start (ms).
const VOICE_HOLD_MS: f64 = 1500.0;
/// Seconds between ready-up reminders in warmup.
const READY_NAG_SECONDS: f64 = 60.0;
/// Time the countdown needs before "3" to fit "get ready to fight".
const GET_READY_LEAD_SECONDS: f64 = 1.5;

const COUNT: [Option<&str>; 4] = [
    None,
    Some("sounds/announcer/countdown/1_0"),
    Some("sounds/announcer/countdown/2_0"),
    Some("sounds/announcer/countdown/3_0"),
];
const GET_READY: &str = "sounds/announcer/countdown/get_ready_to_fight0";
const FIGHT: &str = "sounds/announcer/countdown/fight0";
const GAME_OVER: &str = "sounds/announcer/postmatch/game_over0";
const OVERTIME: [&str; 2] = [
    "sounds/announcer/overtime/overtime.ogg",
    "sounds/announcer/overtime/overtime02.ogg",
];
const READY_UP: &str = "sounds/announcer/pleasereadyup.ogg";
const TIMEOUT: &str = "sounds/announcer/timeout/timeout0";
const TIMEIN: &str = "sounds/announcer/timeout/timein0";

/// Where a score stands against the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lead {
    Alone,
    Tied,
    Behind,
}

/// `LEAD`: each standing's line and reads, own then the team's.
fn lead_line(lead: Lead, team: bool) -> (&'static str, usize) {
    match (lead, team) {
        (Lead::Alone, _) => ("sounds/announcer/score/taken_lead0", 2),
        (Lead::Tied, false) => ("sounds/announcer/score/tied_lead0", 2),
        (Lead::Tied, true) => ("sounds/announcer/score/team_tied_lead0", 3),
        (Lead::Behind, false) => ("sounds/announcer/score/lost_lead0", 3),
        (Lead::Behind, true) => ("sounds/announcer/score/team_lost_lead0", 2),
    }
}

/// `leadOf`: `None` with no one to lead.
#[must_use]
pub fn lead_of(scores: &[f64], mine: f64) -> Option<Lead> {
    if scores.len() < 2 {
        return None;
    }
    let best = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if mine < best {
        return Some(Lead::Behind);
    }
    Some(if scores.iter().filter(|&&s| s == best).count() > 1 {
        Lead::Tied
    } else {
        Lead::Alone
    })
}

fn all(stem: &str, reads: usize) -> impl Iterator<Item = String> + '_ {
    (1..=reads).map(move |i| format!("{stem}{i}.ogg"))
}

/// `FPS_ANNOUNCER_SOUND_PATHS`.
#[must_use]
pub fn fps_announcer_sound_paths() -> Vec<String> {
    let mut paths: Vec<String> = COUNT[1..]
        .iter()
        .flatten()
        .chain([GET_READY, FIGHT, GAME_OVER].iter())
        .flat_map(|stem| all(stem, 2))
        .collect();
    paths.extend(OVERTIME.map(str::to_owned));
    paths.push(READY_UP.to_owned());
    paths.extend(all(TIMEOUT, 2));
    paths.extend(all(TIMEIN, 2));
    for lead in [Lead::Alone, Lead::Tied, Lead::Behind] {
        for team in [false, true] {
            let (stem, reads) = lead_line(lead, team);
            paths.extend(all(stem, reads));
        }
    }
    paths.dedup();
    paths
}

/// `MatchMoment`: what the match flow needs to know each frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchMoment {
    pub phase: MatchPhase,
    pub time_remaining: f64,
    pub overtime: u32,
    /// Search and Destroy calls its own rounds.
    pub rounds_called_by_mode: bool,
    /// The local player is in the match, not spectating.
    pub playing: bool,
    pub ready: bool,
    pub lead: Option<Lead>,
    pub team_based: bool,
    /// A timeout holding the match, or its count back in (`Timeout`,
    /// `Timein`); `Running` otherwise.
    pub pause: MatchPauseState,
    /// Seconds until the timeout runs out or the count ends.
    pub pause_seconds_left: f64,
}

pub struct FpsAnnouncer {
    waiting: VecDeque<String>,
    held_until: f64,
    last_phase: Option<MatchPhase>,
    last_count_second: Option<i64>,
    last_overtime: u32,
    warmup_seconds: f64,
    last_lead: Option<Lead>,
    last_pause: MatchPauseState,
    last_pause_second: Option<i64>,
    /// Every line said, for tests.
    #[cfg(test)]
    said: Vec<String>,
    random: Box<dyn FnMut() -> f64 + Send>,
}

impl Default for FpsAnnouncer {
    fn default() -> Self {
        Self {
            waiting: VecDeque::new(),
            held_until: 0.0,
            last_phase: None,
            last_count_second: None,
            last_overtime: 0,
            warmup_seconds: 0.0,
            last_lead: None,
            last_pause: MatchPauseState::Running,
            last_pause_second: None,
            #[cfg(test)]
            said: Vec::new(),
            random: Box::new(mulberry32(0x0A11_0CE5)),
        }
    }
}

impl FpsAnnouncer {
    /// One of a line's reads, `01` up to `0<reads>`.
    fn read(&mut self, stem: &str, reads: usize) -> String {
        format!("{stem}{}.ogg", 1 + ((self.random)() * reads as f64) as usize)
    }

    /// `say`: speaks now, at `now` ms.
    pub fn say(&mut self, audio: &mut AudioV2Sfx, path: &str, now: f64) {
        audio.play_2d(
            path,
            &SfxPlayOptions {
                volume: Some(1.0),
                max_instances: Some(2),
                channel: SfxChannel::Announcer,
                ..SfxPlayOptions::default()
            },
        );
        self.held_until = now + VOICE_HOLD_MS;
        #[cfg(test)]
        self.said.push(path.to_owned());
    }

    /// `queue`: speaks when the voice is free.
    pub fn queue(&mut self, audio: &mut AudioV2Sfx, path: String, now: f64) {
        self.waiting.push_back(path);
        self.release(audio, now);
    }

    /// `clear`: forgets everything, as on leaving a match.
    pub fn clear(&mut self) {
        self.waiting.clear();
        self.held_until = 0.0;
        self.last_phase = None;
        self.last_count_second = None;
        self.last_overtime = 0;
        self.warmup_seconds = 0.0;
        self.last_lead = None;
        self.last_pause = MatchPauseState::Running;
        self.last_pause_second = None;
    }

    /// `update`, every frame: calls the match and releases queued lines.
    /// `dt` in seconds, `now` in ms.
    pub fn update(&mut self, audio: &mut AudioV2Sfx, moment: &MatchMoment, dt: f64, now: f64) {
        if !self.call_pause(audio, moment, now) {
            self.call_match(audio, moment, dt, now);
        }
        self.release(audio, now);
    }

    /// `callPause`: true while a pause holds the match, and with it every
    /// other call.
    fn call_pause(&mut self, audio: &mut AudioV2Sfx, moment: &MatchMoment, now: f64) -> bool {
        let previous = self.last_pause;
        self.last_pause = moment.pause;
        match moment.pause {
            MatchPauseState::Timeout => {
                if previous != MatchPauseState::Timeout {
                    let line = self.read(TIMEOUT, 2);
                    self.say(audio, &line, now);
                }
                true
            }
            MatchPauseState::Timein => {
                let second = moment.pause_seconds_left.ceil() as i64;
                if previous != MatchPauseState::Timein {
                    let line = self.read(TIMEIN, 2);
                    self.say(audio, &line, now);
                } else if Some(second) != self.last_pause_second && second == 1 {
                    // "time in" fills the first two seconds of the count
                    let line = self.read(COUNT[1].unwrap_or(FIGHT), 2);
                    self.say(audio, &line, now);
                }
                self.last_pause_second = Some(second);
                true
            }
            MatchPauseState::Running => {
                self.last_pause_second = None;
                if previous == MatchPauseState::Timein {
                    let line = self.read(FIGHT, 2);
                    self.say(audio, &line, now);
                }
                false
            }
        }
    }

    fn release(&mut self, audio: &mut AudioV2Sfx, now: f64) {
        if now < self.held_until {
            return;
        }
        if let Some(path) = self.waiting.pop_front() {
            self.say(audio, &path, now);
        }
    }

    fn call_match(&mut self, audio: &mut AudioV2Sfx, moment: &MatchMoment, dt: f64, now: f64) {
        let previous = self.last_phase;
        let entered = Some(moment.phase) != previous;
        self.last_phase = Some(moment.phase);
        // Joining a match under way announces nothing that came before.
        if previous.is_none() {
            self.last_overtime = moment.overtime;
        }

        match moment.phase {
            MatchPhase::Warmup => {
                self.warmup_seconds = if entered { 0.0 } else { self.warmup_seconds + dt };
                if moment.playing && !moment.ready && self.warmup_seconds >= READY_NAG_SECONDS {
                    self.warmup_seconds = 0.0;
                    self.queue(audio, READY_UP.to_owned(), now);
                }
                return;
            }
            MatchPhase::PostGame => {
                if entered && previous.is_some() {
                    let line = self.read(GAME_OVER, 2);
                    self.queue(audio, line, now);
                }
                return;
            }
            _ => {}
        }
        if moment.rounds_called_by_mode {
            return;
        }
        if moment.phase == MatchPhase::Countdown {
            if entered {
                self.last_count_second = None;
                if moment.time_remaining >= 3.0 + GET_READY_LEAD_SECONDS {
                    let line = self.read(GET_READY, 2);
                    self.say(audio, &line, now);
                }
            }
            let second = moment.time_remaining.ceil() as i64;
            if Some(second) != self.last_count_second
                && let Some(Some(stem)) = usize::try_from(second).ok().and_then(|s| COUNT.get(s))
            {
                let line = self.read(stem, 2);
                self.say(audio, &line, now);
            }
            self.last_count_second = Some(second);
            return;
        }
        if moment.phase != MatchPhase::Active {
            self.last_lead = None;
            return;
        }
        if entered && previous == Some(MatchPhase::Countdown) {
            let line = self.read(FIGHT, 2);
            self.say(audio, &line, now);
        }
        self.call_lead(audio, moment, now);
        if moment.overtime > self.last_overtime {
            let line = OVERTIME[usize::from((self.random)() >= 0.5)].to_owned();
            self.queue(audio, line, now);
        }
        self.last_overtime = moment.overtime;
    }

    /// The first standing seen is only noted; every change after it is called.
    fn call_lead(&mut self, audio: &mut AudioV2Sfx, moment: &MatchMoment, now: f64) {
        let lead = if moment.playing { moment.lead } else { None };
        let from = self.last_lead;
        self.last_lead = lead;
        let (Some(lead), Some(from)) = (lead, from) else { return };
        if lead == from {
            return;
        }
        let (stem, reads) = lead_line(lead, moment.team_based);
        let line = self.read(stem, reads);
        self.queue(audio, line, now);
    }

    /// Lines waiting, for tests.
    #[must_use]
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment(phase: MatchPhase, time_remaining: f64) -> MatchMoment {
        MatchMoment {
            phase,
            time_remaining,
            overtime: 0,
            rounds_called_by_mode: false,
            playing: true,
            ready: false,
            lead: None,
            team_based: false,
            pause: MatchPauseState::Running,
            pause_seconds_left: 0.0,
        }
    }

    #[test]
    fn leads() {
        assert_eq!(lead_of(&[3.0], 3.0), None);
        assert_eq!(lead_of(&[3.0, 1.0], 3.0), Some(Lead::Alone));
        assert_eq!(lead_of(&[3.0, 3.0], 3.0), Some(Lead::Tied));
        assert_eq!(lead_of(&[3.0, 4.0], 3.0), Some(Lead::Behind));
        assert!(fps_announcer_sound_paths().contains(&"sounds/announcer/score/team_tied_lead03.ogg".to_owned()));
    }

    #[test]
    fn a_timeout_is_called_counted_back_in_and_fought_holding_everything_else() {
        let mut audio = AudioV2Sfx::offline(48_000);
        let mut a = FpsAnnouncer::default();
        let (dt, mut now) = (0.016, 0.0);
        let mut frames = |a: &mut FpsAnnouncer, seconds: f64, m: &dyn Fn(f64) -> MatchMoment| {
            let mut t = 0.0;
            while t < seconds {
                a.update(&mut audio, &m(t), dt, now);
                t += dt;
                now += dt * 1000.0;
            }
        };
        frames(&mut a, 1.0, &|_| moment(MatchPhase::Active, 0.0));
        a.said.clear();
        // Overtime starting under a timeout is not called until play resumes.
        let paused = |state, left| MatchMoment {
            overtime: 1,
            pause: state,
            pause_seconds_left: left,
            ..moment(MatchPhase::Active, 0.0)
        };
        frames(&mut a, 5.0, &|_| paused(MatchPauseState::Timeout, 100.0));
        frames(&mut a, 3.0, &|t| paused(MatchPauseState::Timein, 3.0 - t));
        frames(&mut a, 2.0, &|_| paused(MatchPauseState::Running, 0.0));
        let lines: Vec<&str> = a
            .said
            .iter()
            .map(|p| {
                let p = p.trim_start_matches("sounds/announcer/");
                p.trim_end_matches(".ogg").trim_end_matches(char::is_numeric)
            })
            .collect();
        assert_eq!(
            lines,
            [
                "timeout/timeout",
                "timeout/timein",
                "countdown/1_",
                "countdown/fight",
                "overtime/overtime"
            ]
        );
    }

    #[test]
    fn a_lead_change_waits_for_the_voice() {
        let mut audio = AudioV2Sfx::offline(48_000);
        let mut a = FpsAnnouncer::default();
        a.update(&mut audio, &moment(MatchPhase::Countdown, 2.5), 0.016, 0.0);
        // "3"... is not in a 2.5 s countdown; "fight" holds the voice.
        a.update(&mut audio, &moment(MatchPhase::Active, 300.0), 0.016, 2600.0);
        assert!(a.held_until > 2600.0);
        let mut lead = moment(MatchPhase::Active, 300.0);
        lead.lead = Some(Lead::Behind);
        a.update(&mut audio, &lead, 0.016, 2700.0);
        lead.lead = Some(Lead::Alone);
        a.update(&mut audio, &lead, 0.016, 2800.0);
        assert_eq!(a.waiting(), 1);
        a.update(&mut audio, &lead, 0.016, 4200.0);
        assert_eq!(a.waiting(), 0);
    }
}
