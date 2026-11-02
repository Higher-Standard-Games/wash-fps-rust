//! wash-fps `client/FPSReloadAudio.ts`: one cue per reload edge.

use std::collections::HashMap;

use shagger::audio::audio_v2_sfx::{AudioV2Sfx, SfxPlayOptions};
use shagger::rendering::glam::DVec3;

use crate::client::game_events::canonical_weapon_id;
use crate::shared::entities::definitions::weapons::weapons_table::weapon_reload_model;

pub const RELOAD_SOUND_PATH: &str = "sounds/weapons/reload.ogg";

#[derive(Debug, Clone, Default)]
pub struct FpsReloadAudio {
    states: HashMap<String, (String, bool)>,
}

impl FpsReloadAudio {
    /// `observe`: a `weapon_state` of `entity_id`, heard 2D when it is the
    /// viewed player and otherwise at `position`.
    pub fn observe(
        &mut self,
        audio: &mut AudioV2Sfx,
        entity_id: &str,
        current_weapon: &str,
        reloading: bool,
        viewed: bool,
        position: Option<DVec3>,
    ) {
        let weapon = canonical_weapon_id(current_weapon).to_owned();
        let previous = self.states.insert(entity_id.to_owned(), (weapon.clone(), reloading));
        if !reloading || weapon_reload_model(&weapon) != Some("clip") || previous.is_some_and(|(w, r)| w == weapon && r)
        {
            return;
        }
        let options = SfxPlayOptions {
            volume: Some(0.65),
            max_instances: Some(2),
            ..SfxPlayOptions::default()
        };
        if viewed {
            audio.play_2d(RELOAD_SOUND_PATH, &options);
        } else if let Some(position) = position {
            audio.play_3d(RELOAD_SOUND_PATH, position, &options);
        }
    }

    /// `prune`: forgets the entities that are gone.
    pub fn prune<'a>(&mut self, entity_ids: impl IntoIterator<Item = &'a str>) {
        let alive: std::collections::HashSet<&str> = entity_ids.into_iter().collect();
        self.states.retain(|id, _| alive.contains(id.as_str()));
    }

    pub fn reset(&mut self) {
        self.states.clear();
    }
}
