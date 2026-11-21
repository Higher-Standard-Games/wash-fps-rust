//! wash-fps `client/ProfileView.ts` and its part of `styles/Identity.css`:
//! the player's account, on the gold panel the Welcome screen uses. The key
//! that opens it from any computer first, then changing the name, playing as
//! someone else and deleting the account as screens of their own. The ranked
//! record sits on the stage beside the panel.
//!
//! The key never leaves the computer except to be copied or saved by the
//! player; the lobby only ever sees it at login. Natively the key is the file
//! the lobby panel logs in with, so "Save as file" writes a copy to the
//! Downloads folder and the browser's "Save in browser" has no counterpart.
//! While hidden the key plate blurs stand-in characters, not the key, since
//! the native renderer has no blur to hide it with. The record's match
//! history (`refreshHistory`) is left out for now.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::lobby::player_identity::{DELETED_ACCOUNT_NAME, KEY_LENGTH, format_key, normalize_key};
use shagger::lobby::types::{GametypeInfo, RatingPool};
use shagger::shared::js::{js_group_thousands, js_round};
use shagger::ui::components::lobby_twin::{
    RAIL_SKEW, TwinStyle, clamp_css, cream, gradient_polygon, hsl, hsla, paint_brand, text, violet,
};
use shagger::ui::egui_layer::egui;

use egui::{Color32, Painter, Rect, Shape, Stroke, pos2, vec2};

use crate::client::identity_fields::{Slot, SlotLook, Tone, judge_name, paint_say, show_slot, tidy_key_slot};
use crate::client::identity_frame::{IdentityFrame, action_keys, agree_box, alt_row, gap, go_key, screen_head, warn};
use crate::client::leaderboard_view::{Page, read_page};
use crate::client::new_ui_api::request_failure;
use crate::client::settings::settings_fields::{body, gold_text};
use crate::client::washoi_style::display_family;

/// `ANSWER_TIMEOUT_MS`.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a key says what it did (`said`).
const SAID_FOR: Duration = Duration::from_millis(1600);
const NAME_RULES: &str = "2–16 characters: letters, numbers, _ and -";

/// `KEY_FILE_LINES`.
fn key_file_lines(name: &str, key: &str) -> String {
    [
        "Washiki Washoi player key".to_owned(),
        String::new(),
        format!("Player: {name}"),
        format!("Key:    {key}"),
        String::new(),
        "To play as this player in another browser, open the game, choose \"Use my key\"".to_owned(),
        "and paste the key. Anyone with this key can play as you. Keep it private.".to_owned(),
        String::new(),
    ]
    .join("\n")
}

/// `renameFailureText` (`RENAME_FAILURES`).
#[must_use]
pub fn rename_failure_text(code: &str) -> String {
    match code {
        "name_invalid" => "Names are 2–16 letters, digits, _ or -.".into(),
        "name_reserved" => "That name is kept for bots and new players.".into(),
        "name_offensive" => "That name isn't allowed. Pick another.".into(),
        "name_taken" => "Someone already has that name.".into(),
        "in_lobby" => "Leave the room first, then rename.".into(),
        "rate_limited" => "Too many tries. Try again in a minute.".into(),
        "not_confirmed" => "Tick the box first.".into(),
        "wrong_key" => "That isn't this player's key. Paste the whole key again.".into(),
        "timeout" => "The lobby didn't answer. Try again.".into(),
        "not_connected" => "Not connected to the lobby.".into(),
        other => format!("Rename failed: {other}"),
    }
}

/// `ProfileScreen`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Key,
    Name,
    Switch,
    Delete,
}

impl Screen {
    #[must_use]
    pub fn from_id(id: &str) -> Self {
        match id {
            "name" => Self::Name,
            "switch" => Self::Switch,
            "delete" => Self::Delete,
            _ => Self::Key,
        }
    }
}

/// The account as the lobby panel has it (`currentIdentity`, `storedPlayerKey`).
#[derive(Debug, Clone, Copy)]
pub struct Account<'a> {
    pub name: &'a str,
    pub key: Option<&'a str>,
    /// The key is not on disk (`playerKeyIsSessionOnly`).
    pub key_unsaved: bool,
    pub id: Option<&'a str>,
    pub connected: bool,
    pub in_room: bool,
    pub modes: &'a [GametypeInfo],
}

impl Account<'_> {
    fn offline(&self) -> bool {
        !self.connected || self.id.is_none()
    }
}

/// What the page asks of the lobby panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileAction {
    /// `switchFpsAccount`: log in with this key on a new connection.
    Switch(String),
}

/// `StatsSummary`, as `getPlayerStats` lets it through.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub matches: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub stats_matches: u64,
    pub excluded: u64,
    /// Value, rank, provisional.
    pub rating: Option<(i64, u64, bool)>,
    pub totals: Option<Totals>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Totals {
    pub frags: f64,
    pub deaths: f64,
    pub damage_given: f64,
    pub active_time_ms: f64,
    pub plants: f64,
    pub defuses: f64,
}

/// `getPlayerStats`'s checks on a `stats:summary` for `gametype`, Overall,
/// all time.
pub fn read_stats(message: &Value, gametype: &str) -> Result<Summary, String> {
    const WRONG: &str = "Could not read your player stats.";
    let data = message.get("data").ok_or(WRONG)?;
    let s = |k: &str| data.get(k).and_then(Value::as_str);
    if s("scope") != Some("overall") || s("gametype") != Some(gametype) || s("period") != Some("all") {
        return Err(WRONG.into());
    }
    let n = |k: &str| data.get(k).and_then(Value::as_u64).ok_or(WRONG);
    let (matches, wins, losses, draws) = (n("matches")?, n("wins")?, n("losses")?, n("draws")?);
    let (stats_matches, excluded) = (n("statsMatches")?, n("excluded")?);
    let totals = match data.get("totals") {
        None | Some(Value::Null) => None,
        Some(t) => {
            let f = |k: &str| t.get(k).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or(WRONG);
            Some(Totals {
                frags: f("frags")?,
                deaths: f("deaths")?,
                damage_given: f("damageGiven")?,
                active_time_ms: f("activeTimeMs")?,
                plants: f("plants").unwrap_or(0.0),
                defuses: f("defuses").unwrap_or(0.0),
            })
        }
    };
    if matches != wins + losses + draws || stats_matches > matches || (stats_matches == 0) != totals.is_none() {
        return Err(WRONG.into());
    }
    let rating = match data.get("rating") {
        None | Some(Value::Null) => None,
        Some(r) => {
            let value = r.get("value").and_then(Value::as_i64).ok_or(WRONG)?;
            let rank = r.get("rank").and_then(Value::as_u64).filter(|&v| v >= 1).ok_or(WRONG)?;
            r.get("ratedMatches")
                .and_then(Value::as_u64)
                .filter(|&v| v >= 1)
                .ok_or(WRONG)?;
            let provisional = r.get("provisional").and_then(Value::as_bool).ok_or(WRONG)?;
            Some((value, rank, provisional))
        }
    };
    Ok(Summary {
        matches,
        wins,
        losses,
        draws,
        stats_matches,
        excluded,
        rating,
        totals,
    })
}

fn plural(n: u64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `playTime`: active time as hours and minutes, "2h 14m", "54m".
#[must_use]
pub fn play_time(ms: f64) -> String {
    let minutes = js_round(ms / 60000.0) as i64;
    let hours = minutes / 60;
    if hours > 0 {
        format!("{hours}h {}m", minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

/// A mode's gameplay totals as big numbers with their words, and the note
/// under them: why there are none, or how many matches they cover.
#[must_use]
pub fn stats_cells(summary: &Summary, snd: bool) -> (Vec<(String, String)>, String) {
    let Some(t) = &summary.totals else {
        let note = if summary.matches > 0 {
            "Gameplay totals unavailable for these results."
        } else {
            "No completed matches counted yet."
        };
        return (Vec::new(), note.into());
    };
    let mut stats = vec![
        (js_group_thousands(t.frags), "frags".to_owned()),
        (js_group_thousands(t.deaths), "deaths".to_owned()),
        (js_group_thousands(js_round(t.damage_given)), "damage".to_owned()),
        (play_time(t.active_time_ms), "played".to_owned()),
    ];
    if snd {
        stats.push((js_group_thousands(t.plants), "plants".to_owned()));
        stats.push((js_group_thousands(t.defuses), "defuses".to_owned()));
    }
    let note = if summary.stats_matches < summary.matches {
        format!("Totals from {} of {} matches.", summary.stats_matches, summary.matches)
    } else {
        String::new()
    };
    (stats, note)
}

/// `RecordCardFacts`: one mode's record.
#[derive(Debug, Clone, PartialEq, Default)]
struct RecordCard {
    mode_id: String,
    mode: String,
    rank: Option<u64>,
    /// Value and provisional.
    rating: Option<(i64, bool)>,
    wins: u64,
    losses: u64,
    draws: u64,
    stats: Vec<(String, String)>,
    note: String,
}

impl RecordCard {
    /// `profile-card__rating`'s words after the number.
    fn rating_text(&self) -> String {
        match self.rating {
            None => "Not rated".into(),
            Some((_, provisional)) => format!(" rating{}", if provisional { " · provisional" } else { "" }),
        }
    }

    /// `profile-card__counts`.
    fn counts(&self) -> String {
        let mut out = format!(
            "{} · {}",
            plural(self.wins, "win", "wins"),
            plural(self.losses, "loss", "losses")
        );
        if self.draws > 0 {
            out += &format!(" · {}", plural(self.draws, "draw", "draws"));
        }
        out
    }
}

/// The record's line over the cards: a sentence, or the totals on plates
/// with a note after them.
#[derive(Debug, Clone, PartialEq, Default)]
struct RecordStatus {
    text: String,
    totals: Vec<(String, String)>,
    note: String,
}

impl RecordStatus {
    fn text(words: &str) -> Self {
        Self {
            text: words.to_owned(),
            ..Self::default()
        }
    }

    /// `recordTotals`.
    fn totals(parts: Vec<(String, String)>, note: String) -> Self {
        Self {
            text: String::new(),
            totals: parts,
            note,
        }
    }

    /// What it says, as one sentence (the browser's `aria-label`).
    #[cfg(test)]
    fn sentence(&self) -> String {
        if self.totals.is_empty() {
            return self.text.clone();
        }
        let mut out = self
            .totals
            .iter()
            .map(|(v, w)| format!("{v} {w}"))
            .collect::<Vec<_>>()
            .join(" · ");
        if !self.note.is_empty() {
            out += &format!(" · {}", self.note);
        }
        out
    }
}

#[derive(Debug, Clone)]
enum Answer {
    Stats(Result<Summary, String>),
    Board(Result<Page, String>),
}

/// The record's requests in flight: one per mode.
#[derive(Debug)]
struct Asking {
    /// Id and name.
    modes: Vec<(String, String)>,
    requests: Vec<String>,
    answers: Vec<Option<Answer>>,
    sent: Instant,
    stats: bool,
}

/// `ProfileView`.
#[derive(Debug)]
pub struct ProfileView {
    frame: IdentityFrame,
    screen: Screen,
    /// The open screen's height last frame, to centre it.
    content_h: f32,
    revealed: bool,
    /// `keyAccountId`: a new account hides its key again.
    key_account: Option<String>,
    /// An action key saying what it did, and since when.
    said: Option<(usize, &'static str, Instant)>,
    /// Where the key's file went.
    saved_to: Option<String>,
    name_slot: Slot,
    switch_slot: Slot,
    delete_slot: Slot,
    agree: bool,
    renaming: Option<Instant>,
    deleting: Option<Instant>,
    /// The account is gone: its line stays until the new player arrives.
    deleted: bool,
    switching: bool,
    /// Focus this slot next frame.
    focus: Option<&'static str>,
    /// The lobby offers `playerStats`, and ranks (`leaderboard`, `skillRatings`).
    stats: bool,
    ranks: bool,
    record_status: RecordStatus,
    record_rows: Vec<RecordCard>,
    asking: Option<Asking>,
}

impl Default for ProfileView {
    fn default() -> Self {
        Self::new()
    }
}

impl ProfileView {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frame: IdentityFrame::default(),
            screen: Screen::Key,
            content_h: 0.0,
            revealed: false,
            key_account: None,
            said: None,
            saved_to: None,
            name_slot: Slot::new(NAME_RULES),
            switch_slot: Slot::new(""),
            delete_slot: Slot::new("Paste this player's whole key to confirm."),
            agree: false,
            renaming: None,
            deleting: None,
            deleted: false,
            switching: false,
            focus: None,
            stats: false,
            ranks: false,
            record_status: RecordStatus::default(),
            record_rows: Vec::new(),
            asking: None,
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.frame.is_open()
    }

    /// Open and not closing: the Profile key shows as the current page.
    #[must_use]
    pub fn showing(&self) -> bool {
        self.frame.is_open() && !self.frame.closing()
    }

    /// Open and still: the lobby beneath needn't be drawn.
    #[must_use]
    pub fn covers(&self) -> bool {
        self.frame.covers()
    }

    /// What the lobby offers, from its welcome (`bindPlayerStats`,
    /// `bindLeaderboard`), or nothing when the connection drops. A request
    /// waiting on the old connection fails as `not_connected`.
    pub fn bind(&mut self, stats: bool, ranks: bool, account: &Account<'_>, client: &mut LobbyClient) {
        self.stats = stats;
        self.ranks = ranks;
        self.asking = None;
        if !account.connected {
            for (pending, slot) in [
                (&mut self.renaming, &mut self.name_slot),
                (&mut self.deleting, &mut self.delete_slot),
            ] {
                if pending.take().is_some() {
                    slot.tell(rename_failure_text("not_connected"), Tone::Bad);
                }
            }
        }
        if self.is_open() {
            self.refresh(account, client);
        }
    }

    /// `open`: widen out of the rail, at `screen`.
    pub fn open(&mut self, screen: Screen, account: &Account<'_>, client: &mut LobbyClient) {
        self.revealed = false;
        self.deleted = false;
        if !self.showing() {
            self.frame.open();
        }
        self.refresh(account, client);
        self.show(screen, account);
    }

    /// `close`: narrow back onto the rail.
    pub fn close(&mut self) {
        self.revealed = false;
        self.frame.close();
    }

    /// Esc steps back to the key, and from the key back to the lobby.
    pub fn back(&mut self, account: &Account<'_>) {
        if self.screen == Screen::Key {
            self.close();
        } else {
            self.show(Screen::Key, account);
        }
    }

    fn show(&mut self, screen: Screen, account: &Account<'_>) {
        self.screen = screen;
        self.content_h = 0.0;
        match screen {
            Screen::Key => self.focus = None,
            Screen::Name => {
                account.name.clone_into(&mut self.name_slot.value);
                self.refresh_name_state(account);
                self.focus = Some("profile-name");
            }
            Screen::Switch => {
                self.refresh_switch_state(account);
                self.focus = Some("profile-key");
            }
            Screen::Delete => {
                self.agree = false;
                self.delete_slot.value.clear();
                self.delete_slot
                    .tell("Paste this player's whole key to confirm.", Tone::Plain);
                self.refresh_delete_state(account);
            }
        }
    }

    /// `refresh`: show the account as it stands, and ask for its record.
    pub fn refresh(&mut self, account: &Account<'_>, client: &mut LobbyClient) {
        let id = account.id.map(str::to_owned);
        if id != self.key_account {
            self.revealed = false;
            self.key_account = id;
        }
        if !self.name_slot_focused() && self.renaming.is_none() {
            account.name.clone_into(&mut self.name_slot.value);
        }
        if self.renaming.is_none() {
            self.refresh_name_state(account);
        }
        if !self.switching {
            self.refresh_switch_state(account);
        }
        if self.deleting.is_none() {
            self.refresh_delete_state(account);
        }
        self.refresh_record(account, client);
    }

    /// The name is being typed: an update from elsewhere leaves it be.
    fn name_slot_focused(&self) -> bool {
        self.frame.is_open() && self.screen == Screen::Name
    }

    /// `refreshNameState`.
    fn refresh_name_state(&mut self, account: &Account<'_>) {
        let typed = self.name_slot.value.trim().to_owned();
        let (_, text, tone) = judge_name(&typed);
        if account.in_room {
            self.name_slot.tell("Leave your room to change your name.", Tone::Plain);
        } else if account.offline() {
            self.name_slot
                .tell("Connect to the lobby to change your name.", Tone::Plain);
        } else if typed == account.name {
            self.name_slot.tell("This is your name.", Tone::Plain);
        } else {
            self.name_slot.tell(text, tone);
        }
    }

    fn rename_ready(&self, account: &Account<'_>) -> bool {
        let typed = self.name_slot.value.trim();
        !account.in_room
            && !account.offline()
            && self.renaming.is_none()
            && judge_name(typed).0
            && typed != account.name
    }

    /// `refreshSwitchState`.
    fn refresh_switch_state(&mut self, account: &Account<'_>) {
        if self.switching {
            return;
        }
        if account.in_room {
            self.switch_slot
                .tell("Leave your room before switching player.", Tone::Plain);
        } else if !account.connected {
            self.switch_slot.tell("The lobby is offline.", Tone::Plain);
        } else {
            self.switch_slot.tell(
                format!("{KEY_LENGTH} characters. Dashes and spaces don't matter."),
                Tone::Plain,
            );
        }
    }

    fn switch_ready(&self, account: &Account<'_>) -> bool {
        !self.switching && !account.in_room && account.connected && normalize_key(&self.switch_slot.value).is_some()
    }

    /// `deleteKeyMatches`: the pasted key is whole and, when this computer
    /// has the key, the same one.
    fn delete_key_matches(&self, account: &Account<'_>) -> bool {
        let pasted = normalize_key(&self.delete_slot.value);
        pasted.is_some() && account.key.is_none_or(|stored| normalize_key(stored) == pasted)
    }

    /// `refreshDeleteState`.
    fn refresh_delete_state(&mut self, account: &Account<'_>) {
        if self.deleting.is_some() || self.deleted {
            return;
        }
        let pasted = normalize_key(&self.delete_slot.value);
        if account.in_room {
            self.delete_slot
                .tell("Leave your room before deleting your account.", Tone::Plain);
        } else if account.offline() {
            self.delete_slot
                .tell("Connect to the lobby to delete your account.", Tone::Plain);
        } else if pasted.is_some() && !self.delete_key_matches(account) {
            self.delete_slot.tell("That isn't this player's key.", Tone::Bad);
        } else if pasted.is_some() && !self.agree {
            self.delete_slot.tell("Tick the box above to go on.", Tone::Plain);
        }
    }

    fn delete_ready(&self, account: &Account<'_>) -> bool {
        self.deleting.is_none()
            && !account.in_room
            && !account.offline()
            && self.agree
            && self.delete_key_matches(account)
    }

    /// `refreshRecord`: the account's standing, one line per mode it has a
    /// result in, from the lobby's own stats or, without them, its boards.
    fn refresh_record(&mut self, account: &Account<'_>, client: &mut LobbyClient) {
        if self.stats && account.connected {
            let modes: Vec<(String, String)> = account
                .modes
                .iter()
                .filter(|m| ["duel", "amphi", "ffa", "snd"].contains(&m.id.as_str()))
                .map(|m| (m.id.clone(), m.name.clone()))
                .collect();
            let requests = modes
                .iter()
                .map(|(id, _)| client.request_stats(id, "all", Some(RatingPool::Overall)))
                .collect();
            self.asking = Some(Asking {
                answers: vec![None; modes.len()],
                modes,
                requests,
                sent: Instant::now(),
                stats: true,
            });
            self.finish_record();
            return;
        }
        self.asking = None;
        let refuse = |view: &mut Self, why: &str| {
            view.record_rows.clear();
            view.record_status = RecordStatus::text(why);
        };
        if !account.connected {
            return refuse(self, "Connect to the lobby to see your record.");
        }
        if !self.ranks {
            return refuse(self, "Rankings are unavailable on this lobby server.");
        }
        if account.modes.is_empty() {
            return refuse(self, "This lobby is not advertising any mode.");
        }
        if self.record_rows.is_empty() {
            self.record_status = RecordStatus::text("Reading the rankings…");
        }
        let modes: Vec<(String, String)> = account.modes.iter().map(|m| (m.id.clone(), m.name.clone())).collect();
        let requests = modes
            .iter()
            .map(|(id, _)| client.request_leaderboard(id, "all", 0, 1, None))
            .collect();
        self.asking = Some(Asking {
            answers: vec![None; modes.len()],
            modes,
            requests,
            sent: Instant::now(),
            stats: false,
        });
    }

    /// Builds the record once every mode has answered.
    fn finish_record(&mut self) {
        let Some(asking) = &self.asking else { return };
        if asking.answers.iter().any(Option::is_none) {
            return;
        }
        let asking = self.asking.take().expect("asking");
        if asking.stats {
            self.stats_record(&asking);
        } else {
            self.board_record(&asking);
        }
    }

    fn stats_record(&mut self, asking: &Asking) {
        self.record_rows.clear();
        let (mut matches, mut failure) = (0, String::new());
        for ((id, name), answer) in asking.modes.iter().zip(&asking.answers) {
            let Some(Answer::Stats(answer)) = answer else { continue };
            let summary = match answer {
                Ok(s) => s,
                Err(why) => {
                    if failure.is_empty() {
                        failure.clone_from(why);
                    }
                    continue;
                }
            };
            matches += summary.matches;
            if summary.matches == 0 && summary.excluded == 0 && summary.rating.is_none() {
                continue;
            }
            let (stats, note) = stats_cells(summary, id == "snd");
            self.record_rows.push(RecordCard {
                mode_id: id.clone(),
                mode: name.clone(),
                rank: summary.rating.map(|(_, rank, _)| rank),
                rating: summary.rating.map(|(value, _, p)| (value, p)),
                wins: summary.wins,
                losses: summary.losses,
                draws: summary.draws,
                stats,
                note,
            });
        }
        let word = if matches == 1 { "match" } else { "matches" };
        self.record_status = RecordStatus::totals(vec![(matches.to_string(), word.to_owned())], failure);
    }

    fn board_record(&mut self, asking: &Asking) {
        let mut standings = Vec::new();
        let mut refusal = String::new();
        for ((id, name), answer) in asking.modes.iter().zip(&asking.answers) {
            match answer {
                Some(Answer::Board(Ok(page))) => {
                    if let Some(row) = &page.local_player_entry {
                        standings.push((id.clone(), name.clone(), row.clone()));
                    }
                }
                Some(Answer::Board(Err(why))) if refusal.is_empty() => refusal.clone_from(why),
                _ => {}
            }
        }
        self.record_rows.clear();
        if standings.is_empty() {
            self.record_status = RecordStatus::text(if refusal.is_empty() {
                "No ranked match yet. Finish an eligible match to get a provisional rating."
            } else {
                &refusal
            });
            return;
        }
        let played: u32 = standings.iter().map(|(_, _, r)| r.wins + r.losses + r.draws).sum();
        let best = standings
            .iter()
            .reduce(|top, one| if one.2.rank < top.2.rank { one } else { top })
            .expect("a standing");
        self.record_status = RecordStatus::totals(
            vec![
                (
                    played.to_string(),
                    if played == 1 { "match" } else { "matches" }.to_owned(),
                ),
                (format!("#{}", best.2.rank), format!("in {}", best.1)),
            ],
            String::new(),
        );
        standings.sort_by_key(|(_, _, r)| r.rank);
        for (id, name, row) in standings {
            self.record_rows.push(RecordCard {
                mode_id: id,
                mode: name,
                rank: Some(u64::from(row.rank)),
                rating: Some((row.rating, row.provisional)),
                wins: u64::from(row.wins),
                losses: u64::from(row.losses),
                draws: u64::from(row.draws),
                stats: Vec::new(),
                note: String::new(),
            });
        }
    }

    /// The lobby's answers: names, deletion, stats and boards.
    pub fn on_message(&mut self, kind: &str, message: &Value, account: &Account<'_>) {
        let request = message.get("requestId").and_then(Value::as_str);
        match kind {
            "identity:renamed" => {
                if self.renaming.take().is_some() {
                    let name = message
                        .pointer("/identity/name")
                        .and_then(Value::as_str)
                        .unwrap_or(account.name);
                    name.clone_into(&mut self.name_slot.value);
                    self.name_slot.tell(format!("You're {name} now"), Tone::Ok);
                } else if !self.name_slot_focused() {
                    // Another window renamed the account.
                    if let Some(name) = message.pointer("/identity/name").and_then(Value::as_str) {
                        name.clone_into(&mut self.name_slot.value);
                    }
                }
            }
            "identity:error" => {
                let code = message.get("code").and_then(Value::as_str).unwrap_or("error");
                if self.renaming.take().is_some() {
                    self.name_slot.tell(rename_failure_text(code), Tone::Bad);
                } else if self.deleting.take().is_some() {
                    self.refresh_delete_state(account);
                    self.delete_slot.tell(rename_failure_text(code), Tone::Bad);
                }
            }
            "identity:deleted" if self.deleting.take().is_some() => {
                self.deleted = true;
                self.delete_slot.value.clear();
                self.agree = false;
                self.delete_slot
                    .tell("Your account is deleted. Starting over as a new player…", Tone::Ok);
            }
            "stats:summary" | "stats:error" | "leaderboard:page" | "leaderboard:error" => {
                let Some(asking) = &mut self.asking else { return };
                let Some(at) = request.and_then(|r| asking.requests.iter().position(|q| q == r)) else {
                    return;
                };
                asking.answers[at] = Some(match kind {
                    "stats:summary" => Answer::Stats(read_stats(message, &asking.modes[at].0)),
                    "stats:error" => Answer::Stats(Err("Could not read your player stats.".into())),
                    "leaderboard:page" => Answer::Board(read_page(message)),
                    _ => Answer::Board(Err(request_failure(
                        &format!(
                            "Leaderboard: {}",
                            message.get("reason").and_then(Value::as_str).unwrap_or("error")
                        ),
                        "the rankings",
                    ))),
                });
                self.finish_record();
            }
            _ => {}
        }
    }

    /// The new connection's answer to a switch: the player's name, or why not.
    pub fn switch_answered(&mut self, answer: Result<String, String>, account: &Account<'_>) {
        self.switching = false;
        self.refresh_switch_state(account);
        match answer {
            Ok(name) => {
                self.switch_slot.value.clear();
                self.switch_slot.tell(format!("Welcome back, {name}"), Tone::Ok);
            }
            Err(why) => self.switch_slot.tell(why, Tone::Bad),
        }
    }

    /// `rename`: ask the lobby for the typed name.
    pub fn rename(&mut self, account: &Account<'_>, client: &mut LobbyClient) {
        if !self.rename_ready(account) {
            return;
        }
        let name = self.name_slot.value.trim().to_owned();
        self.name_slot.tell("Asking the lobby…", Tone::Plain);
        self.renaming = Some(Instant::now());
        client.rename(&name);
    }

    /// `deleteThisAccount`: ticked, and the whole key pasted again.
    pub fn delete(&mut self, account: &Account<'_>, client: &mut LobbyClient) {
        if !self.delete_ready(account) {
            return;
        }
        self.deleting = Some(Instant::now());
        self.delete_slot.tell("Asking the lobby…", Tone::Plain);
        client.delete_account(&self.delete_slot.value, self.agree);
    }

    /// `switchAccount`: the pasted key, for the panel to log in with.
    pub fn switch(&mut self, account: &Account<'_>) -> Option<ProfileAction> {
        if !self.switch_ready(account) {
            return None;
        }
        self.switching = true;
        self.switch_slot.tell("Asking the lobby…", Tone::Plain);
        Some(ProfileAction::Switch(self.switch_slot.value.clone()))
    }

    /// For screenshots: the record as if the lobby had sent these summaries
    /// (id, name and summary per mode).
    pub fn seed_stats(&mut self, modes: Vec<(String, String, Summary)>) {
        let asking = Asking {
            requests: Vec::new(),
            sent: Instant::now(),
            stats: true,
            answers: modes
                .iter()
                .map(|(_, _, s)| Some(Answer::Stats(Ok(s.clone()))))
                .collect(),
            modes: modes.into_iter().map(|(id, name, _)| (id, name)).collect(),
        };
        self.asking = None;
        self.stats_record(&asking);
    }

    /// Types into a slot, as the player would: for tests and tools.
    pub fn type_into(&mut self, screen: Screen, text: &str, account: &Account<'_>) {
        match screen {
            Screen::Name => {
                text.clone_into(&mut self.name_slot.value);
                self.refresh_name_state(account);
            }
            Screen::Switch => {
                text.clone_into(&mut self.switch_slot.value);
                tidy_key_slot(&mut self.switch_slot);
            }
            Screen::Delete => {
                self.agree = true;
                text.clone_into(&mut self.delete_slot.value);
                tidy_key_slot(&mut self.delete_slot);
                self.refresh_delete_state(account);
            }
            Screen::Key => {}
        }
    }

    /// The status line of a screen's slot.
    #[must_use]
    pub fn said(&self, screen: Screen) -> (&str, Tone) {
        let slot = match screen {
            Screen::Name | Screen::Key => &self.name_slot,
            Screen::Switch => &self.switch_slot,
            Screen::Delete => &self.delete_slot,
        };
        (&slot.say, slot.tone)
    }

    /// Answers that never came.
    pub fn tick(&mut self) {
        let late = |at: &mut Option<Instant>| at.is_some_and(|t| t.elapsed() > ANSWER_TIMEOUT) && at.take().is_some();
        if late(&mut self.renaming) {
            self.name_slot.tell(rename_failure_text("timeout"), Tone::Bad);
        }
        if late(&mut self.deleting) {
            self.delete_slot.tell(rename_failure_text("timeout"), Tone::Bad);
        }
        if let Some(asking) = &mut self.asking
            && asking.sent.elapsed() > ANSWER_TIMEOUT
        {
            let stats = asking.stats;
            for answer in asking.answers.iter_mut().filter(|a| a.is_none()) {
                *answer = Some(if stats {
                    Answer::Stats(Err("Could not read your player stats.".into()))
                } else {
                    Answer::Board(Err(request_failure("Leaderboard request timed out", "the rankings")))
                });
            }
            self.finish_record();
        }
    }

    /// Draws the page over `screen`, while it is open.
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        time: f64,
        accent: Color32,
        style: &TwinStyle,
        account: &Account<'_>,
        client: &mut LobbyClient,
    ) -> Option<ProfileAction> {
        if self.frame.finished() || !self.frame.is_open() {
            return None;
        }
        if self.showing() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.back(account);
        }
        let mut action = None;
        ui.scope(|ui| {
            // The lobby beneath takes no pointer while the frame is over it.
            let _ = ui.interact(screen, egui::Id::NULL.with("profile-block"), egui::Sense::click());
            let rows = &self.record_rows;
            let status = &self.record_status;
            let layout = self
                .frame
                .paint(ui, screen, time, account.name, true, accent, |ui, stage| {
                    paint_record(ui, stage, status, rows);
                });
            ui.set_opacity(ui.opacity() * layout.inner_alpha);
            // The panel's screens scroll when taller than the window.
            let scroll = Rect::from_min_max(
                layout.panel.min,
                pos2(layout.panel.right() - RAIL_SKEW - 8.0, layout.panel.bottom()),
            );
            let pad = layout.column.top() - layout.panel.top();
            let total = (self.content_h + 2.0 * pad).max(scroll.height());
            ui.scope_builder(egui::UiBuilder::new().max_rect(scroll), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("profile", self.screen as u8))
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let (r, _) = ui.allocate_exact_size(vec2(scroll.width(), total), egui::Sense::hover());
                        // `justify-content: safe center`.
                        let top = r.top() + pad + ((scroll.height() - 2.0 * pad - self.content_h) / 2.0).max(0.0);
                        let left = layout.column.left();
                        let width = layout.column.width();
                        let bottom =
                            self.screen_ui(ui, left, top, width, layout.short, style, account, client, &mut action);
                        self.content_h = bottom - top;
                    });
            });
        });
        if let Some(id) = self.focus.take()
            && self.frame.covers()
        {
            ui.ctx()
                .memory_mut(|m| m.request_focus(egui::Id::NULL.with(("identity-slot", id))));
        } else if let Some(id) = self.focus.take() {
            self.focus = Some(id);
        }
        action
    }

    /// The logo and the open screen, from `top`; returns the bottom.
    #[allow(clippy::too_many_arguments)]
    fn screen_ui(
        &mut self,
        ui: &mut egui::Ui,
        left: f32,
        top: f32,
        width: f32,
        short: bool,
        style: &TwinStyle,
        account: &Account<'_>,
        client: &mut LobbyClient,
        action: &mut Option<ProfileAction>,
    ) -> f32 {
        let painter = ui.painter().clone();
        let screen_w = ui.ctx().content_rect().width();
        // `welcome__logo`: up to 460 wide (300 short), 8 into the margin.
        let logo_w = width.min(if short { 300.0 } else { 460.0 });
        let brand_bottom = paint_brand(
            &painter,
            left - 8.0 - 22.0,
            logo_w + RAIL_SKEW + 38.0,
            top,
            None,
            screen_w,
            style,
            1.0,
        );
        let mut y = if style.logo.is_some() {
            brand_bottom - 16.0 + if short { 4.0 } else { 18.0 }
        } else {
            brand_bottom
        };
        let g = gap(short);
        match self.screen {
            Screen::Key => y = self.key_screen(ui, left, y, width, short, account),
            Screen::Name => {
                y = screen_head(
                    &painter,
                    left,
                    y,
                    width,
                    short,
                    "Who are you?",
                    &[("Everyone sees it: in rooms, chat and the rankings.", false)],
                ) + g;
                let enabled = !account.in_room && !account.offline();
                let look = SlotLook {
                    max_chars: Some(16),
                    counted: true,
                    ..SlotLook::default()
                };
                let r = show_slot(
                    ui,
                    "profile-name",
                    left,
                    y,
                    width,
                    short,
                    &mut self.name_slot,
                    look,
                    enabled,
                );
                if r.changed && self.renaming.is_none() {
                    self.refresh_name_state(account);
                }
                y = r.bottom + g;
                let ready = self.rename_ready(account);
                let (go, bottom) = go_key(ui, "rename", left, y, short, "Save", true, !ready);
                if go || r.submitted {
                    self.rename(account, client);
                }
                y = bottom + g;
                let (back, bottom) = alt_row(
                    ui,
                    "name-alt",
                    left,
                    y,
                    width,
                    "Keep the one you have?",
                    &["Back to your key"],
                );
                if back.is_some() {
                    self.show(Screen::Key, account);
                }
                y = bottom;
            }
            Screen::Switch => {
                y = screen_head(
                    &painter,
                    left,
                    y,
                    width,
                    short,
                    "Paste a key",
                    &[(
                        "This computer forgets its own key when it switches, so save that key first.",
                        false,
                    )],
                ) + g;
                let look = SlotLook {
                    placeholder: "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX",
                    prefix: Some("WASHOI-"),
                    ..SlotLook::default()
                };
                let r = show_slot(
                    ui,
                    "profile-key",
                    left,
                    y,
                    width,
                    short,
                    &mut self.switch_slot,
                    look,
                    true,
                );
                if r.changed {
                    tidy_key_slot(&mut self.switch_slot);
                }
                y = r.bottom + g;
                let ready = self.switch_ready(account);
                let (go, bottom) = go_key(ui, "switch", left, y, short, "Use it", true, !ready);
                if (go || r.submitted) && ready {
                    *action = self.switch(account);
                }
                y = bottom + g;
                let (back, bottom) = alt_row(
                    ui,
                    "switch-alt",
                    left,
                    y,
                    width,
                    "Staying yourself?",
                    &["Back to your key"],
                );
                if back.is_some() {
                    self.show(Screen::Key, account);
                }
                y = bottom;
            }
            Screen::Delete => {
                let lead = format!(
                    "Your name becomes {DELETED_ACCOUNT_NAME} on the leaderboards and in the demo list, your synced \
                     settings are erased, and your key stops working. Demos already recorded stay as they are."
                );
                y = screen_head(
                    &painter,
                    left,
                    y,
                    width,
                    short,
                    "Delete this player?",
                    &[(&lead, false)],
                ) + g;
                let (ticked, bottom) = agree_box(
                    ui,
                    "delete",
                    left,
                    y,
                    width,
                    self.agree,
                    "I understand this can't be undone.",
                    " Nobody can bring this player, its name or its rankings back.",
                );
                if ticked {
                    self.agree = !self.agree;
                    self.refresh_delete_state(account);
                }
                y = bottom + g;
                let look = SlotLook {
                    placeholder: "XXXX-XXXX-XXXX-XXXX-XXXX-XXXX",
                    prefix: Some("WASHOI-"),
                    ..SlotLook::default()
                };
                let r = show_slot(
                    ui,
                    "profile-delete-key",
                    left,
                    y,
                    width,
                    short,
                    &mut self.delete_slot,
                    look,
                    true,
                );
                if r.changed {
                    tidy_key_slot(&mut self.delete_slot);
                    self.refresh_delete_state(account);
                }
                y = r.bottom + g;
                let ready = self.delete_ready(account);
                let (go, bottom) = go_key(ui, "delete", left, y, short, "Delete for good", false, !ready);
                if go || r.submitted {
                    self.delete(account, client);
                }
                y = bottom + g;
                let (back, bottom) = alt_row(
                    ui,
                    "delete-alt",
                    left,
                    y,
                    width,
                    "Changed your mind?",
                    &["Back to your key"],
                );
                if back.is_some() {
                    self.show(Screen::Key, account);
                }
                y = bottom;
            }
        }
        y
    }

    /// The key screen: the plate, its keys, the warnings, Done, and the way
    /// to the other screens.
    fn key_screen(
        &mut self,
        ui: &mut egui::Ui,
        left: f32,
        top: f32,
        width: f32,
        short: bool,
        account: &Account<'_>,
    ) -> f32 {
        let painter = ui.painter().clone();
        let g = gap(short);
        let named = [
            ("Paste it on any computer to play as ", false),
            (account.name, true),
            (".", false),
        ];
        let unnamed = [("Paste it on any computer to play as this player.", false)];
        let lead: &[(&str, bool)] = if account.name.is_empty() { &unnamed } else { &named };
        let mut y = screen_head(&painter, left, top, width, short, "Your player key", lead) + g;
        if let Some(key) = account.key {
            let (reveal, bottom) = self.key_plate(ui, left, y, width, short, account.name, key);
            if reveal {
                self.revealed = !self.revealed;
            }
            y = bottom + g;
            if self.said.is_some_and(|(_, _, at)| at.elapsed() > SAID_FOR) {
                self.said = None;
            }
            let labels: Vec<&str> = ["Copy key", "Save as file"]
                .iter()
                .enumerate()
                .map(|(i, label)| match self.said {
                    Some((at, word, _)) if at == i => word,
                    _ => label,
                })
                .collect();
            let (pressed, bottom) = action_keys(ui, "key-actions", left, y, width, &labels);
            match pressed {
                Some(0) => {
                    ui.ctx().copy_text(format_key(key));
                    self.said = Some((0, "Copied", Instant::now()));
                }
                Some(1) => {
                    let word = match save_key_file(account.name, key) {
                        Ok(path) => {
                            self.saved_to = Some(format!("Saved to {}", path.display()));
                            "Saved"
                        }
                        Err(why) => {
                            self.saved_to = Some(format!("Could not save the key: {why}"));
                            "Not saved"
                        }
                    };
                    self.said = Some((1, word, Instant::now()));
                }
                _ => {}
            }
            y = bottom;
            if let Some(note) = &self.saved_to {
                y = paint_say(&painter, left, y + 4.0, width, note, Tone::Plain);
            }
            y = warn(
                &painter,
                left,
                y + g,
                width,
                "This key is you.",
                " Anyone who has it can play as you, and nobody can get it back for you if it's lost.",
            );
            if account.key_unsaved {
                y = warn(
                    &painter,
                    left,
                    y + g,
                    width,
                    "",
                    "Your key could not be saved on this computer. Copy it or save it as a file before quitting \
                     to keep this player.",
                );
            }
            y += g;
        } else {
            let g2 = text(
                &painter,
                "Your key is made the first time you reach the lobby.",
                20.0,
                body(),
                hsla(250.0, 45.0, 8.0, 0.78),
                0.0,
            );
            let h = g2.size().y;
            painter.galley(pos2(left, y), g2, Color32::WHITE);
            y += h + g;
        }
        let (done, bottom) = go_key(ui, "done", left, y, short, "Done", false, false);
        if done {
            self.close();
        }
        y = bottom + g;
        let (pressed, bottom) = alt_row(
            ui,
            "key-alt",
            left,
            y,
            width,
            "Not right?",
            &["Change name", "Use another key"],
        );
        match pressed {
            Some(0) => self.show(Screen::Name, account),
            Some(1) => self.show(Screen::Switch, account),
            _ => {}
        }
        y = bottom + g;
        let (pressed, bottom) = alt_row(
            ui,
            "key-delete",
            left,
            y,
            width,
            "Done with Washoi?",
            &["Delete account"],
        );
        if pressed.is_some() {
            self.show(Screen::Delete, account);
        }
        bottom
    }

    /// `keyplate`: the key on a dark plate, blurred until asked for. Returns
    /// whether Show / Hide was pressed and the plate's bottom.
    #[allow(clippy::too_many_arguments)]
    fn key_plate(
        &self,
        ui: &mut egui::Ui,
        left: f32,
        top: f32,
        width: f32,
        short: bool,
        name: &str,
        key: &str,
    ) -> (bool, f32) {
        let painter = ui.painter().clone();
        let screen_w = ui.ctx().content_rect().width();
        let (pt, pb, pl) = if short { (18.0, 18.0, 24.0) } else { (24.0, 24.0, 28.0) };
        let gold = gold_text();
        let who = text(
            &painter,
            "PLAYER ",
            12.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.45),
            12.0 * 0.28,
        );
        let who_name = text(&painter, &name.to_uppercase(), 12.0, display_family(), gold, 1.2);
        let prefix = text(
            &painter,
            "WASHOI-",
            16.0,
            display_family(),
            hsla(46.0, 100.0, 60.0, 0.5),
            16.0 * 0.08,
        );
        let size = clamp_css(22.0, screen_w * 0.019, 32.0);
        let groups: Vec<String> = format_key(key).split('-').skip(1).map(str::to_owned).collect();
        let group_g: Vec<_> = groups
            .iter()
            .map(|grp| text(&painter, grp, size, display_family(), gold, size * 0.1))
            .collect();
        let col_w = group_g.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let row_h = group_g.first().map_or(size, |g| g.size().y);
        let rows = groups.len().div_ceil(3).max(1) as f32;
        let groups_h = rows * row_h + (rows - 1.0) * 10.0;
        let who_h = who.size().y.max(who_name.size().y);
        let h = pt + who_h + 18.0 + groups_h + pb;
        let r = Rect::from_min_size(pos2(left, top), vec2(width, h));
        // `clip-path`: the top right corner cut 22px.
        gradient_polygon(
            &painter,
            &[
                r.left_top(),
                pos2(r.right() - 22.0, r.top()),
                pos2(r.right(), r.top() + 22.0),
                r.right_bottom(),
                r.left_bottom(),
            ],
            &[(0.0, hsl(250.0, 40.0, 6.0)), (1.0, hsl(250.0, 34.0, 12.0))],
        );
        let (x, y) = (r.left() + pl, r.top() + pt);
        let ww = who.size().x;
        painter.galley(pos2(x, y), who, Color32::WHITE);
        painter.galley(pos2(x + ww, y), who_name, Color32::WHITE);
        let gy = y + who_h + 18.0;
        let pw = prefix.size().x;
        painter.galley(
            pos2(x, gy + groups_h / 2.0 - prefix.size().y / 2.0),
            prefix,
            Color32::WHITE,
        );
        let gx = x + pw + 22.0;
        for (i, g) in group_g.into_iter().enumerate() {
            let at = pos2(
                gx + (i % 3) as f32 * (col_w + 22.0),
                gy + (i / 3) as f32 * (row_h + 10.0),
            );
            if self.revealed {
                painter.galley(at, g, Color32::WHITE);
            } else {
                // `filter: blur(9px)` over stand-ins, so nothing of the key shows.
                const STAND_INS: [&str; 6] = ["8M3K", "W7QH", "4RZP", "J2XD", "6TBN", "C9VF"];
                let stand_in = text(&painter, STAND_INS[i % 6], size, display_family(), gold, size * 0.1);
                for ring in [(4.0, 10, 0.07), (8.0, 16, 0.045), (12.0, 20, 0.03), (16.0, 24, 0.015)] {
                    for s in 0..ring.1 {
                        let a = s as f32 / ring.1 as f32 * std::f32::consts::TAU;
                        painter.galley_with_override_text_color(
                            at + vec2(a.cos(), a.sin()) * ring.0,
                            stand_in.clone(),
                            gold.gamma_multiply(ring.2),
                        );
                    }
                }
            }
        }
        // `keyplate__reveal`, its lower right.
        let label = if self.revealed { "HIDE" } else { "SHOW" };
        let lg = text(&painter, label, 11.0, display_family(), cream(), 11.0 * 0.14);
        let br = Rect::from_min_max(
            pos2(
                r.right() - 18.0 - lg.size().x - 24.0,
                r.bottom() - 14.0 - lg.size().y - 12.0,
            ),
            pos2(r.right() - 18.0, r.bottom() - 14.0),
        );
        let response = ui.interact(br, egui::Id::NULL.with("keyplate-reveal"), egui::Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let skew = br.width() * 0.08;
        painter.add(Shape::convex_polygon(
            vec![
                pos2(br.left() + skew, br.top()),
                br.right_top(),
                pos2(br.right() - skew, br.bottom()),
                br.left_bottom(),
            ],
            if response.hovered() {
                violet()
            } else {
                hsla(276.0, 90.0, 64.0, 0.22)
            },
            Stroke::NONE,
        ));
        painter.galley(br.center() - lg.size() / 2.0, lg, Color32::WHITE);
        (response.clicked(), r.bottom())
    }
}

/// `profile-card`'s `--mode-accent`.
fn mode_accent(id: &str) -> Color32 {
    match id {
        "duel" => hsl(182.0, 95.0, 52.0),
        "amphi" => hsl(100.0, 85.0, 52.0),
        "ffa" => hsl(46.0, 100.0, 57.0),
        "snd" => hsl(322.0, 95.0, 62.0),
        _ => hsl(276.0, 90.0, 64.0),
    }
}

/// `profile-card__stats`: `repeat(auto-fit, minmax(78px, 1fr))`, 10 by 12.
fn stats_columns(width: f32, count: usize) -> usize {
    (((width + 12.0) / (78.0 + 12.0)).floor() as usize).clamp(1, count.max(1))
}

const STAT_H: f32 = 22.0 + 2.0 + 17.0;

/// A card's height at `width` (its padding included).
fn card_height(painter: &Painter, card: &RecordCard, width: f32) -> f32 {
    let inner = width - 40.0 - 5.0;
    let mut h = 16.0 + 30.0 + 12.0 + 24.0 + 12.0 + 16.0;
    if !card.stats.is_empty() {
        let rows = card.stats.len().div_ceil(stats_columns(inner, card.stats.len())) as f32;
        h += 12.0 + 2.0 + 12.0 + rows * STAT_H + (rows - 1.0) * 10.0;
    }
    if !card.note.is_empty() {
        h += 12.0 + note_galley(painter, &card.note, inner).size().y;
    }
    h + 18.0
}

fn note_galley(painter: &Painter, note: &str, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        note.to_owned(),
        egui::FontId::proportional(15.0),
        hsla(45.0, 100.0, 95.0, 0.65),
        width.max(1.0),
    );
    job.wrap.break_anywhere = false;
    painter.layout_job(job)
}

/// `recordCard`: one mode's record, a dark card edged in the mode's colour.
fn paint_card(painter: &Painter, r: Rect, card: &RecordCard) {
    let accent = mode_accent(&card.mode_id);
    let cut = 18.0;
    let outline = [
        r.left_top(),
        pos2(r.right() - cut, r.top()),
        pos2(r.right(), r.top() + cut),
        r.right_bottom(),
        r.left_bottom(),
    ];
    shagger::ui::components::lobby_twin::gradient_polygon_x(
        painter,
        &outline,
        &[(0.0, hsla(250.0, 45.0, 9.0, 0.88)), (1.0, hsla(250.0, 45.0, 6.0, 0.72))],
    );
    painter.rect_filled(Rect::from_min_size(r.min, vec2(5.0, r.height())), 0.0, accent);
    let left = r.left() + 5.0 + 20.0;
    let right = r.right() - 20.0;
    let width = right - left;
    let mut y = r.top() + 16.0;
    // The head: the mode and its rank, on one baseline.
    let rank = card.rank.map(|n| {
        text(
            painter,
            &format!("#{n}"),
            30.0,
            display_family(),
            hsl(45.0, 100.0, 60.0),
            0.0,
        )
    });
    let rank_w = rank.as_ref().map_or(0.0, |g| g.size().x + 16.0);
    let name = shagger::ui::components::lobby_twin::elide(
        painter,
        &card.mode.to_uppercase(),
        24.0,
        display_family(),
        width - rank_w,
    );
    let name = text(painter, &name, 24.0, display_family(), cream(), 24.0 * 0.04);
    let base = y + 30.0;
    painter.galley(pos2(left, base - name.size().y - 1.0), name, Color32::WHITE);
    if let Some(g) = rank {
        painter.galley(pos2(right - g.size().x, base - g.size().y), g, Color32::WHITE);
    }
    y = base + 12.0;
    // The rating: its number in the mode's colour, then what it is.
    let dim = hsla(45.0, 100.0, 95.0, 0.7);
    let mut x = left;
    if let Some((value, _)) = card.rating {
        let g = text(painter, &value.to_string(), 22.0, display_family(), accent, 0.0);
        let w = g.size().x;
        painter.galley(pos2(x, y + 24.0 - g.size().y), g, Color32::WHITE);
        x += w;
    }
    let g = text(painter, &card.rating_text(), 17.0, body(), dim, 0.0);
    painter.galley(pos2(x, y + 24.0 - g.size().y), g, Color32::WHITE);
    y += 24.0 + 12.0;
    // The record: one bar of wins against draws and losses, its counts beside it.
    let counts = text(painter, &card.counts(), 16.0, body(), cream(), 0.0);
    let bar = Rect::from_min_max(
        pos2(left, y + 3.0),
        pos2((right - counts.size().x - 14.0).max(left + 80.0), y + 13.0),
    );
    painter.galley(
        pos2(right - counts.size().x, y + 8.0 - counts.size().y / 2.0),
        counts,
        Color32::WHITE,
    );
    let skew = |b: Rect| {
        vec![
            pos2(b.left() + 4.0, b.top()),
            b.right_top(),
            pos2(b.right() - 4.0, b.bottom()),
            b.left_bottom(),
        ]
    };
    painter.add(Shape::convex_polygon(
        skew(bar),
        hsla(250.0, 30.0, 30.0, 0.5),
        Stroke::NONE,
    ));
    let parts = [
        (card.wins, hsl(100.0, 80.0, 52.0)),
        (card.draws, hsla(45.0, 30.0, 80.0, 0.55)),
        (card.losses, hsl(350.0, 85.0, 58.0)),
    ];
    let total = (card.wins + card.draws + card.losses) as f32;
    if total > 0.0 {
        let shown = parts.iter().filter(|(n, _)| *n > 0).count() as f32;
        let room = bar.width() - 2.0 * (shown - 1.0);
        let clip = painter.with_clip_rect(bar.intersect(painter.clip_rect()));
        let mut px = bar.left();
        for (n, color) in parts {
            if n == 0 {
                continue;
            }
            let w = room * n as f32 / total;
            let part = Rect::from_min_size(pos2(px, bar.top()), vec2(w, bar.height()));
            clip.add(Shape::convex_polygon(skew(part), color, Stroke::NONE));
            px += w + 2.0;
        }
    }
    y += 16.0;
    // The totals: big numbers, each with its word.
    if !card.stats.is_empty() {
        y += 12.0 + 2.0;
        painter.hline(left..=right, y, Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.22)));
        y += 12.0;
        let columns = stats_columns(width, card.stats.len());
        let cell_w = (width - 12.0 * (columns as f32 - 1.0)) / columns as f32;
        for (i, (value, what)) in card.stats.iter().enumerate() {
            let cx = left + (i % columns) as f32 * (cell_w + 12.0);
            let cy = y + (i / columns) as f32 * (STAT_H + 10.0);
            let g = text(painter, value, 20.0, display_family(), cream(), 0.0);
            painter.galley(pos2(cx, cy), g, Color32::WHITE);
            let g = text(painter, what, 15.0, body(), hsla(45.0, 100.0, 95.0, 0.65), 0.0);
            painter.galley(pos2(cx, cy + 24.0), g, Color32::WHITE);
        }
        let rows = card.stats.len().div_ceil(columns) as f32;
        y += rows * STAT_H + (rows - 1.0) * 10.0;
    }
    if !card.note.is_empty() {
        y += 12.0;
        painter.galley(pos2(left, y), note_galley(painter, &card.note, width), Color32::WHITE);
    }
}

/// `recordTotals`: the totals over the cards, each on a plate in the
/// campaign's style, the first gold and the rest dark, each number large;
/// a note after them as text. Returns the bottom.
fn paint_totals(painter: &Painter, left: f32, top: f32, width: f32, status: &RecordStatus) -> f32 {
    use crate::client::campaign::campaign_view::{PlateLook, plate_behind};
    let mut x = left;
    let mut y = top;
    let mut line_h: f32 = 0.0;
    for (i, (value, word)) in status.totals.iter().enumerate() {
        let look = if i == 0 { PlateLook::Gold } else { PlateLook::Dark };
        let ink = if i == 0 { hsl(250.0, 45.0, 7.0) } else { cream() };
        let number = text(painter, value, 26.0, display_family(), ink, 26.0 * 0.02);
        let label = text(painter, &word.to_uppercase(), 14.0, display_family(), ink, 14.0 * 0.12);
        let w = 20.0 + number.size().x + 10.0 + label.size().x + 26.0;
        let h = 10.0 + number.size().y + 12.0;
        if x > left && x + w > left + width {
            x = left;
            y += line_h + 14.0;
            line_h = 0.0;
        }
        let r = Rect::from_min_size(pos2(x, y), vec2(w, h));
        plate_behind(painter, r, look);
        let base = r.top() + 10.0 + number.size().y;
        let nw = number.size().x;
        painter.galley(pos2(r.left() + 20.0, r.top() + 10.0), number, Color32::WHITE);
        painter.galley(
            pos2(r.left() + 20.0 + nw + 10.0, base - label.size().y - 2.0),
            label,
            Color32::WHITE,
        );
        x = r.right() + 22.0;
        line_h = line_h.max(h);
    }
    if !status.note.is_empty() {
        let g = text(painter, &status.note, 16.0, body(), hsla(45.0, 100.0, 95.0, 0.7), 0.0);
        if x > left && x + g.size().x > left + width {
            x = left;
            y += line_h + 14.0;
            line_h = 0.0;
        }
        painter.galley(
            pos2(x, y + (line_h - g.size().y).max(0.0) / 2.0),
            g.clone(),
            Color32::WHITE,
        );
        line_h = line_h.max(g.size().y);
    }
    y + line_h
}

/// `profile-record` on the stage: the head, the totals and a card per mode;
/// the cards scroll when the stage is short.
fn paint_record(ui: &mut egui::Ui, stage: Rect, status: &RecordStatus, cards: &[RecordCard]) {
    let painter = ui.painter().clone();
    let left = stage.left() + RAIL_SKEW + 56.0;
    let width = (stage.right() - 56.0 - left).min(660.0) - 8.0;
    if width < 120.0 {
        return;
    }
    let mut y = stage.top() + 64.0;
    let title = text(&painter, "YOUR RECORD", 15.0, display_family(), cream(), 15.0 * 0.16);
    let th = title.size().y;
    painter.galley(pos2(left, y), title, Color32::WHITE);
    y += th + 12.0;
    painter.line_segment(
        [pos2(left, y), pos2(left + width, y)],
        Stroke::new(1.5, hsla(276.0, 90.0, 64.0, 0.35)),
    );
    y += 16.0;
    if !status.totals.is_empty() {
        y = paint_totals(&painter, left, y, width, status) + 18.0;
    } else if !status.text.is_empty() {
        let mut job = egui::text::LayoutJob::simple(
            status.text.clone(),
            egui::FontId::proportional(16.0),
            hsla(45.0, 100.0, 95.0, 0.7),
            width,
        );
        job.wrap.break_anywhere = false;
        let g = painter.layout_job(job);
        let h = g.size().y;
        painter.galley(pos2(left, y), g, Color32::WHITE);
        y += h + 18.0;
    }
    if cards.is_empty() {
        return;
    }
    let area = Rect::from_min_max(pos2(left, y), pos2(left + width + 14.0, stage.bottom() - 24.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(area), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("profile-record")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let origin = ui.cursor().min;
                let mut cy = origin.y;
                for card in cards {
                    let h = card_height(ui.painter(), card, width);
                    paint_card(
                        ui.painter(),
                        Rect::from_min_size(pos2(origin.x, cy), vec2(width, h)),
                        card,
                    );
                    cy += h + 14.0;
                }
                ui.allocate_rect(
                    Rect::from_min_max(origin, pos2(origin.x + width, cy - 14.0)),
                    egui::Sense::hover(),
                );
            });
    });
}

/// The Downloads folder, or the config folder without one.
fn downloads_dir() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    home.map(|h| h.join("Downloads"))
        .filter(|d| d.is_dir())
        .or_else(crate::config_dir)
}

/// `washoi-key-<name>.txt`, with anything but letters, digits, `_` and `-`
/// in the name made `_`: the lobby's names are only those, but the file
/// name must not depend on that.
fn key_file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("washoi-key-{safe}.txt")
}

/// `saveKeyFile`: `washoi-key-<name>.txt` in the Downloads folder.
pub(crate) fn save_key_file(name: &str, key: &str) -> std::io::Result<PathBuf> {
    let dir = downloads_dir().ok_or_else(|| std::io::Error::other("no folder to save in"))?;
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(key_file_name(name));
    shagger::core::atomic_file::write_atomic_private(&path, key_file_lines(name, &format_key(key)).as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_key_file_name_stays_in_its_folder() {
        assert_eq!(key_file_name("Pilot_7"), "washoi-key-Pilot_7.txt");
        assert_eq!(key_file_name("../../x"), "washoi-key-______x.txt");
        assert_eq!(key_file_name("a/b\\c"), "washoi-key-a_b_c.txt");
    }

    fn summary(data: Value) -> Value {
        json!({ "type": "stats:summary", "requestId": "stats_1", "data": data })
    }

    fn whole() -> Value {
        json!({
            "scope": "overall", "gametype": "snd", "period": "all",
            "matches": 3, "wins": 2, "losses": 1, "draws": 0, "statsMatches": 2, "excluded": 1,
            "rating": { "value": 1234, "rank": 7, "ratedMatches": 3, "provisional": true },
            "totals": { "frags": 21, "deaths": 9, "damageGiven": 2050.5, "activeTimeMs": 1_530_000,
                        "plants": 2, "defuses": 1 }
        })
    }

    #[test]
    fn a_summary_is_checked_as_get_player_stats_checks_it() {
        let s = read_stats(&summary(whole()), "snd").expect("a summary");
        assert_eq!(s.rating, Some((1234, 7, true)));
        assert!(read_stats(&summary(whole()), "duel").is_err());
        let mut uneven = whole();
        uneven["wins"] = json!(5);
        assert!(read_stats(&summary(uneven), "snd").is_err());
        let mut no_totals = whole();
        no_totals["totals"] = Value::Null;
        assert!(read_stats(&summary(no_totals), "snd").is_err());
    }

    #[test]
    fn rename_failures_read_as_the_browser_says_them() {
        assert_eq!(rename_failure_text("name_taken"), "Someone already has that name.");
        assert_eq!(rename_failure_text("bogus"), "Rename failed: bogus");
    }

    #[test]
    fn the_key_file_names_the_player_and_the_key() {
        let file = key_file_lines("Tarmo", "WASHOI-0123");
        assert!(file.starts_with("Washiki Washoi player key\n\nPlayer: Tarmo\nKey:    WASHOI-0123\n"));
        assert!(file.ends_with("Keep it private.\n"));
    }

    fn mode(id: &str, name: &str) -> GametypeInfo {
        serde_json::from_value(json!({
            "id": id, "name": name, "maxPlayers": 2, "hasTeams": false
        }))
        .expect("a mode")
    }

    fn entry(rank: u32, wins: u32) -> Value {
        json!({ "rank": rank, "playerId": "p", "name": "Tarmo", "wins": wins, "losses": 1, "draws": 0,
                "rating": 1100, "conservativeScore": 10.0, "provisional": false, "ratedMatches": 3 })
    }

    #[test]
    fn without_stats_the_record_comes_from_the_boards() {
        let mut client = LobbyClient::connect("ws://127.0.0.1:9");
        let modes = [mode("duel", "Duel"), mode("ffa", "FFA")];
        let account = Account {
            name: "Tarmo",
            key: None,
            key_unsaved: false,
            id: Some("a1"),
            connected: true,
            in_room: false,
            modes: &modes,
        };
        let mut view = ProfileView::new();
        view.open(Screen::Key, &account, &mut client);
        view.bind(false, true, &account, &mut client);
        assert_eq!(view.record_status.sentence(), "Reading the rankings…");
        let requests = view.asking.as_ref().expect("asking").requests.clone();
        for (request, rank) in requests.iter().zip([5, 2]) {
            let page = json!({ "type": "leaderboard:page", "requestId": request, "scope": "overall",
                "system": "trueskill", "policyVersion": "shag-trueskill-v2", "entries": [], "total": 9,
                "localPlayerEntry": entry(rank, 3) });
            view.on_message("leaderboard:page", &page, &account);
        }
        assert_eq!(view.record_status.sentence(), "8 matches · #2 in FFA");
        let order: Vec<&str> = view.record_rows.iter().map(|r| r.mode.as_str()).collect();
        assert_eq!(order, ["FFA", "Duel"]);
        assert_eq!(view.record_rows[0].rating, Some((1100, false)));
        assert_eq!(view.record_rows[0].rating_text(), " rating");
        assert_eq!(view.record_rows[0].counts(), "3 wins · 1 loss");
    }

    #[test]
    fn a_cards_totals_read_as_the_browser_writes_them() {
        assert_eq!(play_time(54.0 * 60000.0), "54m");
        assert_eq!(play_time(134.4 * 60000.0), "2h 14m");
        assert_eq!(js_group_thousands(1234567.0), "1,234,567");
        assert_eq!(js_group_thousands(999.0), "999");
        let summary = read_stats(&summary(whole()), "snd").expect("a summary");
        let (stats, note) = stats_cells(&summary, true);
        let words: Vec<String> = stats.iter().map(|(v, w)| format!("{v} {w}")).collect();
        assert_eq!(
            words,
            [
                "21 frags",
                "9 deaths",
                "2,051 damage",
                "26m played",
                "2 plants",
                "1 defuses"
            ]
        );
        assert_eq!(note, "Totals from 2 of 3 matches.");
    }

    #[test]
    fn a_rename_says_how_it_went() {
        let mut client = LobbyClient::connect("ws://127.0.0.1:9");
        let account = Account {
            name: "Tarmo",
            key: Some("0123456789ABCDEFGHJKMNPQ"),
            key_unsaved: false,
            id: Some("a1"),
            connected: true,
            in_room: false,
            modes: &[],
        };
        let mut view = ProfileView::new();
        view.open(Screen::Name, &account, &mut client);
        assert_eq!(view.said(Screen::Name), ("This is your name.", Tone::Plain));
        view.type_into(Screen::Name, "Ramen", &account);
        view.rename(&account, &mut client);
        assert!(view.renaming.is_none(), "a bot's name is not sent");
        view.type_into(Screen::Name, "Udon_Fan", &account);
        view.rename(&account, &mut client);
        assert_eq!(view.said(Screen::Name).0, "Asking the lobby…");
        view.on_message("identity:error", &json!({ "code": "name_taken" }), &account);
        assert_eq!(view.said(Screen::Name), ("Someone already has that name.", Tone::Bad));
        // Deleting wants the box ticked and this player's own key.
        view.type_into(Screen::Delete, "1111-2222-3333-4444-5555-6666", &account);
        assert_eq!(view.said(Screen::Delete), ("That isn't this player's key.", Tone::Bad));
        view.type_into(Screen::Delete, "washoi 0123 4567 89ab cdef ghjk mnpq", &account);
        assert!(view.delete_ready(&account));
    }
}
