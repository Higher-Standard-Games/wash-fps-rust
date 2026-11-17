//! The lobby panel's campaign tab (`LobbyPanel.ts`'s campaign functions).
//! Split out of `lobby_panel.rs`; the same module tree.

use super::*;

/// The campaign (`LobbyPanel.ts`'s campaign tab).
impl LobbyPanel {
    /// `refreshCampaign`: mission keys work with a lobby that runs the
    /// campaign and no room open.
    pub(super) fn refresh_campaign(&mut self) {
        let reason = if !self.connected {
            "The campaign needs the lobby server."
        } else if !self.campaign_supported {
            "This lobby server does not run the campaign."
        } else if self.room.is_some() && self.starting_mission.is_none() {
            "Leave your room on the Play page to start a mission."
        } else {
            ""
        };
        let startable = reason.is_empty() && !self.queue_pending && self.starting_mission.is_none();
        self.campaign.set_startable(startable, reason);
    }

    /// `retryCampaignMission`: play again the mission the player asked to
    /// retry, straight from the black screen.
    pub(super) fn retry_campaign_mission(&mut self) {
        let Some(mission_id) = self.campaign_retry.clone() else {
            return;
        };
        let Some((missions, unlocked_through)) = &self.campaign_missions else {
            return;
        };
        self.campaign_retry = None;
        let mission = missions.iter().find(|m| m.id == mission_id).cloned();
        if let Some(mission) = mission
            && mission.number <= *unlocked_through
        {
            self.start_campaign_mission(mission, true);
        }
    }

    /// `startCampaignMission`: the comic card plays on a black screen
    /// while the lobby sets up the mission's room and bots behind it, and
    /// the player readies up once the card has been read. A retry skips the
    /// card.
    pub fn start_campaign_mission(&mut self, mission: CampaignMissionInfo, retry: bool) {
        if self.starting_mission.is_some() || self.room.is_some() || self.queue_pending {
            return;
        }
        if !self.connected || !self.campaign_supported {
            return;
        }
        let story = campaign_story(&mission.id);
        let title = story.map_or(mission.map_title.as_str(), |s| s.title);
        let card = if retry { None } else { story.and_then(|s| s.card) };
        let mut cutscene = CampaignCutscene::new(card, title);
        cutscene.set_status(&format!("Deploying to {}…", mission.map_title));
        self.cutscene = Some(cutscene);
        self.queue_pending = true;
        self.levelshots.want([mission.map.as_str()]);
        self.client.create_campaign_lobby(&mission.id, &mission.gametype);
        self.starting_mission = Some(mission);
        self.refresh_campaign();
    }

    /// Starts the mission to play next, as its Play key does: for
    /// `washoi menu --page campaign --play`. True once it has started.
    pub fn start_next_campaign_mission(&mut self) -> bool {
        self.refresh_campaign();
        let Some(state) = self.campaign.state() else {
            return false;
        };
        let next = state
            .missions
            .iter()
            .find(|m| {
                crate::client::campaign::campaign_view::mission_status(m.number, Some(state))
                    == crate::client::campaign::campaign_view::MissionStatus::Current
            })
            .cloned();
        let Some(mission) = next else { return false };
        if !self.connected || !self.campaign_supported || self.room.is_some() || self.queue_pending {
            return false;
        }
        self.start_campaign_mission(mission, false);
        self.starting_mission.is_some()
    }

    /// `readyForCampaign`: the card is read and the room open: ready up;
    /// the lobby starts the match once every mission bot is seated too.
    pub(super) fn ready_for_campaign(&mut self) {
        let read = self.cutscene.as_ref().is_some_and(CampaignCutscene::panels_read);
        let (Some(room), Some(mission)) = (&self.room, &self.starting_mission) else {
            return;
        };
        if !read {
            return;
        }
        let status = campaign_story(&mission.id).map_or_else(
            || "Starting the mission…".to_owned(),
            |s| format!("{} incoming…", s.rival),
        );
        if let Some(cut) = &mut self.cutscene {
            cut.set_status(&status);
        }
        self.client.set_ready(&room.lobby_id, true);
    }

    /// `failCampaignStart`: a mission that was starting has failed: say so
    /// on the card, which offers the way back to the campaign.
    pub(super) fn fail_campaign_start(&mut self, message: &str) {
        let (Some(cut), Some(_)) = (&mut self.cutscene, &self.starting_mission) else {
            return;
        };
        cut.fail(message);
        self.starting_mission = None;
        self.queue_pending = false;
    }

    /// The comic card over everything, and what it asks for.
    pub(super) fn cutscene_ui(&mut self, ctx: &egui::Context) {
        let Some(cut) = &mut self.cutscene else { return };
        let event = cut.ui(ctx, &mut self.campaign.art);
        let gone = cut.is_gone();
        match event {
            // A card read on its own closes once read.
            Some(CutsceneEvent::PanelsDone) if self.starting_mission.is_none() => cut.close(),
            Some(CutsceneEvent::PanelsDone) => self.ready_for_campaign(),
            Some(CutsceneEvent::Back) => {
                if let Some(room) = &self.room {
                    self.client.leave_lobby(&room.lobby_id);
                }
                self.show_play_stage();
                self.room_to_rejoin = None;
                self.tab = Tab::Campaign;
            }
            None => {}
        }
        if gone {
            self.cutscene = None;
        }
    }
}
