//! wash-fps `client/CampaignBoardPane.ts` and `styles/Leaderboard.css`'s
//! `lb-camp` rules, with `Campaign.css`'s `.campaign__board` sizes: the
//! campaign's boards on the Campaign page. The top scorers over the whole
//! campaign, the first to finish it, and each operation's top fraggers and
//! first clear. Every number is the lobby's; names are the accounts' names
//! now.

use serde_json::Value;
use shagger::ui::components::lobby_twin::{elide, gradient_polygon, hsl, hsla, text};
use shagger::ui::egui_layer::egui;

use egui::{Color32, FontFamily, Painter, Rect, Shape, Stroke, pos2, vec2};

use crate::client::campaign::campaign_story::campaign_story;
use crate::client::campaign::campaign_view::{PlateLook, campaign_plate_in};
use crate::client::washoi_style::display_family;

/// `CampaignBoardRow`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CampaignBoardRow {
    pub rank: u32,
    pub player_id: String,
    pub name: String,
    /// Frags: summed over the cleared missions on the scorers' board, one
    /// mission's best run on a mission's.
    pub frags: Option<u32>,
    /// Missions cleared, on the scorers' board.
    pub cleared: Option<u32>,
    /// When: the campaign finished, or the mission first cleared (ms since
    /// the epoch).
    pub at: Option<f64>,
}

/// `CampaignBoardMission`: one mission's top fraggers and first clears.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CampaignBoardMission {
    pub id: String,
    pub number: u32,
    pub scorers: Vec<CampaignBoardRow>,
    pub first_clears: Vec<CampaignBoardRow>,
}

/// `CampaignBoard`: the lobby's `campaign:board` answer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CampaignBoard {
    /// Missions the campaign has, for "6 / 8".
    pub mission_count: u32,
    pub scorers: Vec<CampaignBoardRow>,
    pub finishers: Vec<CampaignBoardRow>,
    pub missions: Vec<CampaignBoardMission>,
}

fn read_row(v: &Value) -> Option<CampaignBoardRow> {
    let num = |k: &str| v.get(k).and_then(Value::as_f64);
    Some(CampaignBoardRow {
        rank: num("rank")? as u32,
        player_id: v.get("playerId")?.as_str()?.to_owned(),
        name: v.get("name")?.as_str()?.to_owned(),
        frags: num("frags").map(|f| f as u32),
        cleared: num("cleared").map(|c| c as u32),
        at: num("at"),
    })
}

fn read_rows(v: Option<&Value>) -> Vec<CampaignBoardRow> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(read_row).collect())
        .unwrap_or_default()
}

/// Reads a `campaign:board` message.
#[must_use]
pub fn read_board(message: &Value) -> Option<CampaignBoard> {
    let missions = message
        .get("missions")?
        .as_array()?
        .iter()
        .filter_map(|m| {
            Some(CampaignBoardMission {
                id: m.get("id")?.as_str()?.to_owned(),
                number: m.get("number").and_then(Value::as_u64).unwrap_or(0) as u32,
                scorers: read_rows(m.get("scorers")),
                first_clears: read_rows(m.get("firstClears")),
            })
        })
        .collect();
    Some(CampaignBoard {
        mission_count: message.get("missionCount")?.as_u64()? as u32,
        scorers: read_rows(message.get("scorers")),
        finishers: read_rows(message.get("finishers")),
        missions,
    })
}

/// `day`: a date as a day in the player's own time zone, "14 Nov 2026"
/// (`toLocaleDateString` with a numeric day, short month and year).
#[must_use]
pub fn day(at: Option<f64>) -> String {
    day_in(at, &jiff::tz::TimeZone::system())
}

/// [`day`] in the time zone `tz`.
fn day_in(at: Option<f64>, tz: &jiff::tz::TimeZone) -> String {
    let Some(at) = at.filter(|v| v.is_finite()) else {
        return String::new();
    };
    jiff::Timestamp::from_millisecond(at.floor() as i64).map_or_else(
        |_| String::new(),
        |t| t.to_zoned(tz.clone()).strftime("%-d %b %Y").to_string(),
    )
}

/// Each operation's card shows this many of its top fraggers.
const OPERATION_ROWS: usize = 3;

/// `CampaignBoardState`.
#[derive(Debug, Clone, PartialEq)]
pub enum CampaignBoardState {
    Loading,
    Failed(String),
    Ready(CampaignBoard),
}

fn cream(a: f32) -> Color32 {
    hsla(45.0, 100.0, 95.0, a)
}

fn ink() -> Color32 {
    hsl(250.0, 45.0, 7.0)
}

fn gold_text() -> Color32 {
    hsl(46.0, 100.0, 60.0)
}

fn body() -> FontFamily {
    FontFamily::Proportional
}

/// `--accent` on the campaign's page: the leaderboard's purple.
fn accent() -> Color32 {
    hsl(276.0, 90.0, 72.0)
}

fn edge() -> Color32 {
    hsla(322.0, 95.0, 62.0, 0.3)
}

/// The panel behind a board or a card: `polygon(0 0, 100% - cut 0, 100% cut, 100% 100%, 0 100%)`.
fn cut_box(painter: &Painter, r: Rect, cut: f32) {
    let points = vec![
        r.left_top(),
        pos2(r.right() - cut, r.top()),
        pos2(r.right(), r.top() + cut),
        r.right_bottom(),
        r.left_bottom(),
    ];
    painter.add(Shape::convex_polygon(
        points.clone(),
        hsla(262.0, 50.0, 7.0, 0.72),
        Stroke::NONE,
    ));
    let mut closed = points;
    closed.push(r.left_top());
    painter.add(Shape::line(closed, Stroke::new(1.5, edge())));
}

/// `CampaignBoardPane`.
#[derive(Default)]
pub struct CampaignBoardPane;

/// One list's columns: their titles and widths.
struct Columns {
    titles: &'static [&'static str],
    widths: &'static [f32],
}

const ROW_H: f32 = 56.0;
const HEAD_ROW_H: f32 = 40.0;
const TITLE_H: f32 = 22.0 + 22.0 + 12.0;

impl CampaignBoardPane {
    /// `render`: the pane over `rect`'s width from `top`, as it stands;
    /// returns its height. `you` is the account on this computer.
    pub fn paint(&self, painter: &Painter, rect: Rect, state: &CampaignBoardState, you: Option<&str>) -> f32 {
        let board = match state {
            CampaignBoardState::Loading => return status(painter, rect, "Reading the campaign boards…"),
            CampaignBoardState::Failed(reason) => return status(painter, rect, reason),
            CampaignBoardState::Ready(board) => board,
        };
        if board.scorers.is_empty() {
            return status(painter, rect, "No one has cleared an operation yet.");
        }
        // `lb-camp__lists`: 3fr and 2fr, clear of the close key.
        let lists_w = rect.width() - 84.0;
        let gap = 30.0;
        let left_w = (lists_w - gap) * 0.6;
        let right_w = lists_w - gap - left_w;
        let scorers = Rect::from_min_size(rect.min, vec2(left_w, 0.0));
        let finishers = Rect::from_min_size(pos2(rect.left() + left_w + gap, rect.top()), vec2(right_w, 0.0));
        let count = board.mission_count;
        let a = list(
            painter,
            scorers,
            "Top scorers",
            &board.scorers,
            &|row| {
                vec![
                    row.frags.unwrap_or(0).to_string(),
                    format!("{} / {count}", row.cleared.unwrap_or(0)),
                ]
            },
            &Columns {
                titles: &["Frags", "Cleared"],
                widths: &[110.0, 110.0],
            },
            "",
            you,
        );
        let b = list(
            painter,
            finishers,
            "First to finish",
            &board.finishers,
            &|row| vec![day(row.at)],
            &Columns {
                titles: &["Finished"],
                widths: &[150.0],
            },
            "Nobody has finished it yet.",
            you,
        );
        let mut y = rect.top() + a.max(b) + 34.0;
        // `lb-camp__ops`: `repeat(auto-fill, minmax(340px, 1fr))`, 40 by 30.
        let ops = Rect::from_min_max(
            pos2(rect.left() + 4.0, y + 10.0),
            pos2(rect.right() - 4.0, rect.bottom()),
        );
        let (col_gap, row_gap) = (30.0, 40.0);
        let columns = (((ops.width() + col_gap) / (340.0 + col_gap)).floor() as usize).max(1);
        let card_w = (ops.width() - col_gap * (columns as f32 - 1.0)) / columns as f32;
        y = ops.top();
        for chunk in board.missions.chunks(columns) {
            let h = chunk.iter().map(operation_height).fold(0.0, f32::max);
            for (i, mission) in chunk.iter().enumerate() {
                let r = Rect::from_min_size(pos2(ops.left() + i as f32 * (card_w + col_gap), y), vec2(card_w, h));
                operation(painter, r, mission, you);
            }
            y += h + row_gap;
        }
        y - row_gap + 20.0 - rect.top()
    }
}

/// `lb-status`: the pane's one line while it has no boards.
fn status(painter: &Painter, rect: Rect, words: &str) -> f32 {
    let g = painter.layout(
        words.to_owned(),
        egui::FontId::new(18.0, body()),
        cream(0.75),
        (rect.width() - 100.0).max(40.0),
    );
    let h = g.size().y;
    painter.galley(pos2(rect.left() + 4.0, rect.top() + 14.0), g, cream(0.75));
    h + 28.0
}

/// `list`: a board's title, its head row and its rows; returns its height.
#[allow(clippy::too_many_arguments)]
fn list(
    painter: &Painter,
    at: Rect,
    title: &str,
    rows: &[CampaignBoardRow],
    values: &dyn Fn(&CampaignBoardRow) -> Vec<String>,
    columns: &Columns,
    empty: &str,
    you: Option<&str>,
) -> f32 {
    let body_h = if rows.is_empty() {
        18.0 + 24.0 + 22.0
    } else {
        rows.len() as f32 * ROW_H
    };
    let h = TITLE_H + HEAD_ROW_H + body_h;
    let r = Rect::from_min_size(at.min, vec2(at.width(), h));
    // `lb-board`: the purple panel, its corner cut.
    let points = vec![
        r.left_top(),
        pos2(r.right() - 20.0, r.top()),
        pos2(r.right(), r.top() + 20.0),
        r.right_bottom(),
        r.left_bottom(),
    ];
    painter.add(Shape::convex_polygon(
        points.clone(),
        hsla(262.0, 50.0, 7.0, 0.72),
        Stroke::NONE,
    ));
    let mut closed = points;
    closed.push(r.left_top());
    painter.add(Shape::line(closed, Stroke::new(1.5, edge())));
    // `lb-camp__head`.
    let g = text(
        painter,
        &title.to_uppercase(),
        22.0,
        display_family(),
        cream(1.0),
        22.0 * 0.12,
    );
    painter.galley(pos2(r.left() + 22.0, r.top() + 22.0), g, cream(1.0));
    let mut y = r.top() + TITLE_H;
    // The columns from the right: the values, right-aligned.
    let pad_l = 22.0;
    let pad_r = 26.0;
    let mut rights = Vec::new();
    let mut right = r.right() - pad_r;
    for w in columns.widths.iter().rev() {
        rights.push(right);
        right -= w;
    }
    rights.reverse();
    let who_right = right - 8.0;
    // `lb-row--head`.
    let head = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), HEAD_ROW_H));
    painter.hline(
        head.x_range(),
        head.bottom() - 0.5,
        Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.25)),
    );
    let head_c = cream(0.45);
    let cell = |s: &str, x: f32, align_right: bool, color: Color32| {
        let g = text(painter, &s.to_uppercase(), 12.0, body(), color, 12.0 * 0.2);
        let x = if align_right { x - g.size().x } else { x };
        painter.galley(pos2(x, head.center().y - g.size().y / 2.0), g, color);
    };
    cell("Rank", r.left() + pad_l, false, head_c);
    cell("Player", r.left() + pad_l + 64.0, false, head_c);
    for (i, title) in columns.titles.iter().enumerate() {
        cell(title, rights[i], true, if i == 0 { accent() } else { head_c });
    }
    y = head.bottom();
    if rows.is_empty() {
        let g = text(painter, empty, 18.0, body(), cream(0.6), 0.0);
        painter.galley(pos2(r.left() + 26.0, y + 18.0), g, cream(0.6));
    }
    for row in rows {
        let line = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), ROW_H));
        let is_you = you == Some(row.player_id.as_str());
        if is_you {
            let band = [
                line.left_top(),
                line.right_top(),
                line.right_bottom(),
                line.left_bottom(),
            ];
            shagger::ui::components::lobby_twin::gradient_polygon_x(
                painter,
                &band,
                &[
                    (0.0, hsla(46.0, 100.0, 60.0, 0.24)),
                    (0.7, hsla(46.0, 100.0, 60.0, 0.05)),
                    (1.0, hsla(46.0, 100.0, 60.0, 0.05)),
                ],
            );
            painter.rect_filled(Rect::from_min_size(line.min, vec2(5.0, ROW_H)), 0.0, gold_text());
        }
        painter.hline(
            line.x_range(),
            line.bottom() - 0.5,
            Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.1)),
        );
        let cy = line.center().y;
        let rank_c = match row.rank {
            1 => hsl(48.0, 100.0, 62.0),
            2 => hsl(220.0, 25.0, 84.0),
            3 => hsl(26.0, 85.0, 64.0),
            _ => cream(1.0),
        };
        let g = text(painter, &format!("#{}", row.rank), 22.0, display_family(), rank_c, 0.0);
        painter.galley(pos2(r.left() + pad_l, cy - g.size().y / 2.0), g, rank_c);
        let mut x = r.left() + pad_l + 64.0;
        let tag_w = if is_you { 52.0 } else { 0.0 };
        let name = elide(
            painter,
            &row.name,
            19.0,
            display_family(),
            (who_right - x - tag_w).max(20.0),
        );
        let g = text(painter, &name, 19.0, display_family(), cream(1.0), 19.0 * 0.03);
        let w = g.size().x;
        painter.galley(pos2(x, cy - g.size().y / 2.0), g, cream(1.0));
        x += w + 12.0;
        if is_you {
            you_tag(painter, pos2(x, cy));
        }
        for (i, value) in values(row).iter().enumerate() {
            let color = if i == 0 { cream(1.0) } else { cream(0.88) };
            let g = text(painter, value, 20.0, body(), color, 0.0);
            painter.galley(pos2(rights[i] - g.size().x, cy - g.size().y / 2.0), g, color);
        }
        y += ROW_H;
    }
    h
}

/// `lb-row__you`: the gold tag, its left middle at `at`.
fn you_tag(painter: &Painter, at: egui::Pos2) {
    let g = text(painter, "YOU", 10.0, display_family(), ink(), 1.2);
    let b = Rect::from_min_size(pos2(at.x, at.y - 8.0), vec2(g.size().x + 14.0, 16.0));
    let c = b.width() * 0.1;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(b.left() + c, b.top()),
            b.right_top(),
            pos2(b.right() - c, b.bottom()),
            b.left_bottom(),
        ],
        gold_text(),
        Stroke::NONE,
    ));
    painter.galley(b.center() - g.size() / 2.0, g, ink());
}

/// The plate's height, 24px type on the dark plate.
const OP_TITLE_H: f32 = 40.0;
const OP_ROW_H: f32 = 26.0;

fn operation_height(mission: &CampaignBoardMission) -> f32 {
    if mission.scorers.is_empty() {
        return OP_TITLE_H + 16.0 + 18.0 + 24.0 + 22.0;
    }
    let rows = mission.scorers.len().min(OPERATION_ROWS) as f32;
    let mut h = OP_TITLE_H + 16.0 + 4.0 + rows * OP_ROW_H + (rows - 1.0) * 10.0;
    if !mission.first_clears.is_empty() {
        h += 16.0 + 4.0 + 14.0 + 20.0;
    }
    h + 24.0
}

/// `operation`: its name on the campaign's plate, its top fraggers, and who
/// cleared it first.
fn operation(painter: &Painter, r: Rect, mission: &CampaignBoardMission, you: Option<&str>) {
    cut_box(painter, r, 16.0);
    let title = campaign_story(&mission.id).map_or(mission.id.as_str(), |s| s.title);
    let clip = painter.with_clip_rect(r.intersect(painter.clip_rect()));
    let plate = campaign_plate_in(&clip, r.min, &title.to_uppercase(), 24.0, PlateLook::Dark);
    let mut y = r.top() + OP_TITLE_H.max(plate.height()) + 16.0;
    if mission.scorers.is_empty() {
        let g = text(painter, "Not cleared yet", 18.0, body(), cream(0.6), 0.0);
        painter.galley(pos2(r.left() + 26.0, y + 2.0), g, cream(0.6));
        return;
    }
    y += 4.0;
    let left = r.left() + 26.0;
    let right = r.right() - 26.0;
    for (i, row) in mission.scorers.iter().take(OPERATION_ROWS).enumerate() {
        let is_you = you == Some(row.player_id.as_str());
        let color = if is_you { gold_text() } else { cream(1.0) };
        let cy = y + OP_ROW_H / 2.0;
        let place_c = if i == 0 { hsl(48.0, 100.0, 62.0) } else { cream(0.55) };
        let g = text(painter, &format!("#{}", i + 1), 16.0, display_family(), place_c, 0.0);
        painter.galley(pos2(left, cy - g.size().y / 2.0), g, place_c);
        let frags = text(
            painter,
            &row.frags.unwrap_or(0).to_string(),
            22.0,
            display_family(),
            color,
            0.0,
        );
        let fw = frags.size().x;
        painter.galley(pos2(right - fw, cy - frags.size().y / 2.0), frags, color);
        let name_x = left + 44.0 + 10.0;
        let name = elide(painter, &row.name, 20.0, body(), (right - fw - 10.0 - name_x).max(20.0));
        let g = text(painter, &name, 20.0, body(), color, 0.0);
        painter.galley(pos2(name_x, cy - g.size().y / 2.0), g, color);
        y += OP_ROW_H + 10.0;
    }
    if let Some(first) = mission.first_clears.first() {
        y += 4.0 - 10.0 + 4.0;
        painter.hline(left..=right, y, Stroke::new(1.0, hsla(276.0, 90.0, 64.0, 0.2)));
        y += 14.0;
        // "★ name · date"; the star is drawn, the body face has none.
        let g = text(painter, "Mg", 17.0, body(), cream(0.75), 0.0);
        let mid = y + g.size().y / 2.0;
        star(painter, pos2(left + 7.0, mid), 7.5, cream(0.75));
        let line = format!("{} · {}", first.name, day(first.at));
        let line = elide(painter, &line, 17.0, body(), right - left - 22.0);
        let g = text(painter, &line, 17.0, body(), cream(0.75), 0.0);
        painter.galley(pos2(left + 22.0, y), g, cream(0.75));
    }
}

/// A five-pointed star, point up.
fn star(painter: &Painter, centre: egui::Pos2, r: f32, color: Color32) {
    let at = |i: usize, radius: f32| {
        let a = std::f32::consts::PI * (i as f32) / 5.0 - std::f32::consts::FRAC_PI_2;
        centre + vec2(a.cos(), a.sin()) * radius
    };
    // Five triangles from the centre, each a point and its two inner corners.
    for i in (0..10).step_by(2) {
        let points = vec![centre, at((i + 9) % 10, r * 0.42), at(i, r), at(i + 1, r * 0.42)];
        painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
    }
}

/// `.campaign__board`: the panel the boards sit in, the campaign's night
/// edged in its pink, its corner cut.
pub fn board_panel(painter: &Painter, r: Rect) {
    let cut = 26.0;
    let points = |r: Rect| {
        vec![
            r.left_top(),
            pos2(r.right() - cut, r.top()),
            pos2(r.right(), r.top() + cut),
            r.right_bottom(),
            r.left_bottom(),
        ]
    };
    painter.add(Shape::convex_polygon(
        points(r.translate(vec2(8.0, 9.0))),
        hsl(250.0, 50.0, 3.0),
        Stroke::NONE,
    ));
    gradient_polygon(
        painter,
        &points(r),
        &[
            (0.0, hsla(276.0, 60.0, 16.0, 0.92)),
            (1.0, hsla(262.0, 56.0, 9.0, 0.96)),
        ],
    );
    let mut closed = points(r);
    closed.push(r.left_top());
    painter.add(Shape::line(closed, Stroke::new(2.0, hsla(322.0, 95.0, 62.0, 0.45))));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_day_reads_as_the_browser_writes_it() {
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(day(None), "");
        assert_eq!(day_in(Some(0.0), &utc), "1 Jan 1970");
        // 2026-11-14T12:00:00Z.
        assert_eq!(day_in(Some(1_794_657_600_000.0), &utc), "14 Nov 2026");
        assert_eq!(day_in(Some(951_782_400_000.0), &utc), "29 Feb 2000");
        // The player's own day, not UTC's: 02:00 UTC on 15 Nov is still the
        // 14th five hours west.
        let west = jiff::tz::TimeZone::fixed(jiff::tz::offset(-5));
        assert_eq!(day_in(Some(1_794_708_000_000.0), &west), "14 Nov 2026");
        assert_eq!(day_in(Some(1_794_708_000_000.0), &utc), "15 Nov 2026");
    }

    #[test]
    fn a_board_reads_whole() {
        let row = json!({"rank":1,"playerId":"7","name":"Udon","frags":42,"cleared":3});
        let message = json!({"type":"campaign:board","requestId":"cb","missionCount":8,
            "scorers":[row],"finishers":[],
            "missions":[{"id":"m1","number":1,"scorers":[row],"firstClears":[{"rank":1,"playerId":"7","name":"Udon","at":0}]}]});
        let board = read_board(&message).unwrap();
        assert_eq!(board.mission_count, 8);
        assert_eq!(board.scorers[0].frags, Some(42));
        assert_eq!(board.scorers[0].cleared, Some(3));
        assert_eq!(board.missions[0].first_clears[0].at, Some(0.0));
        assert!(read_board(&json!({"type":"campaign:board"})).is_none());
    }
}
