//! Port of wash-fps `liquid/client/LocalAirTracker.ts`: the local player's
//! air, mirrored for presentation with the server's own rules. The
//! callbacks (`onStartedDrowning`, `onDrownTick`) are the returned
//! [`AirEvent`].

use crate::liquid::liquid_types::WaterLevel;
use crate::server::systems::liquid_world_effects::{LiquidEffectState, air_fraction, tick_liquid_effects};

/// What a tick asks to be heard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AirEvent {
    /// The moment air runs out: the gasp.
    StartedDrowning,
    /// Each drowning tick after: the gurgle.
    DrownTick,
}

/// `LocalAirTracker`.
#[derive(Debug, Clone, Default)]
pub struct LocalAirTracker {
    state: LiquidEffectState,
}

impl LocalAirTracker {
    /// `update`: lava and slime are the server's business; this is air.
    pub fn update(&mut self, level: WaterLevel, delta_time: f64) -> Option<AirEvent> {
        let result = tick_liquid_effects(&mut self.state, level, false, false, delta_time);
        if result.started_drowning {
            Some(AirEvent::StartedDrowning)
        } else if result.drown_damage > 0.0 {
            Some(AirEvent::DrownTick)
        } else {
            None
        }
    }

    /// 1 full, 0 empty.
    #[must_use]
    pub fn air(&self) -> f64 {
        air_fraction(&self.state)
    }

    /// Once the lungs are empty.
    #[must_use]
    pub fn drowning(&self) -> bool {
        self.state.air <= 0.0
    }

    /// Refill on respawn.
    pub fn reset(&mut self) {
        self.state = LiquidEffectState::default();
    }
}
