//! Port of wash-fps `client/ReelCards.ts`: the opening and closing cards of a
//! highlight reel or a whole demo, who played and how it went, plus
//! `shotCard` from `CinematicDemoPlayer.ts`. Pure data from the demo; the
//! window draws them.

use shagger::shared::demo::demo_analyzer::{DemoAnalysis, HighlightType};
use shagger::shared::demo::demo_reader::DemoPlayerInfo;
use shagger::shared::demo::sequence_builder::{CinematicReel, CinematicSegment};
use shagger::shared::js::js_round;

/// Names shown for a free-for-all before the rest are counted.
const FFA_NAMES_SHOWN: usize = 8;

/// One side of the match: a team with its players, or everyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReelSide {
    /// The team's id ("udon", "soba"); none for a duel's player or a free-for-all.
    pub team: Option<String>,
    pub label: Option<String>,
    pub names: Vec<String>,
}

/// `ReelTitle`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReelTitle {
    /// Two sides face each other ("vs" between); one side is everyone.
    pub sides: Vec<ReelSide>,
    /// Players a free-for-all's list leaves out.
    pub more: usize,
}

fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// `reelTitle`: teams as two sides (Udon first), a duel as its two players,
/// a free-for-all as everyone up to [`FFA_NAMES_SHOWN`].
#[must_use]
pub fn reel_title(players: &[DemoPlayerInfo]) -> ReelTitle {
    let players: Vec<&DemoPlayerInfo> = players.iter().filter(|p| !p.name.is_empty()).collect();
    let mut teams: Vec<(String, Vec<String>)> = Vec::new();
    for p in &players {
        if p.team.is_empty() || p.team == "spectator" {
            continue;
        }
        match teams.iter_mut().find(|(t, _)| *t == p.team) {
            Some((_, names)) => names.push(p.name.clone()),
            None => teams.push((p.team.clone(), vec![p.name.clone()])),
        }
    }
    if teams.len() >= 2 {
        // Stable, like `Array.prototype.sort`: Udon first, the rest in order.
        teams.sort_by_key(|(team, _)| team != "udon");
        let sides = teams
            .into_iter()
            .map(|(team, names)| ReelSide {
                label: Some(capitalize(&team)),
                team: Some(team),
                names,
            })
            .collect();
        return ReelTitle { sides, more: 0 };
    }
    let side = |names: Vec<String>| ReelSide {
        team: None,
        label: None,
        names,
    };
    if players.len() == 2 {
        return ReelTitle {
            sides: players.iter().map(|p| side(vec![p.name.clone()])).collect(),
            more: 0,
        };
    }
    let names: Vec<String> = players.iter().map(|p| p.name.clone()).collect();
    ReelTitle {
        more: names.len().saturating_sub(FFA_NAMES_SHOWN),
        sides: vec![side(names.into_iter().take(FFA_NAMES_SHOWN).collect())],
    }
}

/// `ReelSummary`: the closing card.
#[derive(Debug, Clone, PartialEq)]
pub struct ReelSummary {
    pub top_fragger: Option<(String, u32)>,
    /// The reel's best-scoring frag: its title and player.
    pub best: Option<(String, String)>,
    pub frags: usize,
    pub tricks: usize,
}

/// `reelSummary`: the match's top fragger and the reel's best moment.
#[must_use]
pub fn reel_summary(reel: &CinematicReel, analysis: &DemoAnalysis) -> ReelSummary {
    // Each highlight once, in the order its first shot comes.
    let mut highlights: Vec<&CinematicSegment> = Vec::new();
    for s in &reel.segments {
        if !highlights.iter().any(|h| h.highlight == s.highlight) {
            highlights.push(s);
        }
    }
    let frag_scenes: Vec<&CinematicSegment> = highlights
        .iter()
        .copied()
        .filter(|s| s.highlight.kind != HighlightType::Movement)
        .collect();
    let mut top: Option<&(String, u32)> = None;
    for entry in &analysis.frags {
        if top.is_none_or(|t| entry.1 > t.1) {
            top = Some(entry);
        }
    }
    let best = frag_scenes
        .iter()
        .copied()
        .fold(None::<&CinematicSegment>, |best, s| match best {
            Some(b) if b.highlight.score >= s.highlight.score => Some(b),
            _ => Some(s),
        })
        .map(|s| {
            (
                shot_card(s, reel.total_scenes).title,
                s.highlight.primary_player_name.clone(),
            )
        });
    ReelSummary {
        top_fragger: top.map(|(id, n)| (analysis.player_names.get(id).cloned().unwrap_or_else(|| id.clone()), *n)),
        best,
        frags: frag_scenes.len(),
        tricks: highlights.len() - frag_scenes.len(),
    }
}

/// `EndCard`: a closing card as drawn, labelled chips over a line of counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndCard {
    /// `(label, value)`.
    pub chips: Vec<(String, String)>,
    pub counts: String,
}

/// `plural`: a count and its noun; a number prints as JavaScript's does for
/// the counts here.
fn plural(count: f64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1.0 { one } else { many })
}

/// `reelEndCard`: a reel's closing card, its top fragger, best moment and scenes.
#[must_use]
pub fn reel_end_card(summary: &ReelSummary) -> EndCard {
    let mut chips = Vec::new();
    if let Some((name, frags)) = &summary.top_fragger {
        chips.push((
            "Top fragger".to_owned(),
            format!("{name} · {}", plural(f64::from(*frags), "frag", "frags")),
        ));
    }
    if let Some((title, player)) = &summary.best {
        chips.push(("Best moment".to_owned(), format!("{title} · {player}")));
    }
    let mut counts = vec![plural(summary.frags as f64, "highlight", "highlights")];
    if summary.tricks > 0 {
        counts.push(plural(summary.tricks as f64, "trick jump", "trick jumps"));
    }
    EndCard {
        chips,
        counts: counts.join(" · "),
    }
}

/// A name and a score: a player's frags, or a side's match score.
#[derive(Debug, Clone, PartialEq)]
pub struct Standing {
    pub name: String,
    pub score: f64,
}

/// `DemoFinal`: how a whole demo ended, the scores on its last frame.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DemoFinal {
    /// Every player's score, the HUD's frag count.
    pub players: Vec<Standing>,
    /// Each side's match score where the mode has sides: teams, or a duel's
    /// or Amphi's two players, whose round wins decide it. None in a
    /// free-for-all.
    pub sides: Option<Vec<Standing>>,
    /// Frags by a player over the whole demo, suicides and falls left out.
    pub frags: u32,
    pub duration_ms: f64,
}

/// `leader`: whoever is alone on the top score, or none on a tie.
fn leader(entries: &[Standing]) -> Option<&Standing> {
    let mut sorted: Vec<&Standing> = entries.iter().collect();
    // Stable, as `Array.prototype.sort` with `b.score - a.score`.
    sorted.sort_by(|a, b| b.score.total_cmp(&a.score));
    match sorted.as_slice() {
        [only] => Some(only),
        [first, second, ..] if first.score > second.score => Some(first),
        _ => None,
    }
}

/// `demoEndCard`: a whole demo's closing card. A recording stops as the
/// post-game begins, before the last round reaches the match score, so the
/// winner is read from the final scores: the leading side where the mode
/// has sides, or else the player with most frags. Sides of one player that
/// stand level go to the frags of the round that ended it; teams that stand
/// level get no winner, as the recording cannot tell. Teams of several
/// players get their top fragger.
#[must_use]
pub fn demo_end_card(final_: &DemoFinal) -> EndCard {
    let frags = |s: &Standing| format!("{} · {}", s.name, plural(s.score, "frag", "frags"));
    let mut chips = Vec::new();
    match &final_.sides {
        Some(sides) if !sides.is_empty() => {
            let teams = final_.players.len() > sides.len();
            let winner = match leader(sides) {
                Some(side) => Some(side.name.clone()),
                None if teams => None,
                None => Some(leader(&final_.players).map_or_else(|| "Draw".to_owned(), |p| p.name.clone())),
            };
            if let Some(winner) = winner {
                chips.push(("Winner".to_owned(), winner));
            }
            if teams && let Some(top) = leader(&final_.players) {
                chips.push(("Top fragger".to_owned(), frags(top)));
            }
        }
        _ if !final_.players.is_empty() => {
            let winner = leader(&final_.players).map_or_else(|| "Draw".to_owned(), frags);
            chips.push(("Winner".to_owned(), winner));
        }
        _ => {}
    }
    let seconds = js_round(final_.duration_ms / 1000.0) as i64;
    let length = format!("{}:{:02}", seconds.div_euclid(60), seconds.rem_euclid(60));
    EndCard {
        chips,
        counts: format!("{} · {length}", plural(f64::from(final_.frags), "frag", "frags")),
    }
}

/// A shot's card, as the controls show it when the shot starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotCard {
    pub kicker: String,
    pub title: String,
    pub player: String,
    pub detail: String,
    pub tags: Vec<String>,
}

fn highlight_label(kind: HighlightType) -> &'static str {
    match kind {
        HighlightType::Frag => "Frag",
        HighlightType::MultiKill => "Multi kill",
        HighlightType::AirborneFrag => "Airborne frag",
        HighlightType::StreakKill => "Streak kill",
        HighlightType::FirstBlood => "First blood",
        HighlightType::Movement => "Trick jump",
    }
}

fn tag_label(tag: &str) -> String {
    match tag {
        "airborne" => "Air",
        "rocket_jump" => "RJ",
        "low_hp" => "Low HP",
        "close_range" => "Close",
        "long_range" => "Long",
        "multi_kill" => "Multi",
        "revenge" => "Revenge",
        "first_blood" => "First",
        "on_fire" => "On fire",
        "raging" => "Raging",
        "unstoppable" => "Unstoppable",
        "high_speed" => "Speed",
        "trick_jump" => "Combo",
        "height_gain" => "Climb",
        "wall_jump" => "Wall jump",
        "slide" => "Slide",
        "bunny_hop" => "Hops",
        other => return other.replace('_', " "),
    }
    .to_owned()
}

/// `shotCard`.
#[must_use]
pub fn shot_card(segment: &CinematicSegment, total_scenes: usize) -> ShotCard {
    let h = &segment.highlight;
    let kills = match h.kill_count {
        Some(n) if n > 1 => format!(" x{n}"),
        _ => String::new(),
    };
    ShotCard {
        kicker: format!(
            "{} {}/{}",
            if segment.is_replay { "Replay" } else { "Highlight" },
            segment.scene + 1,
            total_scenes
        ),
        title: format!("{}{kills}", highlight_label(h.kind)),
        player: match &h.victim_player_name {
            Some(victim) => format!("{} » {victim}", h.primary_player_name),
            None => h.primary_player_name.clone(),
        },
        detail: h.weapon_id.as_deref().unwrap_or("").replace('_', " "),
        tags: h.tags.iter().map(|t| tag_label(t.as_str())).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standings(list: &[(&str, f64)]) -> Vec<Standing> {
        list.iter()
            .map(|(name, score)| Standing {
                name: (*name).to_owned(),
                score: *score,
            })
            .collect()
    }

    fn chips(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(l, v)| ((*l).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn draws_a_reels_summary_as_its_end_card() {
        let summary = ReelSummary {
            top_fragger: Some(("Ann".to_owned(), 7)),
            best: None,
            frags: 2,
            tricks: 1,
        };
        assert_eq!(
            reel_end_card(&summary),
            EndCard {
                chips: chips(&[("Top fragger", "Ann · 7 frags")]),
                counts: "2 highlights · 1 trick jump".to_owned(),
            }
        );
    }

    #[test]
    fn closes_a_whole_demo_on_its_winner_from_the_final_scores() {
        let players = standings(&[("Ann", 9.0), ("Bo", 14.0)]);
        let card = |players: &[Standing], sides: Option<&[Standing]>, frags: u32, duration_ms: f64| {
            demo_end_card(&DemoFinal {
                players: players.to_vec(),
                sides: sides.map(<[Standing]>::to_vec),
                frags,
                duration_ms,
            })
        };
        assert_eq!(
            card(&players, None, 25, 125_400.0),
            EndCard {
                chips: chips(&[("Winner", "Bo · 14 frags")]),
                counts: "25 frags · 2:05".to_owned(),
            }
        );
        let level = standings(&[("Ann", 3.0), ("Bo", 3.0)]);
        assert_eq!(card(&level, None, 6, 59_000.0).chips, chips(&[("Winner", "Draw")]));
        let teams = standings(&[("Udon", 7.0), ("Soba", 5.0)]);
        let mut four = players.clone();
        four.extend(standings(&[("Cy", 2.0), ("Di", 1.0)]));
        assert_eq!(
            card(&four, Some(&teams), 1, 60_000.0),
            EndCard {
                chips: chips(&[("Winner", "Udon"), ("Top fragger", "Bo · 14 frags")]),
                counts: "1 frag · 1:00".to_owned(),
            }
        );
        // One player a side, as in Amphi: its round wins decide, not the last round's frags.
        let rounds = standings(&[("Ann", 3.0), ("Bo", 1.0)]);
        assert_eq!(
            card(&players, Some(&rounds), 30, 60_000.0).chips,
            chips(&[("Winner", "Ann")])
        );
        // Level on rounds: the last round, still on the frag counts, decided it.
        let level2 = standings(&[("Ann", 2.0), ("Bo", 2.0)]);
        assert_eq!(
            card(&players, Some(&level2), 30, 60_000.0).chips,
            chips(&[("Winner", "Bo")])
        );
        // Teams level: no winner the recording can show.
        assert_eq!(
            card(&four, Some(&level2), 30, 60_000.0).chips,
            chips(&[("Top fragger", "Bo · 14 frags")])
        );
        assert_eq!(
            card(&[], None, 0, 59_000.0),
            EndCard {
                chips: Vec::new(),
                counts: "0 frags · 0:59".to_owned(),
            }
        );
    }

    fn player(name: &str, team: &str) -> DemoPlayerInfo {
        DemoPlayerInfo {
            player_id: name.to_owned(),
            name: name.to_owned(),
            team: team.to_owned(),
            account_id: None,
        }
    }

    #[test]
    fn titles_match_the_typescript() {
        let teams = reel_title(&[player("a", "soba"), player("b", "udon"), player("c", "soba")]);
        assert_eq!(teams.sides.len(), 2);
        assert_eq!(teams.sides[0].label.as_deref(), Some("Udon"));
        assert_eq!(teams.sides[1].names, ["a", "c"]);
        let duel = reel_title(&[player("a", ""), player("", ""), player("b", "")]);
        assert_eq!(duel.sides.len(), 2);
        let ffa: Vec<_> = (0..10).map(|i| player(&format!("p{i}"), "")).collect();
        let ffa = reel_title(&ffa);
        assert_eq!((ffa.sides[0].names.len(), ffa.more), (8, 2));
    }
}
