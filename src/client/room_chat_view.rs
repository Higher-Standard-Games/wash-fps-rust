//! wash-fps `client/RoomChatView.ts`: a match room's member-only chat, and
//! who comes and goes. The global lobby feed stays on the Play page; this
//! one takes the rail's middle while the player is in a room. The model is
//! here; [`crate::client::lobby_panel`] draws it with the lobby chat's look.

use std::collections::VecDeque;

use serde_json::Value;
use shagger::lobby::lobby_client::LobbyClient;
use shagger::lobby::types::LobbyIn;

use crate::client::lobby_panel::ChatLine;

/// `MAX_VISIBLE_MESSAGES`.
const MAX_VISIBLE_MESSAGES: usize = 50;

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// `RoomChatView`.
#[derive(Debug)]
pub struct RoomChatView {
    pub lobby_id: String,
    /// The lobby server keeps room chat (`chatHistory` is in the room's state).
    pub supported: bool,
    /// The room's members by id, in arrival order: a leave names only the id.
    members: Vec<(String, String)>,
    pub lines: VecDeque<ChatLine>,
    pub draft: String,
}

impl RoomChatView {
    /// From the room's state (`lobby:created`).
    #[must_use]
    pub fn new(lobby: &Value) -> Self {
        let history = lobby.get("chatHistory").and_then(Value::as_array);
        let mut view = Self {
            lobby_id: str_of(lobby, "id").to_owned(),
            supported: history.is_some(),
            members: Vec::new(),
            lines: VecDeque::new(),
            draft: String::new(),
        };
        for p in lobby.get("players").and_then(Value::as_array).into_iter().flatten() {
            view.members
                .push((str_of(p, "id").to_owned(), str_of(p, "name").to_owned()));
        }
        for m in history.into_iter().flatten() {
            view.push(ChatLine {
                from: str_of(m, "from").to_owned(),
                text: str_of(m, "text").to_owned(),
                ts: m.get("ts").and_then(Value::as_f64).unwrap_or(0.0),
                system: false,
            });
        }
        view
    }

    /// `refreshHere`: "5 here".
    #[must_use]
    pub fn here(&self) -> String {
        format!("{} here", self.members.len())
    }

    /// The field's placeholder (`refreshConnection`).
    #[must_use]
    pub fn placeholder(&self, connected: bool) -> &'static str {
        if !self.supported {
            "Room chat needs a newer lobby server"
        } else if connected {
            "Say something…"
        } else {
            "Lobby server offline"
        }
    }

    /// `showEmpty`.
    #[must_use]
    pub fn empty_text(&self) -> &'static str {
        if self.supported {
            "No messages yet. Only the people in this room see them."
        } else {
            "Room chat is unavailable on this lobby server."
        }
    }

    fn push(&mut self, line: ChatLine) {
        self.lines.push_back(line);
        while self.lines.len() > MAX_VISIBLE_MESSAGES {
            self.lines.pop_front();
        }
    }

    /// `appendNote`: someone came or went.
    fn note(&mut self, text: String) {
        self.push(ChatLine {
            from: String::new(),
            text,
            ts: now_ms(),
            system: true,
        });
    }

    /// The lobby's messages for this room.
    pub fn handle(&mut self, message: &LobbyIn) {
        let LobbyIn::Other { kind, message } = message else {
            return;
        };
        match kind.as_str() {
            "lobby:chat_message" => {
                if !self.supported || str_of(message, "lobbyId") != self.lobby_id {
                    return;
                }
                self.push(ChatLine {
                    from: str_of(message, "from").to_owned(),
                    text: str_of(message, "text").to_owned(),
                    ts: message.get("ts").and_then(Value::as_f64).unwrap_or_else(now_ms),
                    system: false,
                });
            }
            "lobby:player_joined" => {
                let Some(p) = message.get("player") else { return };
                let id = str_of(p, "id");
                // A player the room already has is announced again on a reconnect.
                if self.members.iter().any(|(m, _)| m == id) {
                    return;
                }
                let name = str_of(p, "name").to_owned();
                self.members.push((id.to_owned(), name.clone()));
                self.note(format!("{name} joined"));
            }
            "lobby:player_left" => {
                let id = str_of(message, "playerId");
                let Some(at) = self.members.iter().position(|(m, _)| m == id) else {
                    return;
                };
                let (_, name) = self.members.remove(at);
                // Back from a match, a player leaves on the old connection and
                // joins on the new one: only a name that is really gone is said.
                if !self.members.iter().any(|(_, n)| *n == name) {
                    self.note(format!("{name} left"));
                }
            }
            _ => {}
        }
    }

    /// The form's submit: sends the draft and clears it.
    pub fn send(&mut self, client: &LobbyClient, connected: bool) {
        let text = self.draft.trim().to_owned();
        if text.is_empty() || !connected || !self.supported {
            return;
        }
        client.room_chat_send(&self.lobby_id, &text);
        self.draft.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn other(kind: &str, message: Value) -> LobbyIn {
        LobbyIn::Other {
            kind: kind.into(),
            message,
        }
    }

    #[test]
    fn the_room_chat_says_who_comes_and_goes_once() {
        let room = serde_json::json!({
            "id": "L1", "chatHistory": [{"from": "Ramen", "fromId": "b", "text": "hi", "ts": 1.0}],
            "players": [{"id": "me", "name": "Washiki"}]
        });
        let mut chat = RoomChatView::new(&room);
        assert_eq!(chat.lines.len(), 1);
        assert_eq!(chat.here(), "1 here");
        let join = other(
            "lobby:player_joined",
            serde_json::json!({"player": {"id": "b", "name": "Ramen"}}),
        );
        chat.handle(&join);
        chat.handle(&join);
        assert_eq!(chat.here(), "2 here");
        assert_eq!(chat.lines.back().map(|l| l.text.as_str()), Some("Ramen joined"));
        assert_eq!(chat.lines.len(), 2);
        chat.handle(&other("lobby:player_left", serde_json::json!({"playerId": "b"})));
        assert_eq!(chat.lines.back().map(|l| l.text.as_str()), Some("Ramen left"));
        chat.handle(&other(
            "lobby:chat_message",
            serde_json::json!({"lobbyId": "other", "from": "x", "text": "not ours"}),
        ));
        assert_eq!(chat.lines.len(), 3);
    }

    #[test]
    fn an_older_lobby_server_has_no_room_chat() {
        let chat = RoomChatView::new(&serde_json::json!({"id": "L1", "players": []}));
        assert!(!chat.supported);
        assert_eq!(chat.placeholder(true), "Room chat needs a newer lobby server");
    }
}
