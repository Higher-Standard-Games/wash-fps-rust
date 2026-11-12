//! wash-fps `client/LobbyRooms.ts`: the decisions the lobby makes about
//! rooms and its Play keys, kept apart from drawing so they can be tested.

use shagger::lobby::types::LiveMatchInfo;

/// `isJoinable`: a room still forming with a free seat.
#[must_use]
pub fn is_joinable(m: &LiveMatchInfo) -> bool {
    m.state == "waiting" && m.player_count < m.max_players
}

/// `sortRooms`: joinable first, fewest seats left on top; then full rooms
/// about to start; then running matches. Stable, as `Array.sort` is.
#[must_use]
pub fn sort_rooms(matches: &[LiveMatchInfo]) -> Vec<LiveMatchInfo> {
    let rank = |m: &LiveMatchInfo| {
        if is_joinable(m) {
            0
        } else if m.state == "waiting" {
            1
        } else {
            2
        }
    };
    let left = |m: &LiveMatchInfo| i64::from(m.max_players) - i64::from(m.player_count);
    let mut sorted = matches.to_vec();
    sorted.sort_by(|a, b| rank(a).cmp(&rank(b)).then(left(a).cmp(&left(b))));
    sorted
}

/// `roomCountLabel`: "1 of 3 open", or "none".
#[must_use]
pub fn room_count_label(matches: &[LiveMatchInfo]) -> String {
    if matches.is_empty() {
        return "none".into();
    }
    let open = matches.iter().filter(|m| is_joinable(m)).count();
    format!("{open} of {} open", matches.len())
}

/// `RoomAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomAction {
    Join,
    Watch,
    Full,
}

impl RoomAction {
    /// The badge's word.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Join => "join",
            Self::Watch => "watch",
            Self::Full => "full",
        }
    }
}

/// `RoomRow`: what a room's row offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomRow {
    pub action: RoomAction,
    /// The map for a running match, otherwise "Open" or "Starting".
    pub where_: String,
    pub disabled: bool,
}

/// `describeRoom`: a match under way takes players while it has room, as a
/// Quake server does; a full one can still be watched.
#[must_use]
pub fn describe_room(m: &LiveMatchInfo) -> RoomRow {
    let playing = m.state == "playing";
    let drop_in = playing && m.server_url.is_some() && m.player_count < m.max_players;
    let action = if playing {
        if drop_in { RoomAction::Join } else { RoomAction::Watch }
    } else if is_joinable(m) {
        RoomAction::Join
    } else {
        RoomAction::Full
    };
    let where_ = if playing {
        if m.map_display_name.is_empty() {
            m.map.clone()
        } else {
            m.map_display_name.clone()
        }
    } else if action == RoomAction::Full {
        "Starting".into()
    } else {
        "Open".into()
    };
    let disabled = action == RoomAction::Full || (action == RoomAction::Watch && m.server_url.is_none());
    RoomRow {
        action,
        where_,
        disabled,
    }
}

/// `PlayKeysInput`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayKeysInput<'a> {
    /// Title of the selected mode, such as "Duel".
    pub mode_title: &'a str,
    /// The lobby server offers the selected mode.
    pub available: bool,
    pub connected: bool,
    pub queue_pending: bool,
    pub private_lobby_supported: bool,
    /// Turned away at the lobby's player cap, rather than unable to reach it.
    pub lobby_full: bool,
}

/// `PlayKeys`: the Find and Private keys' state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayKeys {
    pub find_disabled: bool,
    pub find_label: String,
    pub private_disabled: bool,
    pub private_title: &'static str,
}

/// `playKeys`.
#[must_use]
pub fn play_keys(i: PlayKeysInput<'_>) -> PlayKeys {
    let find_label = if i.queue_pending {
        "Joining…".to_owned()
    } else if !i.connected {
        if i.lobby_full { "Servers full" } else { "Lobby offline" }.to_owned()
    } else if i.available {
        format!("Find {}", i.mode_title)
    } else {
        format!("{} soon", i.mode_title)
    };
    PlayKeys {
        find_disabled: i.queue_pending || !i.connected || !i.available,
        find_label,
        private_disabled: i.queue_pending || !i.connected || !i.available || !i.private_lobby_supported,
        private_title: if !i.private_lobby_supported {
            "This lobby server does not offer private rooms."
        } else if !i.available {
            "This mode is not offered by the lobby server."
        } else {
            "Create an invite-only room."
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(id: &str, state: &str, count: u32, max: u32, server: bool) -> LiveMatchInfo {
        LiveMatchInfo {
            id: id.into(),
            gametype: "duel".into(),
            gametype_name: "Duel".into(),
            map: "q3dm17".into(),
            map_display_name: String::new(),
            players: Vec::new(),
            player_count: count,
            max_players: max,
            state: state.into(),
            started_at: 0.0,
            server_url: server.then(|| "ws://game".into()),
        }
    }

    #[test]
    fn rooms_sort_as_the_browser_sorts_them() {
        let rooms = [
            room("playing", "playing", 1, 2, true),
            room("full", "waiting", 2, 2, false),
            room("roomy", "waiting", 1, 8, false),
            room("nearly", "waiting", 3, 4, false),
        ];
        let ids: Vec<_> = sort_rooms(&rooms).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["nearly", "roomy", "full", "playing"]);
        assert_eq!(room_count_label(&rooms), "2 of 4 open");
        assert_eq!(room_count_label(&[]), "none");
    }

    #[test]
    fn a_row_offers_what_the_room_allows() {
        let drop_in = describe_room(&room("a", "playing", 1, 2, true));
        assert_eq!(
            (drop_in.action, drop_in.where_.as_str(), drop_in.disabled),
            (RoomAction::Join, "q3dm17", false)
        );
        let watch = describe_room(&room("b", "playing", 2, 2, true));
        assert_eq!(watch.action, RoomAction::Watch);
        assert!(describe_room(&room("c", "playing", 2, 2, false)).disabled);
        let full = describe_room(&room("d", "waiting", 2, 2, false));
        assert_eq!(
            (full.action, full.where_.as_str(), full.disabled),
            (RoomAction::Full, "Starting", true)
        );
        assert_eq!(describe_room(&room("e", "waiting", 1, 2, false)).where_, "Open");
    }

    #[test]
    fn the_play_keys_say_why_they_are_off() {
        let base = PlayKeysInput {
            mode_title: "Duel",
            available: true,
            connected: true,
            private_lobby_supported: true,
            ..PlayKeysInput::default()
        };
        assert_eq!(play_keys(base).find_label, "Find Duel");
        assert!(!play_keys(base).private_disabled);
        let offline = play_keys(PlayKeysInput {
            connected: false,
            ..base
        });
        assert_eq!(
            (offline.find_label.as_str(), offline.find_disabled),
            ("Lobby offline", true)
        );
        let full = PlayKeysInput {
            connected: false,
            lobby_full: true,
            ..base
        };
        assert_eq!(play_keys(full).find_label, "Servers full");
        let soon = PlayKeysInput {
            available: false,
            ..base
        };
        assert_eq!(play_keys(soon).find_label, "Duel soon");
        assert_eq!(
            play_keys(soon).private_title,
            "This mode is not offered by the lobby server."
        );
        let pending = PlayKeysInput {
            queue_pending: true,
            ..base
        };
        assert_eq!(play_keys(pending).find_label, "Joining…");
    }
}
