//! wash-fps `client/JoinLobbyDialog.ts` and `styles/JoinLobbyDialog.css`:
//! one field that takes a room link or a room code, and joins that room. A
//! room in another region needs that region's lobby, where the player has
//! a separate account, so the dialog says so before switching.
//!
//! A small card in the welcome guide's style (`lobby_welcome_dialog`).
//! Switching region is the panel's to do: the browser goes to the other
//! region's site with the room in its address; this client starts that
//! region's lobby again with the room as its `--join` link.

use shagger::ui::components::lobby_twin::{hsl, hsla, text};
use shagger::ui::components::raised_key::{KeyLook, KeyText, RaisedKey};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontId, Rect, Stroke, pos2, vec2};

use crate::client::lobby_code::{CodeTarget, RoomRef, describe, in_this_region, parse_room_code};
use crate::client::lobby_welcome_dialog::{card_rect, ink_key, ink_key_width, paint_card, veil};
use crate::client::settings::settings_fields::body;
use crate::client::washoi_style::display_family;
use crate::regions::Region;

/// What the dialog asks of the lobby.
#[derive(Debug, Clone, PartialEq)]
pub enum JoinAnswer {
    /// Cancel, Escape or a click beside the card.
    Close,
    /// A room in this region: join it, then close.
    Join(RoomRef),
    /// A room in another region: start that region's lobby with it.
    Switch(RoomRef),
}

/// `--ink`.
fn ink(a: f32) -> Color32 {
    hsla(250.0, 45.0, 7.0, a)
}

/// `JoinLobbyDialog`.
pub struct JoinLobbyDialog {
    text: String,
    target: Option<CodeTarget>,
    /// The field takes the keyboard when the dialog opens.
    focus: bool,
}

impl JoinLobbyDialog {
    /// Empty, or with a pasted code or link in it.
    #[must_use]
    pub fn new(text: Option<String>) -> Self {
        Self {
            text: text.unwrap_or_default(),
            target: None,
            focus: true,
        }
    }

    /// The room the field names, if any.
    fn room(&self) -> Option<&RoomRef> {
        match &self.target {
            Some(CodeTarget::Room(room)) => Some(room),
            _ => None,
        }
    }

    /// `go`: join here, or switch region for a room elsewhere.
    fn go(&self, here: Option<&Region>) -> Option<JoinAnswer> {
        let room = self.room()?.clone();
        Some(if in_this_region(&room, here) {
            JoinAnswer::Join(room)
        } else {
            JoinAnswer::Switch(room)
        })
    }

    /// The dialog over `screen`. `regions` are the ones a code may name;
    /// `here` is the region this client plays in.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        regions: &[Region],
        here: Option<&Region>,
    ) -> Option<JoinAnswer> {
        // `read`: the field's text, as it stands.
        self.target = parse_room_code(&self.text, regions, None);
        let w = screen.width();
        let (pad_x, pad_top, pad_bottom, gap) = (28.0, 26.0, 24.0, 14.0);
        let field_h = 12.0 + 18.0 * 1.4 + 12.0 + 4.0;
        // Two lines: the region message wraps at this width.
        let status_h = 16.0 * 1.35 * 2.0;
        let height = pad_top + 33.0 + gap + field_h + gap + status_h + gap + 60.0 + 6.0 + pad_bottom;
        let card = card_rect(screen, 560.0, height);
        let mut answer = None;
        if veil(ui, screen, "join-lobby", card) {
            answer = Some(JoinAnswer::Close);
        }
        let painter = ui.painter().clone();
        let _ = paint_card(&painter, card, w);
        let inner = Rect::from_min_max(
            card.min + vec2(6.0 + pad_x, pad_top),
            card.max - vec2(pad_x, pad_bottom),
        );
        // `.join-lobby__title`.
        let title = text(&painter, "Join lobby", 30.0, display_family(), ink(1.0), 0.0);
        let mut y = inner.top();
        painter.galley(pos2(inner.left(), y), title.clone(), Color32::WHITE);
        y += 33.0f32.max(title.size().y) + gap;

        // `.join-lobby__field`: cream, an ink border, ink text.
        let field = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), field_h));
        let edit_id = egui::Id::NULL.with("join-lobby-field");
        let focused = ui.ctx().memory(|m| m.has_focus(edit_id));
        painter.rect_filled(field, 4.0, Color32::from_rgb(0xff, 0xfd, 0xf5));
        painter.rect_stroke(field, 4.0, Stroke::new(2.0, ink(1.0)), egui::StrokeKind::Inside);
        if focused {
            painter.rect_stroke(
                field.expand(2.0),
                5.0,
                Stroke::new(3.0, hsl(276.0, 90.0, 64.0)),
                egui::StrokeKind::Outside,
            );
        }
        let input = field.shrink2(vec2(14.0, 0.0));
        let edit = egui::TextEdit::singleline(&mut self.text)
            .id(edit_id)
            .frame(egui::Frame::NONE)
            .font(FontId::new(18.0, body()))
            .text_color(ink(1.0))
            .hint_text(egui::RichText::new("Room link or code").color(ink(0.45)))
            .char_limit(400)
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(input.width());
        let response = ui
            .scope(|ui| {
                ui.visuals_mut().text_cursor.stroke = Stroke::new(2.0, ink(1.0));
                ui.visuals_mut().selection.bg_fill = hsla(276.0, 90.0, 64.0, 0.35);
                ui.put(input, edit)
            })
            .inner;
        if std::mem::take(&mut self.focus) {
            response.request_focus();
        }
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if response.changed() {
            self.target = parse_room_code(&self.text, regions, None);
        }
        y += field_h + gap;

        // `.join-lobby__status`: what the text names, once there is any.
        if !self.text.trim().is_empty() {
            let mut job = egui::text::LayoutJob::default();
            job.wrap.max_width = inner.width();
            job.append(
                &describe(self.target.as_ref(), here),
                0.0,
                egui::TextFormat {
                    font_id: FontId::new(16.0, body()),
                    color: ink(1.0),
                    ..Default::default()
                },
            );
            painter.galley(pos2(inner.left(), y), painter.layout_job(job), Color32::WHITE);
        }
        y += status_h + gap;

        // `.join-lobby__actions`: Join (or Switch to US) and Cancel.
        let room = self.room().cloned();
        let join_label = match &room {
            Some(r) if !in_this_region(r, here) => {
                format!("Switch to {}", r.region.as_ref().map_or("", |g| g.code.as_str()))
            }
            _ => "Join".to_owned(),
        };
        let join_w = ink_key_width(&painter, &join_label, 0.0);
        if (ink_key(ui, "join-lobby", inner.left(), y, join_w, &join_label, room.is_none()) || entered)
            && let Some(a) = self.go(here)
        {
            answer = Some(a);
        }
        let cancel_left = inner.left() + join_w + 12.0;
        let g = text(&painter, "CANCEL", 20.0, display_family(), Color32::WHITE, 20.0 * 0.07);
        let cancel = Rect::from_min_size(pos2(cancel_left, y), vec2(g.size().x + 48.0, 60.0));
        let face = RaisedKey::new("join-lobby-cancel", cancel, KeyLook::cream()).show(ui);
        KeyText::new("Cancel", "", display_family(), body())
            .size(20.0, 13.0)
            .spacing(0.07)
            .pad(26.0, 22.0)
            .paint(&painter, &face);
        if face.clicked {
            answer = Some(JoinAnswer::Close);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            answer = Some(JoinAnswer::Close);
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regions::builtin;

    fn run(dialog: &mut JoinLobbyDialog, here: Option<&Region>) -> Option<JoinAnswer> {
        let ctx = egui::Context::default();
        let mut answer = None;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            answer = dialog.ui(ui, ui.max_rect(), &builtin(), here);
        });
        out.textures_delta.clear();
        answer
    }

    #[test]
    fn a_room_here_is_joined_and_one_elsewhere_switches_region() {
        let regions = builtin();
        let eu = &regions[0];
        let mut here = JoinLobbyDialog::new(Some("eu:3fa9c2d1:tok".into()));
        assert_eq!(run(&mut here, Some(eu)), None);
        match here.go(Some(eu)) {
            Some(JoinAnswer::Join(room)) => {
                assert_eq!(room.lobby_id, "3fa9c2d1");
                assert_eq!(room.invite.as_deref(), Some("tok"));
            }
            other => panic!("{other:?}"),
        }
        let mut away = JoinLobbyDialog::new(Some("us:3fa9c2d1".into()));
        run(&mut away, Some(eu));
        assert!(matches!(away.go(Some(eu)), Some(JoinAnswer::Switch(_))));
        let mut nothing = JoinLobbyDialog::new(Some("not a room".into()));
        run(&mut nothing, Some(eu));
        assert_eq!(nothing.go(Some(eu)), None);
    }
}
