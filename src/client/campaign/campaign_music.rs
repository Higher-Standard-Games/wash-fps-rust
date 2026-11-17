//! wash-fps `client/campaign/CampaignMusic.ts`: the campaign's own music.
//! The campaign page plays its opener over the menus' playlist, and some
//! missions play a song of their own in place of the game playlist. None of
//! it plays anywhere else.

/// The campaign page's playlist: the opener, looping.
pub const CAMPAIGN_MENU_PLAYLIST: &str = "data/campaign.playlist.json";

/// Every playlist a mission plays, for the data sync and the music loader.
pub const MISSION_PLAYLISTS: [&str; 3] = [
    "data/campaign_cartel.playlist.json",
    "data/campaign_boss.playlist.json",
    // Its own file, so the lobby music key leaves Bot Trap's music playing.
    "data/campaign_survival.playlist.json",
];

/// `missionPlaylist`: the playlist a campaign mission's match plays, or
/// `None` for the game playlist.
#[must_use]
pub fn mission_playlist(mission_id: &str) -> Option<&'static str> {
    match mission_id {
        "cartel-contract" => Some(MISSION_PLAYLISTS[0]),
        "siege-breaker" | "road-ends" => Some(MISSION_PLAYLISTS[1]),
        "bot-trap" => Some(MISSION_PLAYLISTS[2]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn some_missions_play_their_own_song() {
        assert_eq!(
            mission_playlist("cartel-contract"),
            Some("data/campaign_cartel.playlist.json")
        );
        assert_eq!(mission_playlist("road-ends"), Some("data/campaign_boss.playlist.json"));
        assert_eq!(mission_playlist("street-riot"), None);
    }
}
