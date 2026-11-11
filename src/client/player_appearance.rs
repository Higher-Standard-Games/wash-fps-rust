//! wash-fps `client/RemotePlayerRenderer.ts`'s choice of each player's body
//! and colours: `resolvePlayerTeamSlot`, `visualStyle`, `ffaColor`,
//! `assignedLook`, `assignedModel` and `modelChoiceFor`.
//!
//! A player is an ally or an enemy to the viewer: the followed player in a
//! demo (`setPerspectiveEntity`), else the local player's team. In a
//! free-for-all the server hands each player a body and a colour (the look
//! byte); every viewer and demo then shows a player the same way. Your own
//! colour stays yours, but your body is the server's too.

use std::collections::HashMap;

use shagger::animation::player_model_manager::PlayerModelVisualStyle;

use crate::client::player_model_selection::{PlayerModelSide, player_model_choice};
use crate::client::player_model_visual_settings::{
    FfaColors, PlayerTeamSlot, get_ffa_player_model_visual_style, get_player_model_visual_style, get_player_team_slot,
};
use crate::shared::players::player_look::{Look, decode_look};

/// Who is looking, and at whom: one frame's roster.
#[derive(Debug, Clone, Copy)]
pub struct AppearanceView<'a> {
    /// Each player's team by entity ("ffa" for none).
    pub teams: &'a HashMap<String, String>,
    /// Each free-for-all player's look byte by entity.
    pub looks: &'a HashMap<String, u8>,
    /// The player whose eyes a demo borrows (`perspectiveEntityId`).
    pub perspective: Option<&'a str>,
    /// The local player in live play (`ownEntityId`); a demo has none.
    pub own: Option<&'a str>,
    /// A team gametype (Duel included): the side colours apply.
    pub team_based: bool,
}

impl AppearanceView<'_> {
    /// `resolvePlayerTeamSlot`.
    #[must_use]
    pub fn slot(&self, entity_id: &str) -> PlayerTeamSlot {
        let team = |id: &str| self.teams.get(id).map(String::as_str);
        match self.perspective {
            Some(viewer) if viewer == entity_id => PlayerTeamSlot::Ally,
            Some(viewer) => get_player_team_slot(team(entity_id), team(viewer)),
            None => get_player_team_slot(team(entity_id), self.own.and_then(team)),
        }
    }

    /// `assignedLook`: a free-for-all opponent's server-assigned look, or
    /// `None` to use your settings.
    #[must_use]
    pub fn assigned_look(&self, entity_id: &str) -> Option<Look> {
        if self.team_based || Some(entity_id) == self.perspective {
            return None;
        }
        self.looks.get(entity_id).copied().and_then(decode_look)
    }

    /// `assignedModel`: the body the server gave a free-for-all player,
    /// yours too.
    #[must_use]
    pub fn assigned_model(&self, entity_id: &str) -> Option<&'static str> {
        if self.team_based {
            return None;
        }
        let look = self.looks.get(entity_id).copied()?;
        let own = Some(entity_id) == self.own;
        if !own && Some(entity_id) != self.perspective && self.slot(entity_id) != PlayerTeamSlot::Ffa {
            return None;
        }
        decode_look(look).map(|l| l.model)
    }

    /// `modelChoiceFor`.
    #[must_use]
    pub fn model_for(&self, entity_id: &str) -> &'static str {
        self.assigned_model(entity_id).unwrap_or_else(|| {
            let ally = Some(entity_id) == self.own || self.slot(entity_id) == PlayerTeamSlot::Ally;
            player_model_choice(if ally {
                PlayerModelSide::Ally
            } else {
                PlayerModelSide::Enemy
            })
        })
    }
}

/// The free-for-all colours kept over a session (`ffaColors`).
#[derive(Debug, Default)]
pub struct PlayerAppearance {
    ffa_colors: FfaColors,
}

impl PlayerAppearance {
    /// `visualStyle`: a free-for-all opponent in their own colour, everyone
    /// else in their side's.
    pub fn style(&mut self, entity_id: &str, view: &AppearanceView<'_>, model_scale: f32) -> PlayerModelVisualStyle {
        let slot = view.slot(entity_id);
        if slot != PlayerTeamSlot::Ffa || view.team_based {
            return get_player_model_visual_style(slot, model_scale);
        }
        let given = view.assigned_look(entity_id).map(|l| l.color);
        let handed_out: Vec<&'static str> = view
            .looks
            .keys()
            .filter_map(|id| view.assigned_look(id).map(|l| l.color))
            .collect();
        let color = self.ffa_colors.color_for(entity_id, given, handed_out);
        get_ffa_player_model_visual_style(color, model_scale)
    }

    /// `resetSession`.
    pub fn clear(&mut self) {
        self.ffa_colors.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(
        teams: &'a HashMap<String, String>,
        looks: &'a HashMap<String, u8>,
        team_based: bool,
    ) -> AppearanceView<'a> {
        AppearanceView {
            teams,
            looks,
            perspective: None,
            own: Some("me"),
            team_based,
        }
    }

    #[test]
    fn a_free_for_all_player_wears_the_servers_body_and_colour() {
        let teams: HashMap<String, String> = [("me", "ffa"), ("bot", "ffa")]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .into();
        // bot: riotkitty (3), red (5); me: robot (4), lime (2)
        let looks: HashMap<String, u8> = [("bot".to_owned(), 0x35), ("me".to_owned(), 0x42)].into();
        let ffa = view(&teams, &looks, false);
        assert_eq!(ffa.model_for("bot"), "riotkitty");
        assert_eq!(ffa.model_for("me"), "robot");
        let mut appearance = PlayerAppearance::default();
        let style = appearance.style("bot", &ffa, 36.0);
        assert_eq!(style.outline_color, shagger::rendering::glam::Vec3::new(2.1, 0.3, 0.3));
        // A team mode keeps the side colours and the bodies you chose.
        let team = view(&teams, &looks, true);
        assert_eq!(team.assigned_model("bot"), None);
        assert_eq!(team.model_for("me"), player_model_choice(PlayerModelSide::Ally));
    }

    #[test]
    fn sides_are_seen_from_the_followed_player() {
        let teams: HashMap<String, String> = [("a", "udon"), ("b", "soba"), ("c", "udon")]
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .into();
        let looks = HashMap::new();
        let mut v = view(&teams, &looks, true);
        v.own = None;
        v.perspective = Some("a");
        assert_eq!(v.slot("a"), PlayerTeamSlot::Ally);
        assert_eq!(v.slot("c"), PlayerTeamSlot::Ally);
        assert_eq!(v.slot("b"), PlayerTeamSlot::Enemy);
        v.perspective = None;
        assert_eq!(v.slot("b"), PlayerTeamSlot::Ffa);
    }
}
