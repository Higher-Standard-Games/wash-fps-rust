//! In-game chat (wash-fps `client/hud/widgets/Chat.ts`, styles
//! `.fps-chat`): the last five messages float over the game and fade after
//! eight seconds; talk mode (T, or Y for the team) opens the history and a
//! line to type in. Enter sends (no local echo), Escape cancels. `/mute`
//! and `/unmute` stay local; `/say_team` sends to the team.

use shagger::ui::hud_2d::{Align, Hud2d};

use crate::client::hud::washiki::{Fonts, Slab, UI_SCALE, hsla};

/// `ChatMessage`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub sender: String,
    pub text: String,
    /// "udon", "soba", "ffa", "spectator" or "": colours the sender.
    pub team: String,
    /// Seconds on the HUD's clock.
    pub timestamp: f64,
    /// From the game rather than a player: no sender stamp.
    pub notice: bool,
    pub player_name: String,
}

const MESSAGE_FADE: f64 = 8.0;
const FADE_OUT: f64 = 1.0;
const MAX_VISIBLE: usize = 5;
const HISTORY_MAX: usize = 100;
const MAX_LENGTH: usize = 128;

/// `FPSChat`.
#[derive(Debug, Default)]
pub struct Chat {
    messages: Vec<ChatMessage>,
    local_team: String,
    /// The line being typed, in talk mode.
    input: Option<String>,
    team_input: bool,
    muted: Vec<String>,
    now: f64,
}

/// What Enter asks to send: the text and whether to the team only.
pub type Outgoing = (String, bool);

impl Chat {
    /// `setLocalTeam`.
    pub fn set_local_team(&mut self, team: &str) {
        team.clone_into(&mut self.local_team);
    }

    /// Moves the clock on, for the fade.
    pub fn tick(&mut self, dt: f64) {
        self.now += dt;
    }

    /// `addMessage`.
    pub fn add_message(&mut self, sender: &str, text: &str, team: &str) {
        if self.muted.contains(&sender.trim().to_lowercase()) {
            return;
        }
        self.push(ChatMessage {
            sender: sender.to_owned(),
            text: text.to_owned(),
            team: team.to_owned(),
            timestamp: self.now,
            notice: false,
            player_name: sender.to_owned(),
        });
    }

    /// `addNotice`: a line from the game, such as why a join was refused.
    pub fn add_notice(&mut self, text: &str) {
        self.push(ChatMessage {
            sender: String::new(),
            text: text.to_owned(),
            team: String::new(),
            timestamp: self.now,
            notice: true,
            player_name: String::new(),
        });
    }

    fn push(&mut self, message: ChatMessage) {
        self.messages.push(message);
        if self.messages.len() > HISTORY_MAX {
            self.messages.remove(0);
        }
    }

    /// `openInput`.
    pub fn open_input(&mut self, team_only: bool) {
        if team_only && self.local_team != "udon" && self.local_team != "soba" {
            self.add_notice("Team chat is only available to players on a team.");
            return;
        }
        self.team_input = team_only;
        self.input = Some(String::new());
    }

    /// `closeInput`.
    pub fn close_input(&mut self) {
        self.input = None;
    }

    #[must_use]
    pub fn is_input_active(&self) -> bool {
        self.input.is_some()
    }

    /// Typed text (the input's `maxLength` is 128).
    pub fn type_text(&mut self, text: &str) {
        if let Some(line) = &mut self.input {
            for ch in text.chars().filter(|c| !c.is_control()) {
                if line.chars().count() < MAX_LENGTH {
                    line.push(ch);
                }
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(line) = &mut self.input {
            line.pop();
        }
    }

    /// Enter: closes talk mode and says what to send, if anything.
    pub fn submit(&mut self) -> Option<Outgoing> {
        let text = self.input.take()?.trim().to_owned();
        if text.is_empty() || self.local_command(&text) {
            return None;
        }
        let lower = text.to_lowercase();
        if lower == "/say_team" || lower.starts_with("/say_team ") {
            let message = text["/say_team".len()..].trim();
            if message.is_empty() {
                self.add_notice("Usage: /say_team <message>");
                return None;
            }
            return Some((message.to_owned(), true));
        }
        Some((text, self.team_input))
    }

    /// `localCommand`: `/mute` and `/unmute`.
    fn local_command(&mut self, text: &str) -> bool {
        let lower = text.to_lowercase();
        let (command, rest) = match lower.split_once(char::is_whitespace) {
            Some((c, _)) => (c.to_owned(), text[c.len()..].trim()),
            None => (lower.clone(), ""),
        };
        let mute = match command.as_str() {
            "/mute" => true,
            "/unmute" => false,
            _ => return false,
        };
        let name = rest.trim_matches('"');
        let key = name.to_lowercase();
        let verb = &command[1..];
        if key.is_empty() {
            self.add_notice(&format!("Usage: /{verb} <playername>"));
            return true;
        }
        if mute {
            if !self.muted.contains(&key) {
                self.muted.push(key.clone());
            }
            self.messages
                .retain(|m| m.notice || m.player_name.trim().to_lowercase() != key);
            self.add_notice(&format!("Muted {name} for this match. Use /unmute {name} to undo."));
        } else if let Some(i) = self.muted.iter().position(|k| *k == key) {
            self.muted.remove(i);
            self.add_notice(&format!("Unmuted {name}."));
        } else {
            self.add_notice(&format!("{name} is not muted."));
        }
        true
    }

    /// The sender's colour: ally, enemy, spectator, or plain.
    fn sender_color(&self, team: &str) -> [f32; 4] {
        let side = team == "udon" || team == "soba";
        if team == "spectator" {
            hsla(220.0, 10.0, 55.0, 1.0)
        } else if side && team == self.local_team {
            hsla(320.0, 100.0, 60.0, 1.0)
        } else if side {
            hsla(186.0, 98.0, 58.0, 1.0)
        } else {
            hsla(180.0, 100.0, 50.0, 1.0)
        }
    }

    /// Draws the log above `bottom` from `left`, `.fps-chat`'s place.
    pub fn draw(&self, hud: &mut Hud2d, fonts: &Fonts, left: f32, bottom: f32, width: f32) {
        let s = UI_SCALE;
        let text_color = hsla(180.0, 100.0, 95.0, 1.0);
        let body = fonts
            .body(16.0, text_color)
            .shadow([1.0 * s, 1.0 * s], hsla(250.0, 45.0, 4.0, 0.9));
        let (ascent, descent) = hud.line_metrics(&body);
        let line_h = (ascent + descent) * 1.1;
        let row_h = line_h + 4.0 * s;
        let talking = self.input.is_some();
        let mut y = bottom;
        if let Some(line) = &self.input {
            // The input line: a dark slab, the gold SAY or TEAM stamp, the text.
            let h = row_h + 14.0 * s;
            y -= h;
            Slab {
                clip: &[[0.015, 0.0], [1.0, 0.0], [0.985, 1.0], [0.0, 1.0]],
                background: crate::client::hud::washiki::Gradient::solid(hsla(238.0, 38.0, 8.0, 0.92)),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, left, y, width, h);
            hud.rect(left, y, 4.0 * s, h, hsla(180.0, 100.0, 50.0, 0.4));
            let stamp = fonts
                .display(11.0, hsla(246.0, 38.0, 7.0, 1.0))
                .spacing(11.0 * s * 0.14);
            let label = if self.team_input { "TEAM" } else { "SAY" };
            let stamp_w = hud.measure(label, &stamp) + 16.0 * s;
            let stamp_h = 11.0 * s * 1.3 + 4.0 * s;
            let stamp_x = left + 12.0 * s;
            let stamp_y = y + (h - stamp_h) / 2.0;
            Slab {
                clip: &[[0.08, 0.0], [1.0, 0.0], [0.92, 1.0], [0.0, 1.0]],
                background: crate::client::hud::washiki::Gradient::solid(hsla(45.0, 100.0, 55.0, 1.0)),
                skew_deg: 0.0,
                shadow: None,
            }
            .draw(hud, stamp_x, stamp_y, stamp_w, stamp_h);
            hud.text_in_box(stamp_x + 8.0 * s, stamp_y, stamp_h, label, &stamp);
            let text_x = stamp_x + stamp_w + 8.0 * s;
            let field = fonts.body(16.0, text_color);
            let shown = if line.is_empty() {
                hud.text_in_box(
                    text_x,
                    y,
                    h,
                    "Enter to send · Esc to cancel",
                    &fonts.body(16.0, hsla(220.0, 10.0, 55.0, 1.0)),
                );
                0.0
            } else {
                hud.text_in_box(text_x, y, h, line, &field)
            };
            // The caret, gold, blinking.
            if (self.now % 1.0) < 0.5 {
                hud.rect(
                    text_x + shown + 1.0,
                    y + (h - line_h) / 2.0,
                    2.0 * s,
                    line_h,
                    hsla(45.0, 100.0, 55.0, 1.0),
                );
            }
            y -= 6.0 * s;
        }
        // Which messages, newest last.
        let shown: Vec<&ChatMessage> = if talking {
            self.messages.iter().collect()
        } else {
            let start = self.messages.len().saturating_sub(MAX_VISIBLE);
            self.messages[start..]
                .iter()
                .filter(|m| self.now - m.timestamp < MESSAGE_FADE + FADE_OUT)
                .collect()
        };
        let max_h = if talking { 360.0 } else { 180.0 } * s;
        // Lay out from the newest up, wrapping each message to the width.
        let pad = if talking { 10.0 * s } else { 0.0 };
        let inner = width - 2.0 * pad - 12.0 * s;
        let mut rows: Vec<(f32, &ChatMessage, Vec<String>, f32)> = Vec::new();
        let mut used = 0.0;
        for message in shown.iter().rev() {
            let sender_w = if message.notice {
                0.0
            } else {
                let stamp = self.sender_style(fonts, message);
                hud.measure(&message.sender.to_uppercase(), &stamp) + 8.0 * s
            };
            let lines = wrap(hud, &message.text, &body, inner - sender_w, inner);
            let h = lines.len() as f32 * line_h + 4.0 * s;
            if used + h + 2.0 * s > max_h - 2.0 * pad {
                break;
            }
            used += h + 2.0 * s;
            rows.push((h, message, lines, sender_w));
        }
        if talking && !rows.is_empty() {
            let h = used + 2.0 * pad;
            hud.gradient_rect(
                left,
                y - h,
                width,
                h,
                hsla(238.0, 34.0, 9.0, 0.92),
                hsla(242.0, 36.0, 5.0, 0.92),
            );
            hud.rect(left, y - h, 4.0 * s, h, hsla(180.0, 100.0, 50.0, 0.34));
            hud.outline(left, y - h, width, h, 1.5 * s, hsla(180.0, 100.0, 50.0, 0.24));
            y -= pad;
        }
        for (h, message, lines, sender_w) in rows {
            y -= h + 2.0 * s;
            // `fps-chat-message--fading`: a second's fade after eight.
            let age = self.now - message.timestamp;
            let fade = if talking {
                1.0
            } else {
                (1.0 - ((age - MESSAGE_FADE) / FADE_OUT).clamp(0.0, 1.0)) as f32
            };
            let alpha = 0.95 * fade;
            let x = left + pad + 6.0 * s;
            let baseline = y + 2.0 * s + (line_h - (ascent + descent)) / 2.0 + ascent;
            if !message.notice {
                let mut stamp = self.sender_style(fonts, message);
                stamp.color[3] *= alpha;
                hud.text(x, baseline, &message.sender.to_uppercase(), &stamp);
            }
            let mut style = body;
            if message.notice {
                style.color = hsla(45.0, 100.0, 55.0, 1.0);
            }
            style.color[3] *= alpha;
            if let Some(shadow) = &mut style.shadow {
                shadow.1[3] *= alpha;
            }
            for (i, line) in lines.iter().enumerate() {
                let lx = if i == 0 { x + sender_w } else { x };
                hud.text(lx, baseline + i as f32 * line_h, line, &style);
            }
        }
    }

    fn sender_style(&self, fonts: &Fonts, message: &ChatMessage) -> shagger::ui::hud_2d::TextStyle {
        fonts
            .display(12.0, self.sender_color(&message.team))
            .spacing(12.0 * UI_SCALE * 0.06)
            .align(Align::Left)
    }
}

/// Word-wraps `text`: the first line `first` wide (after the sender), the
/// rest `rest` wide.
fn wrap(hud: &Hud2d, text: &str, style: &shagger::ui::hud_2d::TextStyle, first: f32, rest: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        let limit = if lines.is_empty() { first } else { rest };
        if hud.measure(&candidate, style) <= limit || line.is_empty() {
            line = candidate;
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        }
    }
    lines.push(line);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sends_says_and_team_lines_and_keeps_mutes_local() {
        let mut chat = Chat::default();
        chat.open_input(false);
        chat.type_text("gg wp");
        assert_eq!(chat.submit(), Some(("gg wp".to_owned(), false)));
        assert!(!chat.is_input_active());
        chat.open_input(false);
        chat.type_text("/say_team push b");
        assert_eq!(chat.submit(), Some(("push b".to_owned(), true)));
        chat.add_message("Ramen", "hi", "ffa");
        chat.open_input(false);
        chat.type_text("/mute ramen");
        assert_eq!(chat.submit(), None);
        chat.add_message("Ramen", "still here", "ffa");
        assert!(chat.messages.iter().all(|m| m.notice));
        // No team: team chat refuses with a notice.
        chat.open_input(true);
        assert!(!chat.is_input_active());
    }
}
