//! wash-fps `client/campaign/CampaignStory.ts`: what the campaign shows for
//! each mission, its name, story and comic card, and the finale's card. The rules (map, gametype,
//! bots, models) come from the lobby; this is keyed by the same mission ids.

/// `CardPanel`: a framed panel of a comic card, in the card image's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardPanel {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// `CampaignCard`: the comic card shown before a mission.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CampaignCard {
    /// The card's number: `assets/campaign/card<n>.webp`.
    pub number: u8,
    pub width: f32,
    pub height: f32,
    /// The panels in reading order; each is shown on its own, the rest
    /// blurred.
    pub panels: &'static [CardPanel],
    /// Which part of the card the page's thumbnail shows (CSS
    /// `object-position`, as fractions).
    pub thumb_focus: (f32, f32),
}

/// `CampaignBriefing`: what to do, shown over the opening countdown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CampaignBriefing {
    pub goal: &'static str,
    pub points: &'static [&'static str],
}

/// `CampaignStory`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CampaignStory {
    pub id: &'static str,
    pub title: &'static str,
    /// The story so far, beside the card on the Campaign page.
    pub intro: &'static str,
    /// One line of story, on the briefing over the opening countdown.
    pub line: &'static str,
    pub rival: &'static str,
    pub briefing: CampaignBriefing,
    /// The comic card; absent until a mission's art is drawn.
    pub card: Option<CampaignCard>,
}

const CARD_W: f32 = 1536.0;
const CARD_H: f32 = 864.0;

const fn panel(x: f32, y: f32, w: f32, h: f32) -> CardPanel {
    CardPanel { x, y, w, h }
}

const fn card(number: u8, panels: &'static [CardPanel], thumb_focus: (f32, f32)) -> Option<CampaignCard> {
    Some(CampaignCard {
        number,
        width: CARD_W,
        height: CARD_H,
        panels,
        thumb_focus,
    })
}

/// `STORIES`, in operation order.
pub const STORIES: [CampaignStory; 8] = [
    CampaignStory {
        id: "cartel-contract",
        title: "Cartel Contract",
        intro: "So you want a Cartel contract? Cute. First, survive three rounds in Washoi Amphi2 against a Cartel King with zero trigger discipline, limited-edition sneakers, and an ego tied directly to live crypto margins.",
        line: "Prove your slide-speed to the Cartel—and don't bleed on the boss's sneakers.",
        rival: "Cartel King",
        briefing: CampaignBriefing {
            goal: "Beat the Cartel King!",
            points: &[
                "Play best of three against the Cartel King.",
                "Most frags takes a three-minute round.",
                "Win two rounds to clear it.",
            ],
        },
        card: card(
            1,
            &[panel(14.0, 14.0, 707.0, 836.0), panel(739.0, 14.0, 783.0, 836.0)],
            (0.78, 0.30),
        ),
    },
    CampaignStory {
        id: "street-riot",
        title: "Street Riot",
        intro: "Congratulations, the Cartel didn't scrap you. Your orientation bonus? Getting dropped straight into a Sector 1 free-for-all swarming with over-caffeinated Glitch Punks and trigger-happy cops. Stay airborne and hoard the armor bubbles.",
        line: "Treat gravity like a suggestion and out-frag a Sector 1 riot.",
        rival: "Street Rioters",
        briefing: CampaignBriefing {
            goal: "Take part in the street riot!",
            points: &[
                "You must finish as top scorer!",
                "The match lasts 10 minutes.",
                "Free for all!",
            ],
        },
        card: card(
            2,
            &[panel(10.0, 10.0, 747.0, 844.0), panel(774.0, 10.0, 753.0, 844.0)],
            (0.70, 0.45),
        ),
    },
    CampaignStory {
        id: "concrete-sabotage",
        title: "Concrete Strike",
        intro: "Time to earn your retainer. The Cartel wants OmniSec's traffic-fine servers vaporized before Monday's billing cycle. Plant the EMP charge at Concrete Strike—just watch out for Metropol Riot Cops running double overtime shifts and treating excessive force like morning cardio.",
        line: "Plant the EMP, wipe the Cartel's parking tickets, and ruin Monday morning.",
        rival: "Metropol Riot Cops",
        briefing: CampaignBriefing {
            goal: "Plant the EMP before Monday's billing cycle!",
            points: &[
                "Win 10 rounds against the Riot Police.",
                "You have 4 teammates.",
                "Plant the EMP on site A or B.",
                "(Or kill everyone in the other team!)",
            ],
        },
        card: card(
            3,
            &[
                panel(6.0, 5.0, 1524.0, 90.0),
                panel(6.0, 99.0, 735.0, 758.0),
                panel(751.0, 99.0, 779.0, 758.0),
            ],
            (0.80, 0.50),
        ),
    },
    CampaignStory {
        id: "glitch-duel",
        title: "Turf Duel",
        intro: "Plot twist: blowing up OmniSec's servers also fried the Glitch Punks' pirate Wi-Fi. Now their neon-haired boss wants your head in a 1v1 turf duel on Washoi DM2. Cut her off at the Mega Health before she rockets her way across the map.",
        line: "You fried their pirate Wi-Fi—now survive the Glitch Punk boss's rockets.",
        rival: "Glitch Punk Leader",
        briefing: CampaignBriefing {
            goal: "Take down the Glitch Punk Leader!",
            points: &[
                "One on one, for 10 minutes.",
                "Finish with more frags than her.",
                "A tie goes to overtime.",
            ],
        },
        card: card(
            4,
            &[panel(8.0, 7.0, 684.0, 848.0), panel(708.0, 7.0, 821.0, 848.0)],
            (0.75, 0.40),
        ),
    },
    CampaignStory {
        id: "bot-trap",
        title: "Bot Trap",
        intro: "A \"100% reliable\" Cartel tip led you straight into a rigged rival vault. With the exit shutters welded shut and three floors of warranty-voided security bots dropping from the rafters, there's only one way out: survive the swarm until the locks blow.",
        line: "Three floors of warranty-voided scrap-bots and zero exit codes.",
        rival: "Rival Cartel Bot Guards",
        briefing: CampaignBriefing {
            goal: "Survive the bot trap!",
            points: &[
                "Last 5 minutes. You get one respawn: a second death ends the mission.",
                "You start with the blade and a machine gun: grab more from the map.",
                "More robots every minute, and tougher.",
                "You heal slowly while you live.",
            ],
        },
        card: card(
            5,
            &[
                panel(8.0, 8.0, 697.0, 469.0),
                panel(8.0, 487.0, 697.0, 369.0),
                panel(719.0, 8.0, 809.0, 848.0),
            ],
            (0.74, 0.45),
        ),
    },
    CampaignStory {
        id: "siege-breaker",
        title: "Siege Breaker",
        intro: "Word of your vault breakout spread fast—and a rogue Spec-Ops handler just unclamped a giant Unit 07 Siege Bot to see how long you last. Crouch-slide beneath its rotary fire in Washoi Amphi1 and turn this expired war machine into scrap.",
        line: "Crouch-slide the rotary miniguns and scrap a giant war titan.",
        rival: "Heavy Siege Bot",
        briefing: CampaignBriefing {
            goal: "Break the Heavy Siege Bot!",
            points: &[
                "Play best of three against a giant robot.",
                "It has 200 health, 200 armor and a new kit every life.",
                "Mind the slime!",
            ],
        },
        card: card(
            6,
            &[panel(8.0, 8.0, 751.0, 848.0), panel(778.0, 8.0, 749.0, 848.0)],
            (0.72, 0.35),
        ),
    },
    CampaignStory {
        id: "depot-heist",
        title: "Depot Heist",
        intro: "Your titan-scrapping highlight reel made you famous—and one final sabotage gig at OmniSec's freight depot pays enough for a skyline penthouse. Only problem? That same rogue Spec-Ops merc and a freshly updated Unit 07 took the defense contract to pay off their monthly firewall bills.",
        line: "Breach the freight yard before Unit 07's antivirus auto-renews.",
        rival: "Rogue Spec-Ops and Unit 07",
        briefing: CampaignBriefing {
            goal: "Pull off the Depot cash heist!",
            points: &[
                "Win 10 rounds against the spec-ops.",
                "You have 4 teammates.",
                "Plant the EMP on site A or B.",
                "(Or kill everyone in the other team!)",
            ],
        },
        card: card(
            7,
            &[
                panel(7.0, 7.0, 877.0, 476.0),
                panel(7.0, 493.0, 877.0, 365.0),
                panel(896.0, 7.0, 633.0, 850.0),
            ],
            (0.78, 0.55),
        ),
    },
    CampaignStory {
        id: "road-ends",
        title: "Road's End",
        intro: "It's a setup. That depot payload wasn't cash—it was a live OmniSec tracking beacon. Now Washoi DM5 is sealed under championship lockdown, and Metropol's top Male and Female riot enforcers have synchronized their HUDs for a regulatory two-on-one execution. Break the blockade and take the crown.",
        line: "Two badges, double overtime, and zero mercy—break the blockade.",
        rival: "Metropol Police Duo",
        briefing: CampaignBriefing {
            goal: "Your road ends here. Or theirs.",
            points: &[
                "You alone against the Metropol Police duo.",
                "A full 10-minute match.",
                "Lead when the time is up to win.",
            ],
        },
        card: card(
            8,
            &[panel(6.0, 6.0, 756.0, 853.0), panel(772.0, 6.0, 758.0, 853.0)],
            (0.72, 0.45),
        ),
    },
];

/// `FINALE_CARD`: the card for a finished campaign, under the last
/// operation once every one is cleared.
pub const FINALE_CARD: CampaignCard = CampaignCard {
    number: 9,
    width: 2752.0,
    height: 1536.0,
    panels: &[panel(11.0, 5.0, 1354.0, 1527.0), panel(1385.0, 5.0, 1357.0, 1527.0)],
    thumb_focus: (0.72, 0.45),
};

/// `campaignStory`: the story for a mission id, or `None` for one this
/// client has no art for.
#[must_use]
pub fn campaign_story(mission_id: &str) -> Option<&'static CampaignStory> {
    STORIES.iter().find(|s| s.id == mission_id)
}

/// `campaignOperationNumber`: a mission's place in the campaign from 1.
#[must_use]
pub fn campaign_operation_number(mission_id: &str) -> Option<usize> {
    STORIES.iter().position(|s| s.id == mission_id).map(|i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missions_are_found_by_id_in_order() {
        assert_eq!(campaign_story("glitch-duel").map(|s| s.title), Some("Turf Duel"));
        assert_eq!(campaign_operation_number("bot-trap"), Some(5));
        assert_eq!(campaign_story("depot-heist").unwrap().card.map(|c| c.number), Some(7));
        assert_eq!(campaign_story("nope"), None);
        // Every panel lies on its card.
        for card in STORIES.iter().filter_map(|s| s.card).chain([FINALE_CARD]) {
            {
                for p in card.panels {
                    assert!(p.x >= 0.0 && p.y >= 0.0 && p.x + p.w <= card.width && p.y + p.h <= card.height);
                }
            }
        }
    }
}
