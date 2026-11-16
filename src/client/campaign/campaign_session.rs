//! wash-fps `client/campaign/CampaignSession.ts`: the campaign between
//! screens. This account's last known progress and each mission's looks
//! are kept on disk beside the settings (the browser's localStorage); the
//! mission being played is kept for the run (its sessionStorage), carrying
//! it from the lobby's connect hand-off to the match and back to the lobby
//! after it, with a mission to play again at once when the player asked to
//! retry it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use shagger::lobby::types::{CampaignMissionInfo, CampaignProgress, LobbyCampaignInfo};

/// The campaign match being played (`MATCH_KEY`).
static MATCH: Mutex<Option<LobbyCampaignInfo>> = Mutex::new(None);
/// The mission to play again at once (`RETRY_KEY`).
static RETRY: Mutex<Option<String>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Where the campaign's files live: beside the settings.
fn dir() -> Option<PathBuf> {
    crate::config_dir().map(|d| d.join("campaign"))
}

/// An account id as a file name: what is not a letter, digit, `-` or `_`
/// becomes `_`.
fn file_name(account_id: &str) -> String {
    let safe: String = account_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("progress.{safe}.json")
}

fn read_json<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let text = std::fs::read_to_string(dir()?.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Storage is a convenience here; the lobby has the record.
fn write_json<T: serde::Serialize>(name: &str, value: &T) {
    let Some(dir) = dir() else { return };
    let Ok(text) = serde_json::to_string(value) else { return };
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = shagger::core::atomic_file::write_atomic(&dir.join(name), text.as_bytes());
    }
}

/// `cachedCampaign`: the account's progress as this computer last heard
/// it, to draw before the lobby answers.
#[must_use]
pub fn cached_campaign(account_id: &str) -> Option<CampaignProgress> {
    read_json(&file_name(account_id))
}

/// `cacheCampaign`.
pub fn cache_campaign(account_id: &str, state: &CampaignProgress) {
    write_json(&file_name(account_id), state);
}

/// `lobbyCampaignInfo`: how a mission's match is drawn.
#[must_use]
pub fn lobby_campaign_info(mission: &CampaignMissionInfo) -> LobbyCampaignInfo {
    LobbyCampaignInfo {
        mission_id: mission.id.clone(),
        models: mission.models.clone(),
        bot_models: mission
            .bots
            .iter()
            .filter_map(|b| Some((b.name.clone(), b.model.clone()?)))
            .collect(),
        bot_scales: mission
            .bots
            .iter()
            .filter_map(|b| Some((b.name.clone(), b.scale?)))
            .collect(),
        slime_immune: Vec::new(),
    }
}

const LOOKS_FILE: &str = "looks.json";

/// `cacheMissionLooks`: keep how each mission's match is drawn (its
/// models, its bots' own models and sizes), by mission id, for its demos: a
/// recording names only its mission. Added to what is kept, so a mission the
/// lobby no longer runs still draws its old demos as they were.
pub fn cache_mission_looks(missions: &[CampaignMissionInfo]) {
    let mut looks: BTreeMap<String, LobbyCampaignInfo> = read_json(LOOKS_FILE).unwrap_or_default();
    looks.extend(missions.iter().map(|m| (m.id.clone(), lobby_campaign_info(m))));
    write_json(LOOKS_FILE, &looks);
}

/// `missionLook`: how a mission's match is drawn, as this computer last
/// heard from the lobby; `None` if never.
#[must_use]
pub fn mission_look(mission_id: &str) -> Option<LobbyCampaignInfo> {
    read_json::<BTreeMap<String, LobbyCampaignInfo>>(LOOKS_FILE)?.remove(mission_id)
}

/// `rememberCampaignMatch`: a match is starting; keep its mission and
/// models for the game (`None` for any other match).
pub fn remember_campaign_match(info: Option<LobbyCampaignInfo>) {
    *lock(&MATCH) = info;
}

/// `campaignMatch`: the campaign match being joined, if it is one.
#[must_use]
pub fn campaign_match() -> Option<LobbyCampaignInfo> {
    lock(&MATCH).clone()
}

/// `requestCampaignRetry`: the player lost a mission and asked to play it
/// again: the lobby starts it on return.
pub fn request_campaign_retry(mission_id: &str) {
    *lock(&RETRY) = Some(mission_id.to_owned());
}

/// `takeCampaignRetry`: the mission to play again at once, once.
#[must_use]
pub fn take_campaign_retry() -> Option<String> {
    lock(&RETRY).take()
}

/// `takeCampaignReturn`: back in the lobby after a campaign match, which
/// mission it was, once.
#[must_use]
pub fn take_campaign_return() -> Option<LobbyCampaignInfo> {
    lock(&MATCH).take()
}

/// What a match draws for a campaign (`Game.ts`'s connect): its models,
/// its crew in green, the countdown that holds everyone still and its own
/// song. Any other match goes back to the settings.
pub fn apply_campaign_looks(campaign: Option<&LobbyCampaignInfo>) {
    use crate::client::player_model_selection::force_player_models;
    use crate::client::player_model_visual_settings::set_campaign_teammates;
    match campaign {
        Some(c) => force_player_models(
            Some((c.models.ally.as_str(), c.models.enemy.as_str())),
            c.bot_models.iter().map(|(n, m)| (n.as_str(), m.as_str())),
            c.bot_scales.iter().map(|(n, s)| (n.as_str(), *s)),
        ),
        None => force_player_models(None, [], []),
    }
    set_campaign_teammates(campaign.is_some());
    crate::client::game_events::set_match_playlist(
        campaign.and_then(|c| crate::client::campaign::campaign_music::mission_playlist(&c.mission_id)),
    );
}

/// `DemoService.applyCampaignLook`: draw a campaign demo as its match was
/// drawn, from the look this computer last heard for its mission; `None`
/// puts every match back to the player's settings.
pub fn apply_demo_look(mission_id: Option<&str>) {
    use crate::client::player_model_selection::force_player_models;
    use crate::client::player_model_visual_settings::set_campaign_teammates;
    let look = mission_id.and_then(mission_look);
    if let (Some(id), None) = (mission_id, &look) {
        println!("demo: no look known for campaign mission \"{id}\": open the campaign page once");
    }
    match &look {
        Some(c) => force_player_models(
            Some((c.models.ally.as_str(), c.models.enemy.as_str())),
            c.bot_models.iter().map(|(n, m)| (n.as_str(), m.as_str())),
            c.bot_scales.iter().map(|(n, s)| (n.as_str(), *s)),
        ),
        None => force_player_models(None, [], []),
    }
    set_campaign_teammates(look.is_some());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_ids_make_safe_file_names() {
        assert_eq!(file_name("a1-b_2"), "progress.a1-b_2.json");
        assert_eq!(file_name("../x"), "progress.___x.json");
    }

    #[test]
    fn a_missions_look_names_its_bots_own_models_and_sizes() {
        let mission: CampaignMissionInfo = serde_json::from_str(
            r#"{"id":"siege-breaker","number":6,"map":"m","gametype":"duel","models":{"ally":"robot","enemy":"robot"},
                "bots":[{"name":"Siege","difficulty":"hard","side":"enemy","scale":1.8},
                        {"name":"Kitty","difficulty":"easy","side":"enemy","model":"riotkitty"}]}"#,
        )
        .unwrap();
        let look = lobby_campaign_info(&mission);
        assert_eq!(look.bot_scales.get("Siege"), Some(&1.8));
        assert_eq!(look.bot_models.get("Kitty").map(String::as_str), Some("riotkitty"));
        assert!(!look.bot_models.contains_key("Siege"));
    }
}
