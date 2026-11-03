//! Port of wash-fps `client/ReelCards.ts`: a highlight reel's opening and
//! closing cards, who played and how it went, plus `shotCard` from
//! `CinematicDemoPlayer.ts`. Pure data from the demo; the window draws them.

use shagger::shared::demo::demo_analyzer::{DemoAnalysis, HighlightType};
use shagger::shared::demo::demo_reader::DemoPlayerInfo;
use shagger::shared::demo::sequence_builder::{CinematicReel, CinematicSegment};

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
