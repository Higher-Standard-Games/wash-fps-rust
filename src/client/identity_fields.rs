//! wash-fps `client/IdentityFields.ts` and the slot in `styles/Identity.css`:
//! the name and key fields the Welcome screen and the Profile page share. A
//! slot cut into the surface, a line that says how it's going, and the
//! judgements the lobby will make, made here first.

use shagger::lobby::player_identity::{KEY_LENGTH, NameProblem, check_player_name, groups_of_four, key_characters};
use shagger::ui::components::lobby_twin::{gradient_polygon, hsl, hsla, text, violet};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, FontId, Painter, Rect, Shape, Stroke, pos2, vec2};

use crate::client::settings::settings_fields::{body, gold_text};
use crate::client::washoi_style::display_family;
use crate::shared::bot::bot_names::is_bot_name;

/// `SayTone`: plain, good or wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Plain,
    Ok,
    Bad,
}

/// `Slot`: what is typed and the status line under it.
#[derive(Debug, Clone, Default)]
pub struct Slot {
    pub value: String,
    pub say: String,
    pub tone: Tone,
}

impl Slot {
    #[must_use]
    pub fn new(say: &str) -> Self {
        Self {
            value: String::new(),
            say: say.to_owned(),
            tone: Tone::Plain,
        }
    }

    /// `tell`: say how it's going.
    pub fn tell(&mut self, text: impl Into<String>, tone: Tone) {
        self.say = text.into();
        self.tone = tone;
    }
}

/// `createSlot`'s options.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotLook<'a> {
    pub placeholder: &'a str,
    /// `WASHOI-` before a key.
    pub prefix: Option<&'a str>,
    pub max_chars: Option<usize>,
    /// Shows `n / max`.
    pub counted: bool,
}

/// What a slot did this frame.
#[derive(Debug, Clone, Copy, Default)]
pub struct SlotResponse {
    pub changed: bool,
    pub submitted: bool,
    pub bottom: f32,
}

/// `.identity-slot` on the gold panel, `width` wide from `left`, `top`:
/// the box (84px, 72 on a short screen) and the status line.
#[allow(clippy::too_many_arguments)]
pub fn show_slot(
    ui: &mut egui::Ui,
    id: &str,
    left: f32,
    top: f32,
    width: f32,
    short: bool,
    slot: &mut Slot,
    look: SlotLook<'_>,
    enabled: bool,
) -> SlotResponse {
    let painter = ui.painter().clone();
    let h = if short { 72.0 } else { 84.0 };
    let r = Rect::from_min_size(pos2(left, top), vec2(width, h));
    // `clip-path: polygon(2.4% 0, 100% 0, 97.6% 100%, 0 100%)`.
    let cut = width * 0.024;
    let shape = [
        pos2(r.left() + cut, r.top()),
        r.right_top(),
        pos2(r.right() - cut, r.bottom()),
        r.left_bottom(),
    ];
    gradient_polygon(
        &painter,
        &shape,
        &[(0.0, hsl(250.0, 42.0, 4.0)), (1.0, hsl(250.0, 34.0, 12.0))],
    );
    // `box-shadow: inset 0 7px 0 hsla(0, 0%, 0%, 0.5)`.
    let lip = [
        shape[0],
        shape[1],
        shape[1].lerp(shape[2], 7.0 / h),
        shape[0].lerp(shape[3], 7.0 / h),
    ];
    painter.add(Shape::convex_polygon(
        lip.to_vec(),
        hsla(0.0, 0.0, 0.0, 0.5),
        Stroke::NONE,
    ));

    // Padding 0 28 0 26, items 16 apart: prefix, input, count.
    let mut x = r.left() + 26.0;
    let mid = r.center().y;
    if let Some(prefix) = look.prefix {
        let g = text(
            &painter,
            prefix,
            18.0,
            display_family(),
            hsla(46.0, 100.0, 60.0, 0.45),
            18.0 * 0.06,
        );
        let w = g.size().x;
        painter.galley(pos2(x, mid - g.size().y / 2.0), g, Color32::WHITE);
        x += w + 16.0;
    }
    let mut right = r.right() - 28.0;
    if look.counted
        && let Some(max) = look.max_chars
    {
        let count = format!("{} / {max}", slot.value.trim().chars().count());
        let g = text(
            &painter,
            &count,
            14.0,
            body(),
            hsla(45.0, 100.0, 95.0, 0.38),
            14.0 * 0.12,
        );
        right -= g.size().x;
        painter.galley(pos2(right, mid - g.size().y / 2.0), g, Color32::WHITE);
        right -= 16.0;
    }
    // `.welcome .identity-slot__input`: 34px on the gold, a key's included
    // (the rule comes after the key's own size); a key is spaced wider.
    let (size, em) = (34.0, if look.prefix.is_some() { 0.08 } else { 0.04 });
    let input = Rect::from_min_max(pos2(x, r.top()), pos2(right, r.bottom()));
    let edit_id = egui::Id::NULL.with(("identity-slot", id));
    let before = slot.value.clone();
    let color = if enabled {
        gold_text()
    } else {
        gold_text().gamma_multiply(0.6)
    };
    let mut layouter = move |ui: &egui::Ui, s: &dyn egui::TextBuffer, _wrap: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            s.as_str(),
            0.0,
            egui::TextFormat {
                font_id: FontId::new(size, display_family()),
                color,
                extra_letter_spacing: size * em,
                ..Default::default()
            },
        );
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let mut edit = egui::TextEdit::singleline(&mut slot.value)
        .id(edit_id)
        .frame(egui::Frame::NONE)
        .font(FontId::new(size, display_family()))
        .text_color(color)
        .layouter(&mut layouter)
        .align(egui::Align2::LEFT_CENTER)
        .desired_width(input.width());
    // `::placeholder`, cut off at the box's edge as the browser's is.
    if before.is_empty() && !look.placeholder.is_empty() {
        let g = text(
            &painter,
            look.placeholder,
            size,
            display_family(),
            hsla(46.0, 100.0, 60.0, 0.22),
            size * em,
        );
        painter
            .with_clip_rect(input)
            .galley(pos2(input.left(), mid - g.size().y / 2.0), g, Color32::WHITE);
    }
    if let Some(max) = look.max_chars {
        edit = edit.char_limit(max);
    }
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            ui.visuals_mut().text_cursor.stroke = Stroke::new(2.0, violet());
            ui.put(input, edit)
        })
        .inner;
    if response.has_focus() {
        // `:focus-within::after`: the violet line along the foot.
        painter.rect_filled(
            Rect::from_min_max(pos2(r.left(), r.bottom() - 4.0), pos2(r.right() - cut, r.bottom())),
            0.0,
            violet(),
        );
    }
    let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

    // `.identity-say`, 8 under the box.
    let say_top = r.bottom() + 8.0;
    let bottom = paint_say(&painter, left, say_top, width, &slot.say, slot.tone);
    SlotResponse {
        changed: slot.value != before,
        submitted,
        bottom: bottom.max(say_top + 20.0),
    }
}

/// `.welcome .identity-say`: 15px uppercase, dark on the gold; a good word
/// starts with a tick. Returns its bottom.
pub fn paint_say(painter: &Painter, left: f32, top: f32, width: f32, say: &str, tone: Tone) -> f32 {
    if say.is_empty() {
        return top;
    }
    let color = match tone {
        Tone::Plain => hsla(250.0, 45.0, 8.0, 0.6),
        Tone::Ok => hsl(142.0, 80.0, 20.0),
        Tone::Bad => hsl(354.0, 80.0, 36.0),
    };
    let mut x = left;
    if tone == Tone::Ok {
        // `::before { content: "✓ " }`, drawn: the face has no tick.
        let y = top + 9.0;
        painter.add(Shape::line(
            vec![pos2(x, y), pos2(x + 4.0, y + 4.0), pos2(x + 11.0, y - 4.0)],
            Stroke::new(2.4, color),
        ));
        x += 18.0;
    }
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (width - (x - left)).max(1.0);
    job.append(
        &say.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: FontId::new(15.0, FontFamily::Proportional),
            color,
            extra_letter_spacing: 15.0 * 0.08,
            ..Default::default()
        },
    );
    let g = painter.layout_job(job);
    let h = g.size().y;
    painter.galley(pos2(x, top), g, color);
    top + h
}

/// `judgeName`: what to say about a name being typed, and whether it can be sent.
#[must_use]
pub fn judge_name(name: &str) -> (bool, String, Tone) {
    if name.is_empty() {
        return (false, "2–16 characters: letters, numbers, _ and -".into(), Tone::Plain);
    }
    match check_player_name(name, Some(&is_bot_name)) {
        Some(NameProblem::Invalid) if name.chars().count() < 2 => (false, "One more character".into(), Tone::Plain),
        Some(NameProblem::Invalid) => (false, "Letters, numbers, _ and - only".into(), Tone::Bad),
        Some(NameProblem::Reserved) => (false, format!("{name} is kept for bots and new players"), Tone::Bad),
        None => (true, format!("{name} looks good"), Tone::Ok),
    }
}

/// `wireKeySlot`: keeps a key slot tidy as it's typed, canonical characters
/// in groups of four and a count until the key is whole. Returns the key so
/// far and whether it is whole.
pub fn tidy_key_slot(slot: &mut Slot) -> (String, bool) {
    let key = key_characters(&slot.value);
    slot.value = groups_of_four(&key).join("-");
    let whole = key.chars().count() == KEY_LENGTH;
    if whole {
        slot.tell("That's a whole key", Tone::Ok);
    } else {
        slot.tell(
            format!("{} / {KEY_LENGTH} characters", key.chars().count()),
            Tone::Plain,
        );
    }
    (key, whole)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_judged_before_the_lobby_sees_them() {
        assert!(!judge_name("").0);
        assert_eq!(judge_name("a").1, "One more character");
        assert_eq!(judge_name("a b").2, Tone::Bad);
        assert_eq!(judge_name("Ramen_3").1, "Ramen_3 is kept for bots and new players");
        assert_eq!(judge_name("Player_42").2, Tone::Bad);
        assert_eq!(judge_name("Tarmo"), (true, "Tarmo looks good".into(), Tone::Ok));
    }

    #[test]
    fn a_key_slot_groups_what_is_typed() {
        let mut slot = Slot::new("");
        slot.value = "washoi-0123 4567o".into();
        let (key, whole) = tidy_key_slot(&mut slot);
        assert_eq!((key.as_str(), whole), ("012345670", false));
        assert_eq!(slot.value, "0123-4567-0");
        assert_eq!(slot.say, "9 / 24 characters");
        slot.value = "0123456789ABCDEFGHJKMNPQ".into();
        assert!(tidy_key_slot(&mut slot).1);
        assert_eq!(slot.tone, Tone::Ok);
    }
}
